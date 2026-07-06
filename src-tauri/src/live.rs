//! Live-download translation loop, ported from `core/live_loop.py`.
//!
//! Polls a still-downloading MKV for growth, re-extracts the chosen subtitle
//! stream leniently, translates newly-available full windows (or the tail once
//! the file is stable), and appends to `<base>.<lang>.translated.srt` with a
//! trailing "please pause" sentinel. Kodi push is handled by the follow mode.

use crate::engine::{self, Progress};
use crate::ffmpeg::extract::extract_srt_lenient;
use crate::services::TranslationService;
use crate::settings::AppSettings;
use crate::srt::{self, Subtitle};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

fn strip_ext(path: &str) -> String {
    let sep = path.rfind(['/', '\\']).map(|p| p + 1).unwrap_or(0);
    match path.rfind('.') {
        Some(pos) if pos > sep => path[..pos].to_string(),
        _ => path.to_string(),
    }
}

fn basename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// Sleep `secs` seconds in 1s steps, returning true if cancellation was requested.
fn sleep_with_cancel(secs: u64, cancel: &Arc<AtomicBool>) -> bool {
    for _ in 0..secs {
        if cancel.load(Ordering::SeqCst) {
            return true;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    false
}

pub fn live_translate_mkv(
    mkv_path: &str,
    stream_index: i64,
    target_lang: &str,
    settings: &AppSettings,
    translator: &TranslationService,
    cancel: &Arc<AtomicBool>,
    emit: &(dyn Fn(Progress) + Sync),
) -> Result<(), String> {
    let poll_interval = (settings.live_poll_interval.max(5)) as u64;
    let stable_threshold = (settings.live_stable_threshold.max(5)) as u64;
    let window = settings.window.max(1) as usize;
    let overlap = settings.overlap as usize;
    let threshold_count = window + overlap;

    let base = strip_ext(mkv_path);
    let out_srt = format!("{base}.{target_lang}.translated.srt");
    let extract_path = format!("{base}.live.stream{stream_index}.srt");
    let out_srt_path = Path::new(&out_srt);

    // Resume from any already-translated lines.
    let mut translated_count: usize = 0;
    if out_srt_path.exists() {
        if let Ok(text) = std::fs::read_to_string(out_srt_path) {
            translated_count = srt::strip_sentinel(srt::parse(&text)).len();
        }
        if translated_count > 0 {
            let _ = srt::write_translated_with_sentinel(out_srt_path, &[]);
        }
    }

    if translated_count > 0 {
        emit(Progress::Status(format!(
            "Live mode: resuming from line {translated_count} (already in {}).",
            basename(&out_srt)
        )));
    } else {
        emit(Progress::Status("Live mode: started.".to_string()));
    }

    let mut last_mtime: Option<SystemTime> = None;
    let mut mtime_stable_for: u64 = 0;
    let mut last_stable_announced = false;

    while !cancel.load(Ordering::SeqCst) {
        let cur_mtime = match std::fs::metadata(mkv_path).and_then(|m| m.modified()) {
            Ok(m) => m,
            Err(e) => {
                emit(Progress::Status(format!("File missing: {e}")));
                return Ok(());
            }
        };

        if last_mtime != Some(cur_mtime) {
            mtime_stable_for = 0;
            last_mtime = Some(cur_mtime);
            last_stable_announced = false;
            emit(Progress::Status("File is still growing…".to_string()));
        } else {
            mtime_stable_for += poll_interval;
        }

        let is_stable = mtime_stable_for >= stable_threshold;
        if is_stable && !last_stable_announced {
            emit(Progress::Status(format!(
                "File unchanged for {mtime_stable_for}s — likely finished downloading. Waiting for new lines to reach threshold."
            )));
            last_stable_announced = true;
        }

        let srt_path = match extract_srt_lenient(mkv_path, stream_index, Some(&extract_path)) {
            Some(p) => p,
            None => {
                emit(Progress::Status("Subtitles not yet available, waiting...".to_string()));
                if sleep_with_cancel(poll_interval, cancel) {
                    return Ok(());
                }
                continue;
            }
        };

        let all_subs = match std::fs::read_to_string(&srt_path) {
            Ok(text) => srt::parse(&text),
            Err(e) => {
                emit(Progress::Status(format!("SRT parse error: {e}")));
                Vec::new()
            }
        };

        let new_subs: Vec<Subtitle> = all_subs.iter().skip(translated_count).cloned().collect();
        let total_input = all_subs.len().max(1);

        if translated_count == 0 && new_subs.len() < threshold_count {
            emit(Progress::Status(format!(
                "Accumulated {}/{} new lines, waiting...",
                new_subs.len(),
                threshold_count
            )));
            if sleep_with_cancel(poll_interval, cancel) {
                return Ok(());
            }
            continue;
        }

        let to_translate_count = if is_stable {
            new_subs.len()
        } else {
            if new_subs.len() < window {
                emit(Progress::Status(format!(
                    "Waiting for {} more lines to fill the next full window (have {})...",
                    window - new_subs.len(),
                    new_subs.len()
                )));
                if sleep_with_cancel(poll_interval, cancel) {
                    return Ok(());
                }
                continue;
            }
            (new_subs.len() / window) * window
        };

        if to_translate_count == 0 {
            emit(Progress::Status("Nothing to translate, waiting...".to_string()));
            if sleep_with_cancel(poll_interval, cancel) {
                return Ok(());
            }
            continue;
        }

        let batch: Vec<Subtitle> = new_subs[..to_translate_count].to_vec();
        emit(Progress::Status(format!("Translating {} new lines...", batch.len())));

        let translated_chunk = match engine::translate_subs(
            &batch,
            translator,
            settings,
            target_lang,
            cancel,
            settings.fulllog,
            emit,
        ) {
            Ok(v) => v,
            Err(e) => {
                emit(Progress::Status(format!("Translate failed for batch: {e}")));
                if sleep_with_cancel(poll_interval, cancel) {
                    return Ok(());
                }
                continue;
            }
        };

        if translated_chunk.is_empty() {
            if sleep_with_cancel(poll_interval, cancel) {
                return Ok(());
            }
            continue;
        }

        // Append with fresh sentinel (indices are reassigned by the writer).
        let new_entries: Vec<Subtitle> = translated_chunk
            .iter()
            .map(|s| Subtitle {
                index: 0,
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                content: s.content.clone(),
            })
            .collect();
        if let Err(e) = srt::write_translated_with_sentinel(out_srt_path, &new_entries) {
            emit(Progress::Status(format!("Failed to write SRT: {e}")));
        }

        translated_count += translated_chunk.len();
        let pct = ((100 * translated_count) / total_input).min(100) as u8;
        emit(Progress::Percent(pct));
        emit(Progress::Status(format!(
            "Wrote {translated_count} lines → {}",
            basename(&out_srt)
        )));

        if sleep_with_cancel(poll_interval, cancel) {
            return Ok(());
        }
    }

    emit(Progress::Status("Live mode: finished.".to_string()));
    Ok(())
}
