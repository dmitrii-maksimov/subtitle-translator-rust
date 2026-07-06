//! OpenAI-compatible Chat API client, ported from `services.py`.
//!
//! Uses `reqwest` blocking so it can be driven from the rayon worker pool in
//! `engine.rs` without an async runtime in the hot path.

use crate::settings::AppSettings;
use crate::srt::Subtitle;
use regex::Regex;
use serde_json::{json, Value};
use std::time::Duration;

/// Result of a chat call. `debug` is populated only when `fulllog` is on.
pub struct ChatResult {
    pub content: String,
    pub debug: Option<Value>,
}

pub struct TranslationService {
    pub settings: AppSettings,
    client: reqwest::blocking::Client,
}

impl TranslationService {
    pub fn new(settings: AppSettings) -> Self {
        let client = reqwest::blocking::Client::builder()
            .build()
            .expect("failed to build HTTP client");
        TranslationService { settings, client }
    }

    fn api_base(&self) -> String {
        self.settings.api_base.trim_end_matches('/').to_string()
    }

    /// Build the user prompt for a group of cues (`build_prompt`).
    pub fn build_prompt(&self, group: &[Subtitle], target_lang: &str) -> String {
        let mut lines = Vec::new();
        for e in group {
            lines.push(format!("{}:", e.index));
            let content = e.content.replace("\r\n", "\n").replace('\r', "\n");
            lines.push(content);
        }
        let src_block = lines.join("\n");

        let extra = self.settings.extra_prompt.trim();
        let extra_clause = if !extra.is_empty() {
            format!(
                "\n- IMPORTANT: {} (this instruction is mandatory even if exact translation suffers)",
                extra
            )
        } else {
            String::new()
        };

        let template = if !self.settings.main_prompt_template.is_empty() {
            self.settings.main_prompt_template.clone()
        } else {
            DEFAULT_TEMPLATE.to_string()
        };

        let header = format!("Translate into {target_lang}. Rules:");
        template
            .replace("{header}", &header)
            .replace("{extra}", &extra_clause)
            .replace("{src_block}", &src_block)
    }

    /// Fetch available model ids from `{api_base}/models`, sorted.
    pub fn list_models(&self) -> Result<Vec<String>, String> {
        if self.settings.api_key.is_empty() {
            return Err("API key is not set in settings".to_string());
        }
        let url = format!("{}/models", self.api_base());
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.settings.api_key)
            .timeout(Duration::from_secs(30))
            .send()
            .map_err(|e| format!("request failed: {e}"))?;
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        if status.as_u16() != 200 {
            return Err(format!("API error {}: {}", status.as_u16(), truncate(&text, 300)));
        }
        let data: Value = serde_json::from_str(&text).map_err(|e| format!("bad JSON: {e}"))?;
        let mut ids: Vec<String> = data
            .get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        Ok(ids)
    }

    /// Translate a prompt via `POST {api_base}/chat/completions`.
    pub fn chat_translate(&self, prompt: &str) -> Result<ChatResult, String> {
        let system_role = if self.settings.system_role.is_empty() {
            DEFAULT_SYSTEM_ROLE.to_string()
        } else {
            self.settings.system_role.clone()
        };
        self.chat(&system_role, prompt, 120)
    }

    /// Infer an ISO 639-2 three-letter code from arbitrary language input.
    pub fn chat_infer_iso3(&self, raw_lang: &str) -> Result<ChatResult, String> {
        let system_msg = "You are a language code normalizer. Given any user-provided language/style description, respond with the ISO 639-2 (bibliographic or terminologic) three-letter lowercase code for the primary language. If uncertain, respond with 'und'. Output MUST be only the 3-letter code or 'und', no extra text.";
        let examples = "Examples:\n- 'феня с матами' -> rus\n- 'русский' -> rus\n- 'english sdH' -> eng\n- '日本語' -> jpn\n- 'китайский мандарин' -> zho\n";
        let user_msg = format!("Language field: {raw_lang:?}. Return only ISO 639-2 code. {examples}");
        let mut res = self.chat(system_msg, &user_msg, 60)?;
        let mut content = res.content.trim().to_lowercase();
        if content != "und" && !(content.len() == 3 && content.chars().all(|c| c.is_ascii_alphabetic())) {
            let re = Regex::new(r"\b([a-z]{3})\b").unwrap();
            content = re
                .captures(&content)
                .map(|c| c[1].to_string())
                .unwrap_or_else(|| "und".to_string());
        }
        res.content = content;
        Ok(res)
    }

    /// Convert a language/style description into a short English MKV-tag phrase.
    pub fn chat_normalize_lang(&self, raw_lang: &str) -> Result<ChatResult, String> {
        let system_msg = "You convert any user-provided language/style description into a short English phrase suitable for an MKV metadata language tag. Rules: respond in lowercase ascii only, max 30 characters, use only letters, numbers and spaces, no punctuation, no quotes, no code blocks. Return ONLY the phrase.";
        let examples = "Examples:\n- 'феня с матами' -> fen bad words\n- 'русский' -> russian\n- '日本語' -> japanese\n- 'русский без мата' -> russian no profanity\n";
        let user_msg = format!("Given a 'language' field: {raw_lang:?}. {examples}\nReturn only the English phrase.");
        self.chat(system_msg, &user_msg, 60)
    }

    /// Shared chat-completions call.
    fn chat(&self, system_msg: &str, user_msg: &str, timeout_secs: u64) -> Result<ChatResult, String> {
        if self.settings.api_key.is_empty() {
            return Err("API key is not set in settings".to_string());
        }
        let url = format!("{}/chat/completions", self.api_base());
        let body = json!({
            "model": self.settings.model,
            "messages": [
                {"role": "system", "content": system_msg},
                {"role": "user", "content": user_msg},
            ],
            "temperature": 1,
        });
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.settings.api_key)
            .header("Content-Type", "application/json")
            .timeout(Duration::from_secs(timeout_secs))
            .json(&body)
            .send()
            .map_err(|e| format!("request failed: {e}"))?;
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        if status.as_u16() != 200 {
            return Err(format!("API error {}: {}", status.as_u16(), truncate(&text, 300)));
        }
        let data: Value = serde_json::from_str(&text).map_err(|_| "Unexpected API response format".to_string())?;
        let content = data
            .pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .ok_or_else(|| "Unexpected API response format".to_string())?
            .trim()
            .to_string();

        let debug = if self.settings.fulllog {
            Some(json!({
                "url": url,
                "headers": {"Authorization": "***", "Content-Type": "application/json"},
                "body": body,
                "status": status.as_u16(),
                "response_json": data,
            }))
        } else {
            None
        };
        Ok(ChatResult { content, debug })
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub const DEFAULT_SYSTEM_ROLE: &str = "You translate subtitles. Output must be ONLY the translated lines, one per input line, without indices, timestamps, or any additional labels.";

pub const DEFAULT_TEMPLATE: &str = "{header}\n- Keep numbering (e.g., 12:, 43:, ...)\n- Do not change the number of lines or merge/split cues\n- Preserve line breaks within each numbered block exactly as in the input\n- Return ONLY the translated text blocks with the same numbering, no timestamps, no extra comments{extra}\n\n- New subtitles don't have to contain any characters in original language\nExample:\n1:\nHello!\n42:\nHow are you?\n\nText:\n{src_block}";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::srt::Subtitle;

    #[test]
    fn build_prompt_numbers_cues() {
        let svc = TranslationService::new(AppSettings::default());
        let group = vec![
            Subtitle { index: 1, start_ms: 0, end_ms: 1000, content: "Hello!".into() },
            Subtitle { index: 42, start_ms: 2000, end_ms: 3000, content: "How are you?".into() },
        ];
        let prompt = svc.build_prompt(&group, "ru");
        assert!(prompt.contains("Translate into ru. Rules:"));
        assert!(prompt.contains("1:\nHello!"));
        assert!(prompt.contains("42:\nHow are you?"));
    }

    #[test]
    fn build_prompt_injects_extra() {
        let mut s = AppSettings::default();
        s.extra_prompt = "keep it formal".into();
        let svc = TranslationService::new(s);
        let group = vec![Subtitle { index: 1, start_ms: 0, end_ms: 1, content: "Hi".into() }];
        let prompt = svc.build_prompt(&group, "ru");
        assert!(prompt.contains("IMPORTANT: keep it formal"));
    }
}
