//! Subtitle Translator (Rust/Tauri edition) — library entry point.

mod commands;
mod engine;
pub mod ffmpeg;
// Ported Kodi client; follow-mode methods wire in a later pass.
#[allow(dead_code)]
mod kodi_client;
mod live;
mod follow;
mod orchestrate;
mod pricing;
mod services;
mod settings;
pub mod srt;
mod tools;
mod track_matcher;

use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init());

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    builder
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::load_settings,
            commands::save_settings,
            commands::app_version,
            commands::default_prompts,
            commands::check_ffmpeg,
            commands::install_ffmpeg,
            commands::price_for,
            commands::list_models,
            commands::models_info,
            commands::probe_subs,
            commands::list_mkvs,
            commands::pick_source_stream,
            commands::cancel_job,
            commands::translate_srt_file,
            commands::run_batch,
            commands::kodi_ping,
            commands::kodi_discover,
            commands::kodi_browse,
            commands::kodi_map_preview,
            commands::probe_subs_partial,
            commands::start_live,
            commands::start_kodi_follow,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
