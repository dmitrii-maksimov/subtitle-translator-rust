//! Parallel windowed subtitle translation, ported from
//! `core/translation_engine.py`.
//!
//! Instead of a Python generator yielding progress, this takes a `progress`
//! callback (invoked from the calling thread as results complete) and a
//! cancellation flag. Fan-out uses a bounded rayon pool.

use crate::services::TranslationService;
use crate::settings::AppSettings;
use crate::srt::{self, Subtitle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Progress emitted while translating.
pub enum Progress {
    /// Percentage 0-100.
    Percent(u8),
    /// Human-readable status line.
    Status(String),
    /// Full-log debug line (only when `fulllog` is on).
    Log(String),
}

struct GroupSpec {
    task_id: usize,
    core_start: usize,
    core_end: usize,
    trans_start: usize,
    trans_end: usize,
}

enum ParseKind {
    /// Parsed as SRT: cue contents in translated-window order.
    Ok(Vec<String>),
    /// Numbered blocks / line fallback: content strings in group order.
    Mapped(Vec<String>),
}

struct GroupResult {
    kind: ParseKind,
    debug: Option<serde_json::Value>,
}

/// Translate `entries`, returning the ordered translated list. Calls `progress`
/// for status/percent updates; honors `cancel` cooperatively.
///
/// `lang_name` is the resolved human-readable target language for the prompt
/// header (empty → fall back to `target_lang`); `expected_ranges` are the
/// Unicode letter ranges of the target script used to validate each window
/// (empty → validation skipped).
#[allow(clippy::too_many_arguments)]
pub fn translate_subs(
    entries: &[Subtitle],
    translator: &TranslationService,
    settings: &AppSettings,
    target_lang: &str,
    lang_name: &str,
    expected_ranges: &[(u32, u32)],
    cancel: &Arc<AtomicBool>,
    fulllog: bool,
    progress: &(dyn Fn(Progress) + Sync),
) -> Result<Vec<Subtitle>, String> {
    if entries.is_empty() {
        return Ok(vec![]);
    }

    let window = settings.window.max(1) as usize;
    let overlap = settings.overlap as usize;
    let n = entries.len();
    let step = window.max(1);
    let half = overlap / 2;

    let mut groups: Vec<GroupSpec> = Vec::new();
    let mut task_id = 0usize;
    let mut s = 0usize;
    while s < n {
        let core_start = s;
        let core_end = (s + window).min(n);
        let trans_start = core_start.saturating_sub(half);
        let trans_end = (core_end + half).min(n);
        task_id += 1;
        groups.push(GroupSpec {
            task_id,
            core_start,
            core_end,
            trans_start,
            trans_end,
        });
        s += step;
    }

    let total_groups = groups.len();
    let max_workers = (settings.workers.max(1)).min(10) as usize;
    progress(Progress::Status(format!(
        "Submitting {total_groups} groups to {max_workers} workers..."
    )));

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(max_workers)
        .build()
        .map_err(|e| format!("failed to build thread pool: {e}"))?;

    let completed = std::sync::atomic::AtomicUsize::new(0);
    // Shared cursor so groups are handed out strictly in order: the first
    // `max_workers` are picked up as 1,2,3,... and each worker grabs the next
    // sequential group as soon as it frees up (instead of rayon splitting the
    // range into per-thread chunks). Results still arrive in completion order
    // and are reassembled by index below.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let progress_ref = &progress;

    let results_mutex: std::sync::Mutex<Vec<(usize, Result<GroupResult, String>)>> =
        std::sync::Mutex::new(Vec::with_capacity(total_groups));
    pool.install(|| {
        rayon::scope(|s| {
            for _ in 0..max_workers {
                s.spawn(|_| loop {
                    let spec_idx = next.fetch_add(1, Ordering::SeqCst);
                    if spec_idx >= total_groups || cancel.load(Ordering::SeqCst) {
                        break;
                    }
                    let g = &groups[spec_idx];
                    let res = translate_group(
                        entries,
                        translator,
                        target_lang,
                        lang_name,
                        expected_ranges,
                        g,
                        *progress_ref,
                    );
                    if let Ok(gr) = &res {
                        // Emit progress as each group finishes.
                        if fulllog {
                            if let Some(dbg) = &gr.debug {
                                emit_fulllog(progress_ref, g.task_id, dbg);
                            }
                        }
                        let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                        let pct = (done as f64 / total_groups as f64 * 80.0) as u8;
                        progress_ref(Progress::Percent(pct));
                        progress_ref(Progress::Status(format!(
                            "Translated group {}/{} (core {}-{}, translated {}-{})",
                            g.task_id,
                            total_groups,
                            g.core_start + 1,
                            g.core_end,
                            g.trans_start + 1,
                            g.trans_end
                        )));
                    }
                    results_mutex.lock().unwrap().push((spec_idx, res));
                });
            }
        });
    });
    let results = results_mutex.into_inner().unwrap();

    if cancel.load(Ordering::SeqCst) {
        return Ok(vec![]);
    }

    // Assemble, keeping only each group's core range. Mirror the drift checks.
    let mut by_idx: std::collections::HashMap<usize, GroupResult> = std::collections::HashMap::new();
    for (spec_idx, res) in results {
        match res {
            Ok(gr) => {
                by_idx.insert(spec_idx, gr);
            }
            Err(e) => {
                if e == "cancelled" {
                    return Ok(vec![]);
                }
                return Err(format!("Group {} failed: {}", spec_idx + 1, e));
            }
        }
    }

    let mut translated: std::collections::HashMap<u32, Subtitle> = std::collections::HashMap::new();
    for (spec_idx, g) in groups.iter().enumerate() {
        let gr = match by_idx.get(&spec_idx) {
            Some(v) => v,
            None => continue,
        };
        let group_local = &entries[g.trans_start..g.trans_end];
        let core_rel_start = g.core_start.saturating_sub(g.trans_start);
        let core_rel_end = core_rel_start.max((g.core_end.saturating_sub(g.trans_start)).min(group_local.len()));

        let payload: &[String] = match &gr.kind {
            ParseKind::Ok(v) => v,
            ParseKind::Mapped(v) => v,
        };
        if payload.len() != group_local.len() {
            let first = group_local.first().map(|e| e.index).unwrap_or(0);
            let last = group_local.last().map(|e| e.index).unwrap_or(0);
            return Err(format!(
                "Line count mismatch in translated window {first}-{last}: got {}, expected {}. Aborting batch to avoid subtitle drift.",
                payload.len(),
                group_local.len()
            ));
        }
        for idx_in_group in core_rel_start..core_rel_end {
            let orig = &entries[g.trans_start + idx_in_group];
            let raw = payload.get(idx_in_group).cloned().unwrap_or_default();
            let clean = srt::sanitize_content(&raw);
            translated.insert(
                orig.index,
                Subtitle {
                    index: orig.index,
                    start_ms: orig.start_ms,
                    end_ms: orig.end_ms,
                    content: clean,
                },
            );
        }
    }

    if translated.len() != entries.len() {
        let missing: Vec<u32> = entries
            .iter()
            .filter(|e| !translated.contains_key(&e.index))
            .map(|e| e.index)
            .take(10)
            .collect();
        return Err(format!("Missing translated entries for indices: {:?}", missing));
    }

    // Per-cue cleanup: a few stubborn lines can stay in the wrong language even
    // after whole-window retries (short/ambiguous lines the model renders in,
    // e.g., Chinese inside the window context). Re-translate each such line in
    // isolation — out of the window context the model usually gets it right.
    if !expected_ranges.is_empty() {
        let display_lang = if lang_name.is_empty() { target_lang } else { lang_name };
        let mut fixed = 0usize;
        let mut unresolved: Vec<u32> = Vec::new();
        for orig in entries {
            let suspect = translated
                .get(&orig.index)
                .map(|s| crate::services::cue_needs_retranslate(&s.content, expected_ranges))
                .unwrap_or(false);
            if !suspect {
                continue;
            }
            if cancel.load(Ordering::SeqCst) {
                break;
            }
            progress(Progress::Status(format!(
                "Line {} not in {display_lang} — re-translating it on its own…",
                orig.index
            )));
            match retranslate_cue(translator, orig, display_lang, expected_ranges) {
                Some(better) => {
                    if let Some(slot) = translated.get_mut(&orig.index) {
                        slot.content = better;
                    }
                    fixed += 1;
                    progress(Progress::Status(format!("Line {} fixed → {display_lang}.", orig.index)));
                }
                None => {
                    unresolved.push(orig.index);
                    progress(Progress::Status(format!(
                        "[Warning] line {} still not in {display_lang} after a separate re-translation.",
                        orig.index
                    )));
                }
            }
        }
        if fixed > 0 {
            progress(Progress::Status(format!(
                "Fixed {fixed} stray wrong-language line(s) with a per-line pass."
            )));
        }
        if !unresolved.is_empty() {
            progress(Progress::Status(format!(
                "[Warning] {} line(s) may still not be in the target language: {:?}",
                unresolved.len(),
                unresolved.iter().take(20).collect::<Vec<_>>()
            )));
        }
    }

    let mut ordered: Vec<Subtitle> = translated.into_values().collect();
    ordered.sort_by_key(|e| e.index);
    Ok(srt::sort_and_reindex(ordered))
}

fn emit_fulllog(progress: &&(dyn Fn(Progress) + Sync), group: usize, dbg: &serde_json::Value) {
    let req = serde_json::json!({
        "url": dbg.get("url"),
        "headers": dbg.get("headers"),
        "body": dbg.get("body"),
    });
    if let Ok(req_str) = serde_json::to_string_pretty(&req) {
        progress(Progress::Log(format!("[FullLog] Request (group {group}):\n{req_str}")));
    }
    if let Ok(resp_str) = serde_json::to_string_pretty(dbg.get("response_json").unwrap_or(&serde_json::Value::Null)) {
        progress(Progress::Log(format!(
            "[FullLog] Response (group {group}):\nHTTP {}\n{resp_str}",
            dbg.get("status").and_then(|s| s.as_u64()).unwrap_or(0)
        )));
    }
}

/// Translate a single window, retrying with escalating temperature when the
/// output has the wrong line count or drifts out of the target script. Every
/// retry and the final best-effort outcome are reported via `progress` as they
/// happen. Returns the validated window; if it never validates but a
/// line-count-correct attempt exists, the least-bad one is returned (its stray
/// lines are handled afterwards by the per-line cleanup pass); only a persistent
/// count mismatch or API error hard-fails (returns `Err`, aborting the batch).
fn translate_group(
    entries: &[Subtitle],
    translator: &TranslationService,
    target_lang: &str,
    lang_name: &str,
    expected_ranges: &[(u32, u32)],
    g: &GroupSpec,
    progress: &(dyn Fn(Progress) + Sync),
) -> Result<GroupResult, String> {
    let group_local = &entries[g.trans_start..g.trans_end];
    let display_lang = if lang_name.is_empty() { target_lang } else { lang_name };
    let prompt = translator.build_prompt(group_local, display_lang);
    let first = group_local.first().map(|e| e.index).unwrap_or(0);
    let last = group_local.last().map(|e| e.index).unwrap_or(0);

    const MAX_ATTEMPTS: u32 = 3;
    let mut last_err = String::new();
    // Best line-count-correct-but-wrong-script attempt so far, kept with its
    // script score so we can surface the least-bad one if every retry drifts.
    let mut best_effort: Option<(f32, GroupResult)> = None;

    for attempt in 0..MAX_ATTEMPTS {
        if attempt > 0 {
            // Report every retry immediately, then back off (0.5s, then 1s).
            progress(Progress::Status(format!(
                "[Retry {attempt}/{}] window {first}-{last}: {last_err} — retrying",
                MAX_ATTEMPTS - 1
            )));
            std::thread::sleep(std::time::Duration::from_millis(500u64 << (attempt - 1)));
        }
        // First try uses the configured (low) temperature for a faithful
        // translation. Retries ESCALATE the temperature instead of forcing 0.0:
        // a temp-0 retry is deterministic and would just reproduce the same
        // wrong-language output, so it could never escape a stuck cue. Higher
        // temperature forces a different sampling that can break out of it.
        let temperature = if attempt == 0 {
            translator.settings.temperature
        } else {
            (0.3 + 0.35 * attempt as f32).min(1.0) // attempt 1 → 0.65, attempt 2 → 1.0
        };
        let result = match translator.chat_translate_temp(&prompt, temperature) {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("request failed: {e}");
                continue;
            }
        };
        let kind = parse_payload(&result.content, group_local);
        let payload: &[String] = match &kind {
            ParseKind::Ok(v) => v,
            ParseKind::Mapped(v) => v,
        };
        if payload.len() != group_local.len() {
            last_err = format!(
                "line count mismatch: got {}, expected {}",
                payload.len(),
                group_local.len()
            );
            continue;
        }
        let joined = payload.join("\n");
        if expected_ranges.is_empty() || crate::services::window_script_ok(&joined, expected_ranges) {
            return Ok(GroupResult { kind, debug: result.debug });
        }
        // Line count is right but the script is wrong. Keep this attempt only if
        // it is closer to the target script than what we already have, then retry.
        // Any lines still off after retries are handled by the per-cue cleanup pass.
        last_err = format!("output not in {display_lang}");
        let score = crate::services::script_score(&joined, expected_ranges);
        if best_effort.as_ref().map_or(true, |(best, _)| score > *best) {
            best_effort = Some((score, GroupResult { kind, debug: result.debug }));
        }
    }

    if best_effort.is_some() {
        progress(Progress::Status(format!(
            "[Warning] window {first}-{last}: still not fully in {display_lang} after {MAX_ATTEMPTS} attempts — kept best attempt; per-line cleanup will follow"
        )));
    }
    // Keep the best line-count-correct attempt (wrong script); hard-fail only
    // when no usable payload was ever produced.
    best_effort.map(|(_, gr)| gr).ok_or_else(|| {
        format!("window {first}-{last} failed after {MAX_ATTEMPTS} attempts: {last_err}")
    })
}

/// Re-translate a single cue out of window context, trying two temperatures.
/// Returns a cleaned translation only if it passes the per-cue script check;
/// otherwise `None`, leaving the original best-effort line in place.
fn retranslate_cue(
    translator: &TranslationService,
    orig: &Subtitle,
    display_lang: &str,
    expected_ranges: &[(u32, u32)],
) -> Option<String> {
    let group = [orig.clone()];
    let prompt = translator.build_prompt(&group, display_lang);
    for (attempt, temp) in [0.0f32, 0.9].into_iter().enumerate() {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        let res = match translator.chat_translate_temp(&prompt, temp) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let kind = parse_payload(&res.content, &group);
        let payload: &[String] = match &kind {
            ParseKind::Ok(v) => v,
            ParseKind::Mapped(v) => v,
        };
        if payload.len() != 1 {
            continue;
        }
        let cand = srt::sanitize_content(&payload[0]);
        if !cand.trim().is_empty()
            && !crate::services::cue_needs_retranslate(&cand, expected_ranges)
        {
            return Some(cand);
        }
    }
    None
}

/// Parse model output into per-cue contents via the same ladder as the Python
/// code: SRT → numbered blocks → plain line split (padded/truncated).
fn parse_payload(text: &str, group_local: &[Subtitle]) -> ParseKind {
    // 1) Try full SRT parse.
    let segs = srt::parse(text);
    if !segs.is_empty() {
        let contents: Vec<String> = segs.into_iter().map(|s| s.content).collect();
        return ParseKind::Ok(contents);
    }

    // 2) Numbered blocks ("12:\n...").
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut numbered: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    let mut cur_idx: Option<u32> = None;
    let mut buff: Vec<String> = Vec::new();
    for line in normalized.split('\n') {
        let t = line.trim();
        if t.ends_with(':') && t[..t.len() - 1].chars().all(|c| c.is_ascii_digit()) && !t[..t.len() - 1].is_empty() {
            if let Some(ci) = cur_idx {
                numbered.insert(ci, buff.join("\n"));
            }
            cur_idx = t[..t.len() - 1].parse().ok();
            buff = Vec::new();
        } else {
            buff.push(line.to_string());
        }
    }
    if let Some(ci) = cur_idx {
        numbered.insert(ci, buff.join("\n"));
    }
    if !numbered.is_empty() {
        let mapped: Vec<String> = group_local
            .iter()
            .map(|orig| numbered.get(&orig.index).cloned().unwrap_or_default())
            .collect();
        return ParseKind::Mapped(mapped);
    }

    // 3) Plain line split, padded/truncated to the expected count.
    let mut contents: Vec<String> = normalized
        .split('\n')
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    let expected = group_local.len();
    if contents.len() < expected {
        contents.resize(expected, String::new());
    } else if contents.len() > expected {
        contents.truncate(expected);
    }
    ParseKind::Mapped(contents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Spawn a throwaway HTTP server that answers any `POST /chat/completions`
    /// with a fixed 3-cue SRT. Returns the bound base URL (`http://127.0.0.1:PORT`).
    fn spawn_mock_api() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                // Read the request headers (enough to unblock the client).
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let content = "1\\n00:00:00,000 --> 00:00:01,000\\nT-one\\n\\n2\\n00:00:01,000 --> 00:00:02,000\\nT-two\\n\\n3\\n00:00:02,000 --> 00:00:03,000\\nT-three\\n";
                let body = format!(
                    "{{\"choices\":[{{\"message\":{{\"content\":\"{content}\"}}}}]}}"
                );
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });
        format!("http://127.0.0.1:{}", addr.port())
    }

    #[test]
    fn translate_subs_against_mock_api() {
        let base = spawn_mock_api();
        let mut settings = AppSettings::default();
        settings.api_base = format!("{base}/v1");
        settings.api_key = "test-key".into();
        settings.window = 10; // one group covering all cues
        settings.overlap = 0;
        settings.workers = 2;

        let translator = TranslationService::new(settings.clone());
        let entries = vec![
            Subtitle { index: 1, start_ms: 0, end_ms: 1000, content: "One".into() },
            Subtitle { index: 2, start_ms: 1000, end_ms: 2000, content: "Two".into() },
            Subtitle { index: 3, start_ms: 2000, end_ms: 3000, content: "Three".into() },
        ];
        let cancel = Arc::new(AtomicBool::new(false));
        let out = translate_subs(
            &entries,
            &translator,
            &settings,
            "ru",
            "",
            &[],
            &cancel,
            false,
            &|_p| {},
        )
        .expect("translate_subs");

        assert_eq!(out.len(), 3);
        // Timecodes preserved from the originals; content from the mock.
        assert_eq!(out[0].start_ms, 0);
        assert_eq!(out[0].content, "T-one");
        assert_eq!(out[1].content, "T-two");
        assert_eq!(out[2].content, "T-three");
        // Reindexed 1..=3.
        assert_eq!(out[0].index, 1);
        assert_eq!(out[2].index, 3);
    }

    /// Mock API that returns an English SRT for the first `fail_first` requests,
    /// then a Thai SRT — used to drive the wrong-language retry path.
    fn spawn_mock_lang(fail_first: usize) -> String {
        use std::sync::atomic::{AtomicUsize, Ordering as O};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf);
                let n = count.fetch_add(1, O::SeqCst);
                let srt = if n < fail_first {
                    "1\n00:00:00,000 --> 00:00:01,000\nThis is the wrong language here\n\n2\n00:00:01,000 --> 00:00:02,000\nNothing was translated at all\n\n3\n00:00:02,000 --> 00:00:03,000\nStill completely in English text\n"
                } else {
                    "1\n00:00:00,000 --> 00:00:01,000\nสวัสดีครับทุกคนวันนี้\n\n2\n00:00:01,000 --> 00:00:02,000\nบ้านหลังนี้มีปัญหา\n\n3\n00:00:02,000 --> 00:00:03,000\nกระจกบานนี้แปลกมาก\n"
                };
                let body = serde_json::json!({"choices":[{"message":{"content": srt}}]}).to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });
        format!("http://127.0.0.1:{}", addr.port())
    }

    fn three_cues() -> Vec<Subtitle> {
        vec![
            Subtitle { index: 1, start_ms: 0, end_ms: 1000, content: "One".into() },
            Subtitle { index: 2, start_ms: 1000, end_ms: 2000, content: "Two".into() },
            Subtitle { index: 3, start_ms: 2000, end_ms: 3000, content: "Three".into() },
        ]
    }

    const THAI_RANGES: &[(u32, u32)] = &[(0x0E01, 0x0E5B)];

    fn has_thai(s: &str) -> bool {
        s.chars().any(|c| ('\u{0E01}'..='\u{0E5B}').contains(&c))
    }

    #[test]
    fn retry_recovers_wrong_language() {
        // First response is English, second is Thai: validation must reject the
        // first and the retry must land on the accepted Thai output.
        let base = spawn_mock_lang(1);
        let mut settings = AppSettings::default();
        settings.api_base = format!("{base}/v1");
        settings.api_key = "test-key".into();
        settings.window = 10;
        settings.overlap = 0;
        settings.workers = 1;

        let translator = TranslationService::new(settings.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let out = translate_subs(
            &three_cues(),
            &translator,
            &settings,
            "th",
            "Thai",
            THAI_RANGES,
            &cancel,
            false,
            &|_p| {},
        )
        .expect("translate_subs");

        assert_eq!(out.len(), 3);
        assert!(has_thai(&out[0].content), "expected Thai, got {:?}", out[0].content);
        assert!(!out[0].content.contains("English"));
    }

    #[test]
    fn keep_best_and_warn_when_stuck() {
        // Every response is English: after retries the best attempt is kept and
        // a [Warning] is emitted rather than aborting or silently accepting.
        let base = spawn_mock_lang(100);
        let mut settings = AppSettings::default();
        settings.api_base = format!("{base}/v1");
        settings.api_key = "test-key".into();
        settings.window = 10;
        settings.overlap = 0;
        settings.workers = 1;

        let translator = TranslationService::new(settings.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let warned = Arc::new(AtomicBool::new(false));
        let w = warned.clone();
        let out = translate_subs(
            &three_cues(),
            &translator,
            &settings,
            "th",
            "Thai",
            THAI_RANGES,
            &cancel,
            false,
            &|p| {
                if let Progress::Status(s) = p {
                    if s.contains("[Warning]") {
                        w.store(true, Ordering::SeqCst);
                    }
                }
            },
        )
        .expect("translate_subs keeps best effort");

        assert_eq!(out.len(), 3);
        assert!(out[0].content.contains("wrong language"), "kept English best attempt");
        assert!(warned.load(Ordering::SeqCst), "expected a [Warning] to be emitted");
    }

    #[test]
    fn empty_input_returns_empty() {
        let settings = AppSettings::default();
        let translator = TranslationService::new(settings.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let out =
            translate_subs(&[], &translator, &settings, "ru", "", &[], &cancel, false, &|_p| {})
                .unwrap();
        assert!(out.is_empty());
    }
}
