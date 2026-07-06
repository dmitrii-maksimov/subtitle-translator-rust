//! Static OpenAI pricing snapshot, ported from `pricing.py`.
//!
//! The `/v1/models` endpoint doesn't return prices, so we keep a local table
//! and join it to the model list when populating the picker. USD per 1M tokens.

use regex::Regex;
use std::sync::OnceLock;

#[allow(dead_code)]
pub const PRICING_DATE: &str = "2026-04-15";
#[allow(dead_code)]
pub const PRICING_SOURCE: &str = "https://openai.com/api/pricing/";

#[derive(Debug, Clone, Copy)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    // Cached-input rate when published; not surfaced in the UI yet.
    #[allow(dead_code)]
    pub cached: Option<f64>,
}

/// Base model id → price. Dated (`-YYYY-MM-DD`) and fine-tuned (`ft:...`) ids
/// resolve to these keys via [`base_model`].
fn table() -> &'static [(&'static str, ModelPrice)] {
    &[
        ("gpt-5.4", ModelPrice { input: 2.50, output: 15.00, cached: Some(0.25) }),
        ("gpt-5.4-mini", ModelPrice { input: 0.75, output: 4.50, cached: Some(0.075) }),
        ("gpt-5.4-nano", ModelPrice { input: 0.20, output: 1.25, cached: Some(0.02) }),
        ("gpt-5.4-pro", ModelPrice { input: 30.00, output: 180.00, cached: None }),
        ("gpt-5.3-chat", ModelPrice { input: 1.75, output: 14.00, cached: Some(0.175) }),
        ("gpt-5.2", ModelPrice { input: 0.875, output: 7.00, cached: Some(0.175) }),
        ("gpt-5.2-chat", ModelPrice { input: 1.75, output: 14.00, cached: Some(0.175) }),
        ("gpt-5.2-pro", ModelPrice { input: 10.50, output: 84.00, cached: None }),
        ("gpt-5.1", ModelPrice { input: 0.625, output: 5.00, cached: Some(0.125) }),
        ("gpt-5.1-chat", ModelPrice { input: 0.625, output: 5.00, cached: Some(0.125) }),
        ("gpt-5", ModelPrice { input: 0.625, output: 5.00, cached: Some(0.125) }),
        ("gpt-5-chat", ModelPrice { input: 1.25, output: 10.00, cached: Some(0.125) }),
        ("gpt-5-mini", ModelPrice { input: 0.125, output: 1.00, cached: Some(0.025) }),
        ("gpt-5-nano", ModelPrice { input: 0.05, output: 0.40, cached: Some(0.005) }),
        ("gpt-4.1", ModelPrice { input: 2.00, output: 8.00, cached: Some(0.50) }),
        ("gpt-4.1-mini", ModelPrice { input: 0.20, output: 0.80, cached: Some(0.10) }),
        ("gpt-4.1-nano", ModelPrice { input: 0.05, output: 0.20, cached: Some(0.025) }),
        ("gpt-4o", ModelPrice { input: 2.50, output: 10.00, cached: Some(1.25) }),
        ("gpt-4o-mini", ModelPrice { input: 0.15, output: 0.60, cached: Some(0.075) }),
        ("o3", ModelPrice { input: 2.00, output: 8.00, cached: Some(0.50) }),
        ("o3-mini", ModelPrice { input: 1.10, output: 4.40, cached: Some(0.55) }),
        ("o4-mini", ModelPrice { input: 1.10, output: 4.40, cached: Some(0.275) }),
        ("o1", ModelPrice { input: 15.00, output: 60.00, cached: Some(7.50) }),
        ("o1-mini", ModelPrice { input: 0.55, output: 2.20, cached: Some(0.55) }),
        ("gpt-4-turbo", ModelPrice { input: 5.00, output: 15.00, cached: None }),
        ("gpt-4", ModelPrice { input: 30.00, output: 60.00, cached: None }),
        ("gpt-3.5-turbo", ModelPrice { input: 0.50, output: 1.50, cached: None }),
        ("gpt-3.5-turbo-16k", ModelPrice { input: 3.00, output: 4.00, cached: None }),
    ]
}

fn date_suffix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"-\d{4}-\d{2}-\d{2}$").unwrap())
}

fn finetune_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^ft:([^:]+):").unwrap())
}

fn strip_date_suffix(model_id: &str) -> String {
    date_suffix_re().replace(model_id, "").to_string()
}

/// Resolve a model id to the key we look up in the table. Handles fine-tuned
/// ids (`ft:gpt-4o-mini-2024-07-18:org::x` → `gpt-4o-mini`) and dated variants.
pub fn base_model(model_id: &str) -> String {
    let base = if let Some(caps) = finetune_prefix_re().captures(model_id) {
        caps.get(1).unwrap().as_str().to_string()
    } else {
        model_id.to_string()
    };
    strip_date_suffix(&base)
}

fn lookup(key: &str) -> Option<ModelPrice> {
    table().iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Return pricing for a model id, resolving dated/fine-tuned variants.
pub fn get_pricing(model_id: &str) -> Option<ModelPrice> {
    if model_id.is_empty() {
        return None;
    }
    if let Some(p) = lookup(model_id) {
        return Some(p);
    }
    lookup(&base_model(model_id))
}

/// Short human-readable price tag, or `None` if unknown. Uses the same `%.3g`
/// formatting as the Python version.
pub fn format_pricing(model_id: &str) -> Option<String> {
    let p = get_pricing(model_id)?;
    Some(format!(
        "${} in / ${} out per 1M tok",
        fmt_g3(p.input),
        fmt_g3(p.output)
    ))
}

/// Filter helper: exclude obviously non-chat models (image/audio/etc).
pub fn is_text_completion_model(model_id: &str) -> bool {
    if model_id.is_empty() {
        return false;
    }
    if model_id.starts_with("ft:") {
        return true;
    }
    const BAD: &[&str] = &[
        "image", "audio", "realtime", "tts", "whisper", "transcribe", "embedding",
        "moderation", "search-preview", "dall-e", "babbage", "davinci", "codex",
    ];
    let low = model_id.to_lowercase();
    !BAD.iter().any(|b| low.contains(b))
}

/// Emulate Python's `%.3g` formatting (3 significant digits, no trailing zeros).
fn fmt_g3(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let magnitude = v.abs().log10().floor() as i32;
    let decimals = (2 - magnitude).max(0) as usize;
    let s = format!("{:.*}", decimals, v);
    // Trim trailing zeros / dot.
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_lookup() {
        let p = get_pricing("gpt-4o-mini").unwrap();
        assert_eq!(p.input, 0.15);
        assert_eq!(p.output, 0.60);
    }

    #[test]
    fn dated_variant_resolves() {
        let p = get_pricing("gpt-4o-mini-2024-07-18").unwrap();
        assert_eq!(p.input, 0.15);
    }

    #[test]
    fn finetune_prefix_resolves() {
        let p = get_pricing("ft:gpt-4o-mini-2024-07-18:org::abc").unwrap();
        assert_eq!(p.output, 0.60);
    }

    #[test]
    fn unknown_is_none() {
        assert!(get_pricing("nonexistent-model").is_none());
        assert!(get_pricing("").is_none());
    }

    #[test]
    fn chat_filter() {
        assert!(is_text_completion_model("gpt-4o-mini"));
        assert!(is_text_completion_model("ft:gpt-4o:org::x"));
        assert!(!is_text_completion_model("dall-e-3"));
        assert!(!is_text_completion_model("whisper-1"));
        assert!(!is_text_completion_model("text-embedding-3-small"));
    }

    #[test]
    fn format_uses_3_significant_digits() {
        // 0.15 -> "0.15", 0.6 -> "0.6"
        let tag = format_pricing("gpt-4o-mini").unwrap();
        assert_eq!(tag, "$0.15 in / $0.6 out per 1M tok");
    }
}
