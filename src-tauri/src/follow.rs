//! Kodi-follow translation loop, ported from `core/kodi_follow.py`.
//!
//! Watches the active Kodi player; for a movie lacking the target-language
//! subtitle, keeps a translated SRT roughly `kodi_follow_buffer_min` minutes
//! ahead of playback and pushes it to Kodi after each append.

use crate::engine::{self, Progress};
use crate::ffmpeg::extract::extract_srt_lenient;
use crate::ffmpeg::probe::ffprobe_subs_partial;
use crate::kodi_client::{map_kodi_to_local, map_local_to_kodi, KodiClient};
use crate::services::TranslationService;
use crate::settings::AppSettings;
use crate::srt::{self, Subtitle};
use crate::track_matcher::pick_source_subtitle_stream;
use serde_json::Value;
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

fn kodi_time_to_sec(time: &Value) -> i64 {
    let g = |k: &str| time.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
    g("hours") * 3600 + g("minutes") * 60 + g("seconds")
}

fn sleep_with_cancel(secs: u64, cancel: &Arc<AtomicBool>) -> bool {
    for _ in 0..secs {
        if cancel.load(Ordering::SeqCst) {
            return true;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    false
}

/// Per-file tracking state.
struct FollowState {
    skip: bool,
    local: String,
    stream_index: i64,
    out_srt: String,
    extract_path: String,
    kodi_sub_path: Option<String>,
    last_mtime: Option<SystemTime>,
    mtime_stable_for: u64,
    translated_count: usize,
}

pub fn kodi_follow_translate(
    settings: &AppSettings,
    translator: &TranslationService,
    kodi: &KodiClient,
    target_lang: &str,
    cancel: &Arc<AtomicBool>,
    emit: &(dyn Fn(Progress) + Sync),
) -> Result<(), String> {
    let poll_interval = settings.live_poll_interval.max(5) as u64;
    let stable_threshold = settings.live_stable_threshold.max(5) as u64;
    let window = settings.window.max(1) as usize;
    let overlap = settings.overlap as usize;
    let threshold_count = window + overlap;
    let buffer_sec = (settings.kodi_follow_buffer_min.max(1) as i64) * 60;

    let mut last_kodi_file: Option<String> = None;
    let mut state: Option<FollowState> = None;

    emit(Progress::Status("Following Kodi: started.".to_string()));

    // Resolve target-language metadata once (prompt name + script ranges).
    let lang_meta = crate::orchestrate::resolve_lang_meta(translator, settings, target_lang).0;

    while !cancel.load(Ordering::SeqCst) {
        let progress = match kodi.get_player_progress() {
            Err(e) => {
                emit(Progress::Status(format!("Kodi connection error: {e}")));
                if sleep_with_cancel(poll_interval, cancel) {
                    return Ok(());
                }
                continue;
            }
            Ok(None) => {
                emit(Progress::Status("Kodi: no active video player. Waiting...".to_string()));
                last_kodi_file = None;
                state = None;
                if sleep_with_cancel(poll_interval, cancel) {
                    return Ok(());
                }
                continue;
            }
            Ok(Some(p)) => p,
        };

        let kodi_file = progress
            .get("_item")
            .and_then(|i| i.get("file"))
            .and_then(|f| f.as_str())
            .unwrap_or("")
            .to_string();
        if kodi_file.is_empty() {
            emit(Progress::Status("Kodi: playing item has no file path. Waiting...".to_string()));
            if sleep_with_cancel(poll_interval, cancel) {
                return Ok(());
            }
            continue;
        }

        if Some(&kodi_file) != last_kodi_file.as_ref() {
            last_kodi_file = Some(kodi_file.clone());
            state = None;
            emit(Progress::Status(format!("Kodi now playing: {}", basename(&kodi_file))));
        }

        // Establish per-file state on first sight.
        if state.is_none() {
            match establish_state(settings, translator, kodi, target_lang, &kodi_file, cancel, emit) {
                StateResult::Ready(st) => state = Some(st),
                StateResult::Retry => {
                    if sleep_with_cancel(poll_interval, cancel) {
                        return Ok(());
                    }
                    continue;
                }
            }
        }

        let st = state.as_mut().unwrap();
        if st.skip {
            if sleep_with_cancel(poll_interval, cancel) {
                return Ok(());
            }
            continue;
        }

        let cur_mtime = match std::fs::metadata(&st.local).and_then(|m| m.modified()) {
            Ok(m) => m,
            Err(e) => {
                emit(Progress::Status(format!("Local file disappeared: {e}")));
                state = None;
                if sleep_with_cancel(poll_interval, cancel) {
                    return Ok(());
                }
                continue;
            }
        };
        if st.last_mtime != Some(cur_mtime) {
            st.mtime_stable_for = 0;
            st.last_mtime = Some(cur_mtime);
        } else {
            st.mtime_stable_for += poll_interval;
        }
        let is_stable = st.mtime_stable_for >= stable_threshold;

        let srt_path = match extract_srt_lenient(&st.local, st.stream_index, Some(&st.extract_path)) {
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
            Ok(t) => srt::parse(&t),
            Err(e) => {
                emit(Progress::Status(format!("SRT parse error: {e}")));
                Vec::new()
            }
        };
        let new_subs: Vec<Subtitle> = all_subs.iter().skip(st.translated_count).cloned().collect();

        if st.translated_count == 0 && new_subs.len() < threshold_count {
            emit(Progress::Status(format!(
                "Accumulated {}/{} lines, waiting...",
                new_subs.len(),
                threshold_count
            )));
            if sleep_with_cancel(poll_interval, cancel) {
                return Ok(());
            }
            continue;
        }

        // Buffer check: are we already far enough ahead of playback?
        let cur_pos_sec = kodi_time_to_sec(progress.get("time").unwrap_or(&Value::Null));
        let already_until_sec = if st.translated_count > 0 {
            (all_subs[st.translated_count - 1].end_ms / 1000) as i64
        } else {
            0
        };
        let ahead_sec = already_until_sec - cur_pos_sec;
        if ahead_sec >= buffer_sec {
            emit(Progress::Status(format!(
                "Buffer ok: translated {} min ahead of playback. Waiting...",
                ahead_sec / 60
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
                    "Need {} more lines for a full window, waiting...",
                    window - new_subs.len()
                )));
                if sleep_with_cancel(poll_interval, cancel) {
                    return Ok(());
                }
                continue;
            }
            (new_subs.len() / window) * window
        };
        if to_translate_count == 0 {
            if sleep_with_cancel(poll_interval, cancel) {
                return Ok(());
            }
            continue;
        }

        let batch: Vec<Subtitle> = new_subs[..to_translate_count].to_vec();
        emit(Progress::Status(format!(
            "Translating {} lines (playback {}:{:02}, buffered {} min ahead)...",
            batch.len(),
            cur_pos_sec / 60,
            cur_pos_sec % 60,
            ahead_sec / 60
        )));

        let translated_chunk = match engine::translate_subs(
            &batch,
            translator,
            settings,
            target_lang,
            &lang_meta.name,
            &lang_meta.ranges,
            cancel,
            settings.fulllog,
            emit,
        ) {
            Ok(v) => v,
            Err(e) => {
                emit(Progress::Status(format!("Translate failed: {e}")));
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

        let new_entries: Vec<Subtitle> = translated_chunk
            .iter()
            .map(|s| Subtitle { index: 0, start_ms: s.start_ms, end_ms: s.end_ms, content: s.content.clone() })
            .collect();
        let _ = srt::write_translated_with_sentinel(Path::new(&st.out_srt), &new_entries);

        st.translated_count += translated_chunk.len();
        let total_input = all_subs.len().max(1);
        let pct = ((100 * st.translated_count) / total_input).min(100) as u8;
        emit(Progress::Percent(pct));
        emit(Progress::Status(format!(
            "Wrote {} lines → {}",
            st.translated_count,
            basename(&st.out_srt)
        )));

        if let Some(kpath) = st.kodi_sub_path.clone() {
            emit(Progress::Status(format!(
                "Pushing subtitles to Kodi ({} lines)...",
                st.translated_count
            )));
            let res = kodi.set_subtitle(&kpath, Some(target_lang), true, |l| emit(Progress::Status(l)));
            match res {
                Ok(_) => {
                    emit(Progress::Status("Pushed subtitles to Kodi and switched ON.".to_string()));
                    let last_end = translated_chunk.last().map(|s| srt::ms_to_hms(s.end_ms)).unwrap_or_else(|| "00:00:00".to_string());
                    kodi.show_notification("Subtitle Translator", &format!("Subtitles translated up to {last_end}"), 4000, "info");
                }
                Err(e) => emit(Progress::Status(format!("Kodi push failed: {e}"))),
            }
        }

        if sleep_with_cancel(poll_interval, cancel) {
            return Ok(());
        }
    }

    emit(Progress::Status("Following Kodi: finished.".to_string()));
    Ok(())
}

enum StateResult {
    Ready(FollowState),
    Retry,
}

#[allow(clippy::too_many_arguments)]
fn establish_state(
    settings: &AppSettings,
    _translator: &TranslationService,
    kodi: &KodiClient,
    target_lang: &str,
    kodi_file: &str,
    _cancel: &Arc<AtomicBool>,
    emit: &(dyn Fn(Progress) + Sync),
) -> StateResult {
    let local_path = match map_kodi_to_local(kodi_file, &settings.kodi_source_path, &settings.local_parent_path) {
        Ok(p) => p,
        Err(e) => {
            emit(Progress::Status(format!("Path mapping failed: {e}")));
            return StateResult::Retry;
        }
    };
    if !Path::new(&local_path).exists() {
        emit(Progress::Status(format!("Local file not found yet: {local_path}")));
        return StateResult::Retry;
    }

    let streams = match ffprobe_subs_partial(&local_path) {
        Ok(s) => s,
        Err(e) => {
            emit(Progress::Status(format!("ffprobe failed: {e}")));
            return StateResult::Retry;
        }
    };

    // Embedded target-language subtitle already present → just switch Kodi to it.
    let target_l = target_lang.to_lowercase();
    let target_l = target_l.trim();
    let embedded = streams.iter().find(|s| {
        let lang = s.language().to_lowercase();
        let lang = lang.trim();
        !lang.is_empty() && (lang.starts_with(target_l) || target_l.starts_with(lang))
    });
    if let Some(s) = embedded {
        emit(Progress::Status(format!(
            "Target language ({target_lang}) found embedded in mkv (stream #{}). Skipping translation for this file.",
            s.index
        )));
        if kodi.enable_subtitle_by_lang(target_lang, |l| emit(Progress::Status(l))) {
            emit(Progress::Status("Switched Kodi to existing target-language subtitle.".to_string()));
        }
        return StateResult::Ready(FollowState {
            skip: true,
            local: local_path,
            stream_index: 0,
            out_srt: String::new(),
            extract_path: String::new(),
            kodi_sub_path: None,
            last_mtime: None,
            mtime_stable_for: 0,
            translated_count: 0,
        });
    }

    if streams.is_empty() {
        emit(Progress::Status("No subtitle streams visible yet (file may still be downloading).".to_string()));
        return StateResult::Retry;
    }

    let stream_index = match pick_source_subtitle_stream(&streams, target_lang) {
        Some(i) => i,
        None => {
            emit(Progress::Status("No usable source subtitle (only target / ASS streams).".to_string()));
            return StateResult::Retry;
        }
    };

    let base = strip_ext(&local_path);
    let out_srt = format!("{base}.{target_lang}.translated.srt");
    let extract_path = format!("{base}.live.stream{stream_index}.srt");

    let mut translated_count = 0usize;
    let mut existing_last_end = "00:00:00".to_string();
    if Path::new(&out_srt).exists() {
        if let Ok(text) = std::fs::read_to_string(&out_srt) {
            let existing = srt::strip_sentinel(srt::parse(&text));
            translated_count = existing.len();
            if let Some(last) = existing.last() {
                existing_last_end = srt::ms_to_hms(last.end_ms);
            }
        }
        if translated_count > 0 {
            let _ = srt::write_translated_with_sentinel(Path::new(&out_srt), &[]);
        }
    }

    let kodi_sub_path = map_local_to_kodi(&out_srt, &settings.local_parent_path, &settings.kodi_source_path).ok();

    emit(Progress::Status(format!(
        "Tracking {} stream #{stream_index} (resuming from {translated_count} lines)",
        basename(&local_path)
    )));

    // Push any already-translated subtitles to Kodi immediately.
    if translated_count > 0 {
        if let Some(kpath) = &kodi_sub_path {
            if Path::new(&out_srt).exists() {
                emit(Progress::Status(format!("Pushing existing subtitles to Kodi ({translated_count} lines)...")));
                match kodi.set_subtitle(kpath, Some(target_lang), true, |l| emit(Progress::Status(l))) {
                    Ok(_) => {
                        emit(Progress::Status("Pushed subtitles to Kodi and switched ON.".to_string()));
                        kodi.show_notification("Subtitle Translator", &format!("Subtitles translated up to {existing_last_end}"), 4000, "info");
                    }
                    Err(e) => emit(Progress::Status(format!("Kodi push failed: {e}"))),
                }
            }
        }
    }

    StateResult::Ready(FollowState {
        skip: false,
        local: local_path,
        stream_index,
        out_srt,
        extract_path,
        kodi_sub_path,
        last_mtime: None,
        mtime_stable_for: 0,
        translated_count,
    })
}
