//! Application settings — a serde mirror of Python's `AppSettings` dataclass
//! (`models.py`). Persisted as pretty JSON at
//! `~/.subtitle_translator_settings.json`, the same path/format the Python app
//! uses, so an existing config keeps working.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_api_base() -> String {
    "https://api.openai.com/v1".to_string()
}
fn default_model() -> String {
    "gpt-4o-mini".to_string()
}
fn default_workers() -> u32 {
    5
}
fn default_window() -> u32 {
    25
}
fn default_overlap() -> u32 {
    10
}
fn default_temperature() -> f32 {
    0.2
}
fn default_target_language() -> String {
    "ru".to_string()
}
fn default_overwrite_original() -> bool {
    true
}
fn default_source_lang() -> String {
    "eng".to_string()
}
fn default_source_title() -> String {
    "Full".to_string()
}
fn default_kodi_port() -> u32 {
    8080
}
fn default_kodi_user() -> String {
    "kodi".to_string()
}
fn default_live_poll_interval() -> u32 {
    30
}
fn default_live_stable_threshold() -> u32 {
    30
}
fn default_kodi_follow_buffer_min() -> u32 {
    10
}
fn default_true() -> bool {
    true
}
fn default_theme() -> String {
    "system".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub api_key: String,
    #[serde(default = "default_api_base")]
    pub api_base: String,
    #[serde(default = "default_model")]
    pub model: String,

    #[serde(default = "default_workers")]
    pub workers: u32,
    #[serde(default = "default_window")]
    pub window: u32,
    #[serde(default = "default_overlap")]
    pub overlap: u32,
    /// Sampling temperature for translation requests. Low by default (0.2) to
    /// keep the model from drifting into the wrong language; retries force 0.0.
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    #[serde(default = "default_target_language")]
    pub target_language: String,
    pub last_dir: String,
    pub fulllog: bool,
    pub extra_prompt: String,
    #[serde(default = "default_overwrite_original")]
    pub overwrite_original: bool,
    pub main_prompt_template: String,
    pub system_role: String,
    /// The default template a user with a *custom* prompt has chosen to keep
    /// their own against, so we stop offering that particular new default.
    /// Empty = never declined.
    pub prompt_migration_declined: String,

    #[serde(default = "default_source_lang")]
    pub default_source_lang: String,
    #[serde(default = "default_source_title")]
    pub default_source_title: String,

    pub cached_tag_lang: String,
    pub cached_iso3: String,
    pub cached_source_lang_input: String,
    /// Cached target-language metadata for script validation, as JSON
    /// `{"input":<target_lang>,"name":<English name>,"ranges":[[lo,hi],...]}`.
    /// Reused while `input` matches the current target language.
    pub cached_lang_meta: String,

    pub cached_models: Vec<String>,
    pub use_custom_model: bool,

    // Kodi integration (persisted now; feature ported in a later pass).
    pub kodi_host: String,
    #[serde(default = "default_kodi_port")]
    pub kodi_port: u32,
    #[serde(default = "default_kodi_user")]
    pub kodi_user: String,
    pub kodi_password: String,
    pub kodi_source_path: String,
    pub local_parent_path: String,

    #[serde(default = "default_live_poll_interval")]
    pub live_poll_interval: u32,
    #[serde(default = "default_live_stable_threshold")]
    pub live_stable_threshold: u32,
    #[serde(default = "default_kodi_follow_buffer_min")]
    pub kodi_follow_buffer_min: u32,

    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
    pub last_update_check: f64,
    pub skip_version: String,

    pub show_kodi: bool,

    // UI theme: "system" (follow OS), "light", or "dark".
    #[serde(default = "default_theme")]
    pub theme: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            api_key: String::new(),
            api_base: default_api_base(),
            model: default_model(),
            workers: default_workers(),
            window: default_window(),
            overlap: default_overlap(),
            temperature: default_temperature(),
            target_language: default_target_language(),
            last_dir: String::new(),
            fulllog: false,
            extra_prompt: String::new(),
            overwrite_original: default_overwrite_original(),
            main_prompt_template: String::new(),
            system_role: String::new(),
            prompt_migration_declined: String::new(),
            default_source_lang: default_source_lang(),
            default_source_title: default_source_title(),
            cached_tag_lang: String::new(),
            cached_iso3: String::new(),
            cached_source_lang_input: String::new(),
            cached_lang_meta: String::new(),
            cached_models: Vec::new(),
            use_custom_model: false,
            kodi_host: String::new(),
            kodi_port: default_kodi_port(),
            kodi_user: default_kodi_user(),
            kodi_password: String::new(),
            kodi_source_path: String::new(),
            local_parent_path: String::new(),
            live_poll_interval: default_live_poll_interval(),
            live_stable_threshold: default_live_stable_threshold(),
            kodi_follow_buffer_min: default_kodi_follow_buffer_min(),
            auto_check_updates: true,
            last_update_check: 0.0,
            skip_version: String::new(),
            // Fresh installs hide Kodi; load() flips this on for upgraders whose
            // file predates the key (see settings_path/load).
            show_kodi: false,
            theme: default_theme(),
        }
    }
}

pub fn settings_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".subtitle_translator_settings.json")
}

impl AppSettings {
    /// Load settings, mirroring `AppSettings.load()`:
    /// - missing file → defaults;
    /// - a file lacking `show_kodi` means a pre-1.4.2 upgrader → keep Kodi
    ///   visible (`show_kodi = true`);
    /// - unknown keys are ignored; any parse error falls back to defaults.
    pub fn load() -> AppSettings {
        let path = settings_path();
        let raw = match std::fs::read_to_string(&path) {
            Ok(r) => r,
            Err(_) => return AppSettings::default(),
        };
        let value: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => return AppSettings::default(),
        };
        let had_show_kodi = value
            .as_object()
            .map(|o| o.contains_key("show_kodi"))
            .unwrap_or(false);
        let mut settings: AppSettings =
            serde_json::from_value(value).unwrap_or_default();
        if !had_show_kodi {
            settings.show_kodi = true;
        }
        // Silently move users who never really customized the prompt (empty, or
        // an override equal to some past/current default) onto the live default
        // by clearing the override. Genuinely custom prompts are left for the UI
        // to offer a migration.
        if crate::services::is_known_default_template(&settings.main_prompt_template) {
            settings.main_prompt_template.clear();
        }
        if crate::services::is_known_default_system_role(&settings.system_role) {
            settings.system_role.clear();
        }
        settings
    }

    /// Persist to disk as pretty UTF-8 JSON. Errors are swallowed to match the
    /// Python behavior of never letting a save failure disturb the UI.
    pub fn save(&self) {
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(settings_path(), text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_python() {
        let s = AppSettings::default();
        assert_eq!(s.api_base, "https://api.openai.com/v1");
        assert_eq!(s.model, "gpt-4o-mini");
        assert_eq!(s.workers, 5);
        assert_eq!(s.window, 25);
        assert_eq!(s.overlap, 10);
        assert_eq!(s.temperature, 0.2);
        assert_eq!(s.target_language, "ru");
        assert!(s.overwrite_original);
        assert!(!s.show_kodi);
    }

    #[test]
    fn unknown_keys_ignored_and_roundtrip() {
        let json = r#"{"api_key":"secret","model":"gpt-5","totally_unknown":42,"show_kodi":false}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let s: AppSettings = serde_json::from_value(value).unwrap();
        assert_eq!(s.api_key, "secret");
        assert_eq!(s.model, "gpt-5");
        assert!(!s.show_kodi);
        // Missing keys fall back to defaults.
        assert_eq!(s.workers, 5);
    }

    #[test]
    fn known_default_prompt_is_cleared_but_custom_kept() {
        // A stored override equal to a past default → cleared (tracks live default).
        let legacy = crate::services::LEGACY_DEFAULT_TEMPLATES[0];
        let mut s = AppSettings::default();
        s.main_prompt_template = legacy.to_string();
        if crate::services::is_known_default_template(&s.main_prompt_template) {
            s.main_prompt_template.clear();
        }
        assert!(s.main_prompt_template.is_empty(), "legacy default should be cleared");

        // A genuinely custom prompt is preserved.
        let mut c = AppSettings::default();
        c.main_prompt_template = "MY CUSTOM {src_block}".into();
        if crate::services::is_known_default_template(&c.main_prompt_template) {
            c.main_prompt_template.clear();
        }
        assert_eq!(c.main_prompt_template, "MY CUSTOM {src_block}");
    }

    #[test]
    fn migration_forces_show_kodi_for_upgraders() {
        // Simulate the load() migration branch on a file with no show_kodi key.
        let json = r#"{"api_key":"x"}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let had = value.as_object().unwrap().contains_key("show_kodi");
        assert!(!had);
        let mut s: AppSettings = serde_json::from_value(value).unwrap();
        if !had {
            s.show_kodi = true;
        }
        assert!(s.show_kodi);
    }
}
