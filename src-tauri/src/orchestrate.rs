//! End-to-end single-file flow: read SRT → translate → write SRT → (optionally)
//! remux into the MKV. Ported from `ui/main_window.py::_translate_and_remux`
//! plus the iso3 / language-tag inference helpers.

use crate::engine::{self, Progress};
use crate::ffmpeg::probe::ffprobe_subs;
use crate::ffmpeg::remux::remux_with_translated_srt;
use crate::ffmpeg::Stream;
use crate::services::TranslationService;
use crate::settings::AppSettings;
use crate::srt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Outcome of a single-file translation.
pub struct FileOutcome {
    pub output_path: String,
    /// Updated language-tag cache fields to persist back into settings.
    pub cached_source_lang_input: String,
    pub cached_tag_lang: String,
    pub cached_iso3: String,
    /// Updated target-language metadata cache (see `AppSettings::cached_lang_meta`).
    pub cached_lang_meta: String,
}

#[allow(clippy::too_many_arguments)]
pub fn translate_and_remux(
    src_srt: &str,
    target_lang: &str,
    mkv_path: Option<&str>,
    delete_indexes: &[i64],
    source_stream_index: Option<i64>,
    streams: &[Stream],
    is_standalone_srt: bool,
    settings: &AppSettings,
    translator: &TranslationService,
    cancel: &Arc<AtomicBool>,
    emit: &(dyn Fn(Progress) + Sync),
) -> Result<FileOutcome, String> {
    if cancel.load(Ordering::SeqCst) {
        return Err("Cancelled.".to_string());
    }
    emit(Progress::Status("Reading SRT...".to_string()));
    let srt_text = std::fs::read_to_string(src_srt)
        .map_err(|e| format!("failed to read SRT: {e}"))?;
    let entries = srt::parse(&srt_text);
    if entries.is_empty() {
        return Err("No subtitles parsed from SRT".to_string());
    }

    // Resolve the target language (human name for the prompt + Unicode script
    // ranges for per-window validation) before translating.
    let (lang_meta, lang_meta_cache) = resolve_lang_meta(translator, settings, target_lang);
    if !lang_meta.name.is_empty() {
        emit(Progress::Status(format!(
            "Target language: {} ({} script range(s) for validation).",
            lang_meta.name,
            lang_meta.ranges.len()
        )));
    }

    let ordered = engine::translate_subs(
        &entries,
        translator,
        settings,
        target_lang,
        &lang_meta.name,
        &lang_meta.ranges,
        cancel,
        settings.fulllog,
        emit,
    )?;

    if cancel.load(Ordering::SeqCst) {
        return Err("Cancelled before writing SRT.".to_string());
    }
    emit(Progress::Status("Building translated SRT...".to_string()));

    // Validate round-trip like the Python code.
    let composed = srt::compose(&ordered);
    let reparsed = srt::parse(&composed);
    if reparsed.len() != ordered.len() {
        return Err(format!(
            "Generated SRT is invalid: expected {} entries, got {}",
            ordered.len(),
            reparsed.len()
        ));
    }

    // Output SRT path.
    let base = strip_ext(src_srt);
    let out_srt = if is_standalone_srt {
        let model = settings.model.trim();
        let window_sz = settings.window.max(1);
        let safe_model: String = model
            .chars()
            .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
            .collect();
        let safe_model = if safe_model.is_empty() { "model".to_string() } else { safe_model };
        format!("{base}.{target_lang}.translated.{safe_model}.w{window_sz}.srt")
    } else {
        format!("{base}.{target_lang}.translated.srt")
    };
    srt::write_srt_crlf(Path::new(&out_srt), &ordered)
        .map_err(|e| format!("failed to write SRT: {e}"))?;

    // Standalone SRT (or non-MKV container) → done, no remux.
    let is_mkv = mkv_path
        .map(|p| Path::new(p).extension().map(|e| e.eq_ignore_ascii_case("mkv")).unwrap_or(false))
        .unwrap_or(false);
    if is_standalone_srt || !is_mkv {
        emit(Progress::Percent(100));
        emit(Progress::Status(format!("Done. Output: {out_srt}")));
        return Ok(FileOutcome {
            output_path: out_srt.clone(),
            cached_source_lang_input: settings.cached_source_lang_input.clone(),
            cached_tag_lang: settings.cached_tag_lang.clone(),
            cached_iso3: settings.cached_iso3.clone(),
            cached_lang_meta: lang_meta_cache.clone(),
        });
    }

    if cancel.load(Ordering::SeqCst) {
        return Err("Cancelled before remux.".to_string());
    }
    let remux_src = mkv_path.ok_or("No MKV path provided for remux.")?;

    // Warn on low disk space — remuxing writes a full copy of the video.
    if let Ok(meta) = std::fs::metadata(remux_src) {
        let needed = meta.len() + 100 * 1024 * 1024; // file size + 100 MB margin
        let out_dir = Path::new(remux_src).parent().unwrap_or_else(|| Path::new("."));
        if let Ok(free) = fs2::available_space(out_dir) {
            if free < needed {
                emit(Progress::Status(format!(
                    "[Warning] Low disk space: {:.2} GB free, ~{:.2} GB needed. Remux may fail with 'No space left on device'.",
                    free as f64 / 1e9,
                    needed as f64 / 1e9
                )));
            }
        }
    }

    let overwrite = settings.overwrite_original;
    let out_mkv = if overwrite {
        emit(Progress::Status(
            "Remuxing and overwriting the original MKV with translated subtitles...".to_string(),
        ));
        format!("{}.__tmp_translated__.mkv", strip_ext(remux_src))
    } else {
        emit(Progress::Status(
            "Remuxing new MKV with translated subtitles...".to_string(),
        ));
        format!("{}.translated.mkv", strip_ext(remux_src))
    };

    // Source track title, if we know which stream was translated.
    let mut src_title: Option<String> = None;
    if let Some(idx) = source_stream_index {
        for st in streams {
            if st.index == idx {
                let t = st.title();
                if !t.is_empty() {
                    src_title = Some(t.to_string());
                }
                break;
            }
        }
    }

    // Language tag + iso3, with settings cache.
    let (tag_lang, iso3, new_cache) = if settings.cached_source_lang_input == target_lang
        && !settings.cached_tag_lang.is_empty()
        && !settings.cached_iso3.is_empty()
    {
        (settings.cached_tag_lang.clone(), settings.cached_iso3.clone(), None)
    } else {
        let tag_lang = infer_lang_for_tag(translator, target_lang);
        emit(Progress::Status(format!(
            "Normalized language for MKV tag: '{tag_lang}' (from '{target_lang}')"
        )));
        let iso3 = infer_iso3(translator, target_lang);
        emit(Progress::Status(format!(
            "ISO 639-2 code inferred: '{iso3}' (from '{target_lang}')"
        )));
        (
            tag_lang.clone(),
            iso3.clone(),
            Some((target_lang.to_string(), tag_lang, iso3)),
        )
    };

    let iso3 = if iso3.len() != 3 || !iso3.chars().all(|c| c.is_ascii_alphabetic()) {
        "und".to_string()
    } else {
        iso3
    };
    let final_title = match src_title {
        None => format!("Translated [{iso3}] ({tag_lang})"),
        Some(t) => format!("{t} | Translated [{iso3}] ({tag_lang})"),
    };

    let mut log = |line: String| emit(Progress::Status(line));
    let remux_res = remux_with_translated_srt(
        remux_src,
        &out_srt,
        streams,
        delete_indexes,
        &iso3,
        &final_title,
        &out_mkv,
        &mut log,
    );
    if let Err(e) = remux_res {
        let _ = std::fs::remove_file(&out_srt);
        let _ = std::fs::remove_file(src_srt);
        return Err(e);
    }

    // Finalize overwrite.
    let mut final_out = out_mkv.clone();
    if overwrite {
        if Path::new(&out_mkv).exists() {
            match std::fs::rename(&out_mkv, remux_src) {
                Ok(_) => {
                    final_out = remux_src.to_string();
                    emit(Progress::Status(
                        "Original MKV has been overwritten with translated version.".to_string(),
                    ));
                }
                Err(e) => emit(Progress::Status(format!(
                    "[Warning] Could not overwrite original file: {e}. Keeping new file as {out_mkv}"
                ))),
            }
        } else {
            emit(Progress::Status(
                "[Warning] Expected temporary output file not found after remux; cannot overwrite original.".to_string(),
            ));
        }
    }

    // Clean up temp SRTs (only for MKV flow).
    let _ = std::fs::remove_file(&out_srt);
    let _ = std::fs::remove_file(src_srt);

    emit(Progress::Percent(100));
    emit(Progress::Status(format!("Done. Output: {final_out}")));

    let (csli, ctl, ci) = match new_cache {
        Some((a, b, c)) => (a, b, c),
        None => (
            settings.cached_source_lang_input.clone(),
            settings.cached_tag_lang.clone(),
            settings.cached_iso3.clone(),
        ),
    };
    Ok(FileOutcome {
        output_path: final_out,
        cached_source_lang_input: csli,
        cached_tag_lang: ctl,
        cached_iso3: ci,
        cached_lang_meta: lang_meta_cache,
    })
}

/// Resolve target-language metadata (English name + Unicode script ranges) for
/// the prompt header and per-window script validation. Reuses
/// `settings.cached_lang_meta` when it was produced for the same `target_lang`;
/// otherwise asks the model. Returns the metadata plus the JSON cache blob to
/// persist (the previous blob is preserved on a cache hit or inference failure).
pub fn resolve_lang_meta(
    translator: &TranslationService,
    settings: &AppSettings,
    target_lang: &str,
) -> (crate::services::LangMeta, String) {
    if let Some(meta) = crate::services::lang_meta_from_cache(&settings.cached_lang_meta, target_lang) {
        return (meta, settings.cached_lang_meta.clone());
    }
    match translator.chat_infer_lang_meta(target_lang) {
        Ok(meta) => {
            let blob = crate::services::lang_meta_to_cache(target_lang, &meta);
            (meta, blob)
        }
        // Inference failed: skip validation (empty meta) and keep any old cache.
        Err(_) => (
            crate::services::LangMeta::default(),
            settings.cached_lang_meta.clone(),
        ),
    }
}

/// Re-probe an MKV's subtitle streams (used by the batch loop before remux).
pub fn probe_streams(mkv_path: &str) -> Result<Vec<Stream>, String> {
    ffprobe_subs(mkv_path)
}

/// Convert language input to a short English MKV tag phrase; chat first, then
/// ascii cleanup, else "und".
fn infer_lang_for_tag(translator: &TranslationService, raw_lang: &str) -> String {
    let sanitize = |s: &str| -> String {
        let s = s.to_lowercase();
        let s = s.trim();
        let allowed: String = s
            .chars()
            .filter(|c| (c.is_alphanumeric() && c.is_ascii()) || *c == ' ')
            .collect();
        let parts: Vec<&str> = allowed.split(' ').filter(|p| !p.is_empty()).collect();
        let out = parts.join(" ");
        out.chars().take(30).collect()
    };
    if let Ok(res) = translator.chat_normalize_lang(raw_lang) {
        let out = sanitize(&res.content);
        if !out.is_empty() {
            return out;
        }
    }
    let cleaned = sanitize(raw_lang);
    if cleaned.is_empty() { "und".to_string() } else { cleaned }
}

/// Infer ISO 639-2 code via chat; fallback "und".
fn infer_iso3(translator: &TranslationService, raw_lang: &str) -> String {
    if let Ok(res) = translator.chat_infer_iso3(raw_lang) {
        let code = res.content.trim().to_lowercase();
        if code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic()) {
            return code;
        }
    }
    "und".to_string()
}

fn strip_ext(path: &str) -> String {
    let sep = path.rfind(['/', '\\']).map(|p| p + 1).unwrap_or(0);
    match path.rfind('.') {
        Some(pos) if pos > sep => path[..pos].to_string(),
        _ => path.to_string(),
    }
}
