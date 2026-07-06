//! Tauri command layer: thin wrappers over the core modules, plus the
//! progress-event contract consumed by the Svelte frontend.
//!
//! Event names emitted during a job:
//!   - `progress` : number  (0-100, per-file)
//!   - `status`   : string  (human-readable status/log line)
//!   - `batch`    : { value: number, text: string }  (overall batch progress)
//!   - `log`      : string  (full-log debug lines; only when fulllog is on)

use crate::engine::Progress;
use crate::ffmpeg::probe::ffprobe_subs;
use crate::ffmpeg::Stream;
use crate::orchestrate::{self, translate_and_remux};
use crate::pricing;
use crate::services::TranslationService;
use crate::settings::AppSettings;
use crate::tools;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

pub struct AppState {
    pub settings: Mutex<AppSettings>,
    pub cancel: Arc<AtomicBool>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            settings: Mutex::new(AppSettings::load()),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDecision {
    pub file_path: String,
    #[serde(default)]
    pub translate_stream_index: Option<i64>,
    #[serde(default)]
    pub delete_stream_indexes: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub price: Option<String>,
    pub is_chat: bool,
}

// ---- settings ----

#[tauri::command]
pub fn load_settings(state: State<AppState>) -> AppSettings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn save_settings(state: State<AppState>, settings: AppSettings) {
    settings.save();
    *state.settings.lock().unwrap() = settings;
}

// ---- environment / model picker ----

#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[derive(Serialize)]
pub struct DefaultPrompts {
    pub main_prompt_template: String,
    pub system_role: String,
}

/// The built-in default prompt template + system role, for the "reset to
/// default" buttons in Settings (single source of truth in `services`).
#[tauri::command]
pub fn default_prompts() -> DefaultPrompts {
    DefaultPrompts {
        main_prompt_template: crate::services::DEFAULT_TEMPLATE.to_string(),
        system_role: crate::services::DEFAULT_SYSTEM_ROLE.to_string(),
    }
}

#[tauri::command]
pub fn check_ffmpeg() -> bool {
    tools::check_ffmpeg_available()
}

/// Download ffmpeg + ffprobe next to the executable (Windows only, mirrors the
/// Python `install_ffmpeg`). Streams progress via the `progress`/`status`
/// events; returns the install directory on success.
#[tauri::command]
pub async fn install_ffmpeg(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let emit = make_emitter(app);
        do_install_ffmpeg(&emit)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

fn do_install_ffmpeg(emit: &(dyn Fn(Progress) + Sync)) -> Result<String, String> {
    if !cfg!(windows) {
        return Err(if cfg!(target_os = "macos") {
            "Auto-download is not supported on macOS.\nInstall ffmpeg via Homebrew:  brew install ffmpeg".to_string()
        } else {
            "Auto-download is not supported on this platform.\nInstall ffmpeg with your package manager, e.g.:  sudo apt install ffmpeg".to_string()
        });
    }

    use std::io::Read;
    let base = tools::base_dir();
    let zip_path = base.join("ffmpeg.zip");
    let url = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip";

    emit(Progress::Status("Downloading ffmpeg…".to_string()));
    let client = reqwest::blocking::Client::builder()
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let mut resp = client
        .get(url)
        .send()
        .map_err(|e| format!("download failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("download failed: HTTP {}", resp.status().as_u16()));
    }
    let total = resp.content_length().unwrap_or(0);

    let mut file = std::fs::File::create(&zip_path).map_err(|e| format!("create zip: {e}"))?;
    let mut downloaded: u64 = 0;
    let mut buf = [0u8; 32 * 1024];
    loop {
        let n = resp.read(&mut buf).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            break;
        }
        use std::io::Write;
        file.write_all(&buf[..n]).map_err(|e| format!("write zip: {e}"))?;
        downloaded += n as u64;
        if total > 0 {
            // Download is the first ~60% of the work.
            let pct = (downloaded as f64 / total as f64 * 60.0) as u8;
            emit(Progress::Percent(pct));
        }
    }
    drop(file);

    emit(Progress::Status("Extracting ffmpeg…".to_string()));
    emit(Progress::Percent(65));
    let zip_file = std::fs::File::open(&zip_path).map_err(|e| format!("open zip: {e}"))?;
    let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| format!("read zip: {e}"))?;

    let mut extracted = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("zip entry: {e}"))?;
        let name = entry.name().to_string();
        let target = if name.ends_with("ffmpeg.exe") {
            Some("ffmpeg.exe")
        } else if name.ends_with("ffprobe.exe") {
            Some("ffprobe.exe")
        } else {
            None
        };
        if let Some(out_name) = target {
            let out_path = base.join(out_name);
            let mut out = std::fs::File::create(&out_path).map_err(|e| format!("create {out_name}: {e}"))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("extract {out_name}: {e}"))?;
            extracted += 1;
        }
    }
    let _ = std::fs::remove_file(&zip_path);

    if extracted < 2 {
        return Err("Could not find ffmpeg.exe / ffprobe.exe in the downloaded archive".to_string());
    }
    emit(Progress::Percent(100));
    emit(Progress::Status("ffmpeg installed.".to_string()));
    Ok(base.to_string_lossy().to_string())
}

#[tauri::command]
pub fn price_for(model: String) -> Option<String> {
    pricing::format_pricing(&model)
}

/// Map a list of model ids to their pricing/chat info from the static table.
/// Used to restore prices for the cached model list on startup (prices come
/// from the local table, not the API).
#[tauri::command]
pub fn models_info(ids: Vec<String>) -> Vec<ModelInfo> {
    ids.into_iter()
        .map(|id| ModelInfo {
            price: pricing::format_pricing(&id),
            is_chat: pricing::is_text_completion_model(&id),
            id,
        })
        .collect()
}

#[tauri::command]
pub fn list_models(state: State<AppState>) -> Result<Vec<ModelInfo>, String> {
    let settings = state.settings.lock().unwrap().clone();
    let translator = TranslationService::new(settings);
    let ids = translator.list_models()?;
    Ok(ids
        .into_iter()
        .map(|id| ModelInfo {
            price: pricing::format_pricing(&id),
            is_chat: pricing::is_text_completion_model(&id),
            id,
        })
        .collect())
}

// ---- probing ----

#[tauri::command]
pub fn probe_subs(path: String) -> Result<Vec<Stream>, String> {
    ffprobe_subs(&path)
}

#[tauri::command]
pub fn pick_source_stream(streams: Vec<Stream>, target_lang: String) -> Option<i64> {
    crate::track_matcher::pick_source_subtitle_stream(&streams, &target_lang)
}

// ---- cancellation ----

#[tauri::command]
pub fn cancel_job(state: State<AppState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

// ---- jobs ----

/// Build an emit closure that forwards `Progress` values to Tauri events.
fn make_emitter(app: AppHandle) -> impl Fn(Progress) + Sync + Send {
    move |p: Progress| match p {
        Progress::Percent(v) => {
            let _ = app.emit("progress", v);
        }
        Progress::Status(s) => {
            let _ = app.emit("status", s);
        }
        Progress::Log(s) => {
            let _ = app.emit("log", s);
        }
    }
}

/// Translate a standalone `.srt`/`.str` file in place (no remux).
#[tauri::command]
pub async fn translate_srt_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();
    let cancel = state.cancel.clone();
    cancel.store(false, Ordering::SeqCst);

    let target_lang = if settings.target_language.is_empty() {
        "ru".to_string()
    } else {
        settings.target_language.clone()
    };

    let result = tauri::async_runtime::spawn_blocking(move || {
        let emit = make_emitter(app);
        let translator = TranslationService::new(settings.clone());
        translate_and_remux(
            &path,
            &target_lang,
            Some(&path),
            &[],
            None,
            &[],
            true,
            &settings,
            &translator,
            &cancel,
            &emit,
        )
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?;

    result.map(|o| o.output_path)
}

/// Run a batch of MKV files: translate the chosen track and remux, dropping any
/// tracks marked for deletion. Emits per-file and overall progress events.
#[tauri::command]
pub async fn run_batch(
    app: AppHandle,
    state: State<'_, AppState>,
    decisions: Vec<FileDecision>,
) -> Result<String, String> {
    let mut settings = state.settings.lock().unwrap().clone();
    let cancel = state.cancel.clone();
    cancel.store(false, Ordering::SeqCst);

    let target_lang = if settings.target_language.is_empty() {
        "ru".to_string()
    } else {
        settings.target_language.clone()
    };

    let app_for_task = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || -> Result<(String, AppSettings), String> {
        let emit = make_emitter(app_for_task.clone());
        let total = decisions.len().max(1);
        let mut last_output = String::new();

        for (i, dec) in decisions.iter().enumerate() {
            if cancel.load(Ordering::SeqCst) {
                return Err("Batch cancelled.".to_string());
            }
            let _ = app_for_task.emit(
                "batch",
                serde_json::json!({
                    "value": (i as f64 / total as f64 * 100.0) as u32,
                    "text": format!("File {}/{}: {}", i + 1, total, basename(&dec.file_path)),
                }),
            );
            let _ = app_for_task.emit("progress", 0u32);

            let translate_idx = match dec.translate_stream_index {
                Some(idx) => idx,
                None => {
                    // Nothing to translate; if there are deletions, drop-only.
                    // MVP: skip drop-only files with a note (remux-drop is a
                    // later refinement).
                    emit(Progress::Status(format!(
                        "Skipping {} (no translate track selected).",
                        basename(&dec.file_path)
                    )));
                    continue;
                }
            };

            let streams = orchestrate::probe_streams(&dec.file_path)?;
            let ffmpeg = crate::tools::find_tool("ffmpeg")?;
            let src_srt = format!("{}.stream{}.srt", strip_ext(&dec.file_path), translate_idx);
            // Extract the chosen stream to a temp SRT.
            crate::ffmpeg::extract::extract_srt(&dec.file_path, translate_idx, &src_srt)
                .map_err(|e| format!("extract failed: {e}"))?;
            let _ = &ffmpeg; // discovered above to fail fast if ffmpeg is missing

            let translator = TranslationService::new(settings.clone());
            let outcome = translate_and_remux(
                &src_srt,
                &target_lang,
                Some(&dec.file_path),
                &dec.delete_stream_indexes,
                Some(translate_idx),
                &streams,
                false,
                &settings,
                &translator,
                &cancel,
                &emit,
            )?;

            // Persist language-tag cache back into settings for reuse.
            settings.cached_source_lang_input = outcome.cached_source_lang_input;
            settings.cached_tag_lang = outcome.cached_tag_lang;
            settings.cached_iso3 = outcome.cached_iso3;
            last_output = outcome.output_path;
        }

        let _ = app_for_task.emit(
            "batch",
            serde_json::json!({ "value": 100u32, "text": "Batch complete." }),
        );
        Ok((last_output, settings))
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?;

    match outcome {
        Ok((last_output, new_settings)) => {
            new_settings.save();
            *state.settings.lock().unwrap() = new_settings;
            Ok(last_output)
        }
        Err(e) => Err(e),
    }
}

fn basename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

fn strip_ext(path: &str) -> String {
    let sep = path.rfind(['/', '\\']).map(|p| p + 1).unwrap_or(0);
    match path.rfind('.') {
        Some(pos) if pos > sep => path[..pos].to_string(),
        _ => path.to_string(),
    }
}
