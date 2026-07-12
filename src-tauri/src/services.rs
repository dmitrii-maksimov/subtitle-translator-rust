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

    /// Translate a prompt at an explicit temperature (the first attempt uses the
    /// configured temperature; retries pass 0.0 to make the model deterministic
    /// and pull it back to the target language).
    pub fn chat_translate_temp(&self, prompt: &str, temperature: f32) -> Result<ChatResult, String> {
        let system_role = if self.settings.system_role.is_empty() {
            DEFAULT_SYSTEM_ROLE.to_string()
        } else {
            self.settings.system_role.clone()
        };
        self.chat(&system_role, prompt, 120, temperature)
    }

    /// Infer an ISO 639-2 three-letter code from arbitrary language input.
    pub fn chat_infer_iso3(&self, raw_lang: &str) -> Result<ChatResult, String> {
        let system_msg = "You are a language code normalizer. Given any user-provided language/style description, respond with the ISO 639-2 (bibliographic or terminologic) three-letter lowercase code for the primary language. If uncertain, respond with 'und'. Output MUST be only the 3-letter code or 'und', no extra text.";
        let examples = "Examples:\n- 'феня с матами' -> rus\n- 'русский' -> rus\n- 'english sdH' -> eng\n- '日本語' -> jpn\n- 'китайский мандарин' -> zho\n";
        let user_msg = format!("Language field: {raw_lang:?}. Return only ISO 639-2 code. {examples}");
        let mut res = self.chat(system_msg, &user_msg, 60, 0.0)?;
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
        self.chat(system_msg, &user_msg, 60, 0.0)
    }

    /// Infer the canonical English name and the Unicode letter ranges of the
    /// script(s) used to write `target_lang`. Used to (a) name the language in
    /// the translation prompt and (b) validate that each window came back in
    /// the right script. On any failure returns an empty `LangMeta` (name = "",
    /// ranges = []), which makes the caller fall back to the raw language string
    /// and skip script validation.
    pub fn chat_infer_lang_meta(&self, target_lang: &str) -> Result<LangMeta, String> {
        let system_msg = "You are a language metadata resolver. Given a user-provided \
language or style description, respond with ONLY a JSON object of the form \
{\"name\": <canonical English language name>, \"ranges\": [[start, end], ...]} where \
`ranges` are the Unicode code-point ranges (decimal integers, inclusive) of the LETTER \
blocks of the script(s) normally used to write that language. Include letter blocks only \
(no punctuation, digits or symbols). Cover every script the language commonly uses. If you \
cannot determine the language, respond with {\"name\": \"\", \"ranges\": []}. No prose, no \
code fences.";
        let examples = "Examples:\n\
- 'th' -> {\"name\": \"Thai\", \"ranges\": [[3585, 3675]]}\n\
- 'русский' -> {\"name\": \"Russian\", \"ranges\": [[1024, 1327]]}\n\
- 'english sdh' -> {\"name\": \"English\", \"ranges\": [[65, 90], [97, 122]]}\n\
- '日本語' -> {\"name\": \"Japanese\", \"ranges\": [[12352, 12447], [12448, 12543], [19968, 40959]]}\n";
        let user_msg = format!("Language field: {target_lang:?}. Return only the JSON object. {examples}");
        let res = self.chat(system_msg, &user_msg, 60, 0.0)?;
        Ok(parse_lang_meta(&res.content))
    }

    /// Shared chat-completions call.
    fn chat(
        &self,
        system_msg: &str,
        user_msg: &str,
        timeout_secs: u64,
        temperature: f32,
    ) -> Result<ChatResult, String> {
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
            "temperature": temperature,
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

/// Target-language metadata used for the prompt header and script validation.
/// An empty `name` / empty `ranges` means "unknown" — the caller falls back to
/// the raw language string and skips validation.
#[derive(Debug, Clone, Default)]
pub struct LangMeta {
    pub name: String,
    pub ranges: Vec<(u32, u32)>,
}

/// Parse the JSON returned by `chat_infer_lang_meta`. Tolerant of surrounding
/// prose / code fences and of ranges given as numbers or hex/decimal strings.
pub fn parse_lang_meta(raw: &str) -> LangMeta {
    let slice = match (raw.find('{'), raw.rfind('}')) {
        (Some(a), Some(b)) if b > a => &raw[a..=b],
        _ => return LangMeta::default(),
    };
    let val: Value = match serde_json::from_str(slice) {
        Ok(v) => v,
        Err(_) => return LangMeta::default(),
    };
    let name = val
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let mut ranges = Vec::new();
    if let Some(arr) = val.get("ranges").and_then(|r| r.as_array()) {
        for pair in arr {
            let p = match pair.as_array() {
                Some(p) if p.len() == 2 => p,
                _ => continue,
            };
            if let (Some(lo), Some(hi)) = (parse_codepoint(&p[0]), parse_codepoint(&p[1])) {
                if lo <= hi {
                    ranges.push((lo, hi));
                }
            }
        }
    }
    LangMeta { name, ranges }
}

/// Parse a single Unicode code point from a JSON number or a decimal/hex string
/// (`"3585"`, `"0x0E01"`, `"U+0E01"`).
fn parse_codepoint(v: &Value) -> Option<u32> {
    if let Some(n) = v.as_u64() {
        return u32::try_from(n).ok();
    }
    let s = v.as_str()?.trim();
    let s = s.trim_start_matches("U+").trim_start_matches("u+");
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u32::from_str_radix(hex, 16).ok();
    }
    // Bare hex is ambiguous with decimal; prefer decimal, fall back to hex.
    s.parse::<u32>()
        .ok()
        .or_else(|| u32::from_str_radix(s, 16).ok())
}

/// Serialize `LangMeta` (plus its cache key) into the JSON blob stored in
/// `settings.cached_lang_meta`.
pub fn lang_meta_to_cache(target_lang: &str, meta: &LangMeta) -> String {
    let ranges: Vec<[u32; 2]> = meta.ranges.iter().map(|(lo, hi)| [*lo, *hi]).collect();
    json!({ "input": target_lang, "name": meta.name, "ranges": ranges }).to_string()
}

/// Read cached `LangMeta` if the blob was produced for `target_lang`.
pub fn lang_meta_from_cache(cache: &str, target_lang: &str) -> Option<LangMeta> {
    let val: Value = serde_json::from_str(cache).ok()?;
    if val.get("input").and_then(|i| i.as_str()) != Some(target_lang) {
        return None;
    }
    Some(parse_lang_meta(cache))
}

/// True if a code point is a Latin letter (Basic Latin, Latin-1 Supplement and
/// the Latin Extended blocks). Latin is always tolerated as the universal source
/// of loanwords and proper nouns.
fn is_latin_letter(c: char) -> bool {
    let u = c as u32;
    ((0x41..=0x5A).contains(&u) || (0x61..=0x7A).contains(&u)) // ASCII A-Z a-z
        || (0x00C0..=0x024F).contains(&u) // Latin-1 Supplement + Latin Extended-A/B
        || (0x1E00..=0x1EFF).contains(&u) // Latin Extended Additional
}

/// Count letters in `text` by category relative to the expected script:
/// (expected-script, Latin, other/foreign). Non-letters are ignored.
fn script_counts(text: &str, expected: &[(u32, u32)]) -> (usize, usize, usize) {
    let in_expected = |c: char| {
        let u = c as u32;
        expected.iter().any(|(lo, hi)| u >= *lo && u <= *hi)
    };
    let (mut good, mut latin, mut foreign) = (0usize, 0usize, 0usize);
    for c in text.chars() {
        if !c.is_alphabetic() {
            continue; // digits, punctuation, spaces, ♪, emoji → neutral
        }
        if in_expected(c) {
            good += 1;
        } else if is_latin_letter(c) {
            latin += 1;
        } else {
            foreign += 1;
        }
    }
    (good, latin, foreign)
}

/// Check whether a translated window is predominantly in the expected script.
///
/// - `expected` empty → unknown language, always OK (validation skipped).
/// - fewer than 8 letters total → too little to judge, OK.
/// - any non-Latin foreign-script letter (e.g. a stray Han character in Thai) →
///   fail.
/// - otherwise fail only if expected-script letters are a minority among
///   expected+Latin (catches whole windows that drifted to English).
pub fn window_script_ok(text: &str, expected: &[(u32, u32)]) -> bool {
    if expected.is_empty() {
        return true;
    }
    let (good, latin, foreign) = script_counts(text, expected);
    let total = good + latin + foreign;
    if total < 8 {
        return true;
    }
    if foreign > 0 {
        return false;
    }
    let denom = good + latin;
    denom > 0 && (good as f32 / denom as f32) >= 0.5
}

/// A continuous "how well does this match the target script" score in roughly
/// [-1, 1] (higher = better), used to pick the least-bad attempt when every
/// retry fails validation. Foreign-script letters count against the score.
/// Returns 1.0 when the language is unknown or there is nothing to judge.
pub fn script_score(text: &str, expected: &[(u32, u32)]) -> f32 {
    if expected.is_empty() {
        return 1.0;
    }
    let (good, latin, foreign) = script_counts(text, expected);
    let total = good + latin + foreign;
    if total == 0 {
        return 1.0;
    }
    (good as f32 - foreign as f32) / total as f32
}

/// Per-cue suspicion for the single-line cleanup pass: true if one translated
/// line looks like it is in the wrong language — it contains a foreign-script
/// letter (e.g. a stray Han character), OR it has Latin words but no
/// target-script letters at all (an untranslated / wrong-language line). A
/// normal target-script line, or one with just a Latin proper noun, is not
/// flagged. Always false when the target language is unknown.
pub fn cue_needs_retranslate(text: &str, expected: &[(u32, u32)]) -> bool {
    if expected.is_empty() {
        return false;
    }
    let (good, latin, foreign) = script_counts(text, expected);
    if foreign > 0 {
        return true; // stray CJK / Cyrillic etc. — always a wrong-language line
    }
    if good > 0 || latin < 3 {
        return false; // has target-script letters, or too little to judge
    }
    // Only Latin letters, no target script: distinguish an untranslated English
    // phrase (re-translate) from a proper noun / code that legitimately stays
    // Latin, e.g. "Krieger-45", "John", "Devora Doktor" (leave alone).
    looks_like_latin_phrase(text)
}

/// Heuristic: the text reads like a phrase/sentence (≥2 words, at least one
/// all-lowercase word) rather than a name or code. Used to avoid "translating"
/// proper nouns in the per-line cleanup pass.
fn looks_like_latin_phrase(text: &str) -> bool {
    let words: Vec<&str> = text
        .split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_alphabetic()))
        .collect();
    if words.len() < 2 {
        return false;
    }
    words.iter().any(|w| {
        let letters: Vec<char> = w.chars().filter(|c| c.is_alphabetic()).collect();
        letters.len() >= 2 && letters.iter().all(|c| c.is_lowercase())
    })
}

pub const DEFAULT_SYSTEM_ROLE: &str = "You translate subtitles. Output must be ONLY the translated lines, one per input line, without indices, timestamps, or any additional labels.";

pub const DEFAULT_TEMPLATE: &str = "{header}\n- Keep numbering (e.g., 12:, 43:, ...)\n- Do not change the number of lines or merge/split cues\n- Preserve line breaks within each numbered block exactly as in the input\n- Return ONLY the translated text blocks with the same numbering, no timestamps, no extra comments\n- Translate EVERY line fully into the target language; do not leave any line in the source language or in any other language\n- Write product, model, brand and code names and alphanumeric identifiers (e.g. iPhone, R2-D2) in Latin letters, not in the target script; do not translate them. Ordinary personal and place names may be rendered naturally in the target language.{extra}\n\nExample:\n1:\nHello!\n42:\nHow are you?\n\nText:\n{src_block}";

/// Default templates shipped by earlier versions. A stored override equal to
/// one of these (or to the current [`DEFAULT_TEMPLATE`]) means the user never
/// really customized the prompt — it is just some version's default — so we can
/// silently move them onto the current default by clearing the override.
pub const LEGACY_DEFAULT_TEMPLATES: &[&str] = &[
    // v2.1.1 and earlier.
    "{header}\n- Keep numbering (e.g., 12:, 43:, ...)\n- Do not change the number of lines or merge/split cues\n- Preserve line breaks within each numbered block exactly as in the input\n- Return ONLY the translated text blocks with the same numbering, no timestamps, no extra comments{extra}\n\n- New subtitles don't have to contain any characters in original language\nExample:\n1:\nHello!\n42:\nHow are you?\n\nText:\n{src_block}",
];

/// Default system roles shipped by earlier versions (none differ from the
/// current one yet).
pub const LEGACY_DEFAULT_SYSTEM_ROLES: &[&str] = &[];

/// True if `s` is the current default template or any previously shipped one.
pub fn is_known_default_template(s: &str) -> bool {
    s == DEFAULT_TEMPLATE || LEGACY_DEFAULT_TEMPLATES.contains(&s)
}

/// True if `s` is the current default system role or any previously shipped one.
pub fn is_known_default_system_role(s: &str) -> bool {
    s == DEFAULT_SYSTEM_ROLE || LEGACY_DEFAULT_SYSTEM_ROLES.contains(&s)
}

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

    // Thai letter block, used across the validation tests.
    const THAI: &[(u32, u32)] = &[(0x0E01, 0x0E5B)];

    #[test]
    fn script_ok_accepts_thai() {
        // "This house has a problem with the mirror." in Thai.
        assert!(window_script_ok("บ้านหลังนี้มีปัญหากับกระจก", THAI));
    }

    #[test]
    fn script_ok_rejects_whole_english_window() {
        assert!(!window_script_ok(
            "Really? Exactly. I even dozed off for a second. For two seconds.",
            THAI
        ));
    }

    #[test]
    fn script_ok_rejects_whole_chinese_window() {
        assert!(!window_script_ok("她总是感到焦虑，总是说，想搬出这个房子", THAI));
    }

    #[test]
    fn script_ok_rejects_single_stray_han() {
        // Mostly Thai but one Han character slipped in ("break her nose").
        assert!(!window_script_ok("ก่อนที่คุณจะพยายามทำให้เธอ 鼻 แตก", THAI));
    }

    #[test]
    fn script_ok_tolerates_latin_proper_noun() {
        // A Latin name inside otherwise-Thai text must not trip validation.
        assert!(window_script_ok(
            "สวัสดีครับ ผมชื่อ John และนี่คือบ้านของเรานะครับ",
            THAI
        ));
    }

    #[test]
    fn script_ok_skips_symbols_and_numbers() {
        assert!(window_script_ok("♪ ♪ 123 - 456 ♪", THAI));
    }

    #[test]
    fn script_ok_skips_unknown_language() {
        assert!(window_script_ok("anything at all goes here", &[]));
    }

    #[test]
    fn script_score_ranks_cleaner_output_higher() {
        let clean = script_score("บ้านหลังนี้มีปัญหากับกระจกบานนี้", THAI);
        let one_han = script_score("บ้านหลังนี้มีปัญหากับกระจก 鼻", THAI);
        let english = script_score("This entire window is still in English text", THAI);
        assert!(clean > one_han, "clean {clean} should beat one-han {one_han}");
        assert!(one_han > english, "one-han {one_han} should beat english {english}");
        assert!((script_score("anything", &[]) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn cue_needs_retranslate_flags_wrong_language_lines() {
        assert!(cue_needs_retranslate("还有什么?", THAI)); // stray Han
        assert!(cue_needs_retranslate("What else?", THAI)); // untranslated English phrase
        assert!(cue_needs_retranslate("I even dozed off for a second.", THAI));
        assert!(!cue_needs_retranslate("อะไรอีก?", THAI)); // correct Thai
        assert!(!cue_needs_retranslate("ผมชื่อ John", THAI)); // Thai + Latin name
        assert!(!cue_needs_retranslate("123 - ♪", THAI)); // no letters
        assert!(!cue_needs_retranslate("anything", &[])); // unknown language
        // Proper nouns / codes must be left alone, not "translated".
        assert!(!cue_needs_retranslate("Krieger-45.", THAI));
        assert!(!cue_needs_retranslate("John", THAI));
        assert!(!cue_needs_retranslate("- Devora - Doktor", THAI));
    }

    #[test]
    fn parse_lang_meta_reads_json() {
        let m = parse_lang_meta(r#"{"name": "Thai", "ranges": [[3585, 3675]]}"#);
        assert_eq!(m.name, "Thai");
        assert_eq!(m.ranges, vec![(3585, 3675)]);
    }

    #[test]
    fn parse_lang_meta_tolerates_fences_and_hex() {
        let m = parse_lang_meta("```json\n{\"name\":\"Thai\",\"ranges\":[[\"0x0E01\",\"0x0E5B\"]]}\n```");
        assert_eq!(m.name, "Thai");
        assert_eq!(m.ranges, vec![(0x0E01, 0x0E5B)]);
    }

    #[test]
    fn parse_lang_meta_empty_on_garbage() {
        let m = parse_lang_meta("I'm not sure what language that is.");
        assert!(m.name.is_empty());
        assert!(m.ranges.is_empty());
    }

    #[test]
    fn lang_meta_cache_roundtrip() {
        let meta = LangMeta { name: "Thai".into(), ranges: vec![(3585, 3675)] };
        let blob = lang_meta_to_cache("th", &meta);
        assert!(lang_meta_from_cache(&blob, "ru").is_none());
        let back = lang_meta_from_cache(&blob, "th").expect("cache hit");
        assert_eq!(back.name, "Thai");
        assert_eq!(back.ranges, vec![(3585, 3675)]);
    }
}
