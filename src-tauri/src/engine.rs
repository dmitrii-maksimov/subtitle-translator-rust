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
pub fn translate_subs(
    entries: &[Subtitle],
    translator: &TranslationService,
    settings: &AppSettings,
    target_lang: &str,
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
    let progress_ref = &progress;

    // Run all groups; collect (spec_idx, Result).
    let results: Vec<(usize, Result<GroupResult, String>)> = pool.install(|| {
        use rayon::prelude::*;
        groups
            .par_iter()
            .enumerate()
            .map(|(spec_idx, g)| {
                if cancel.load(Ordering::SeqCst) {
                    return (spec_idx, Err("cancelled".to_string()));
                }
                let res = translate_group(entries, translator, target_lang, g);
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
                (spec_idx, res)
            })
            .collect()
    });

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

/// Translate a single window and parse the model output via the same ladder as
/// the Python code: SRT → numbered blocks → plain line split (padded/truncated).
fn translate_group(
    entries: &[Subtitle],
    translator: &TranslationService,
    target_lang: &str,
    g: &GroupSpec,
) -> Result<GroupResult, String> {
    let group_local = &entries[g.trans_start..g.trans_end];
    let prompt = translator.build_prompt(group_local, target_lang);
    let result = translator.chat_translate(&prompt)?;
    let text = result.content;
    let debug = result.debug;

    // 1) Try full SRT parse.
    let segs = srt::parse(&text);
    if !segs.is_empty() {
        let contents: Vec<String> = segs.into_iter().map(|s| s.content).collect();
        return Ok(GroupResult {
            kind: ParseKind::Ok(contents),
            debug,
        });
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
        return Ok(GroupResult {
            kind: ParseKind::Mapped(mapped),
            debug,
        });
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
    Ok(GroupResult {
        kind: ParseKind::Mapped(contents),
        debug,
    })
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

    #[test]
    fn empty_input_returns_empty() {
        let settings = AppSettings::default();
        let translator = TranslationService::new(settings.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let out = translate_subs(&[], &translator, &settings, "ru", &cancel, false, &|_p| {}).unwrap();
        assert!(out.is_empty());
    }
}
