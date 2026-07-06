//! Pure helpers for picking and matching subtitle tracks
//! (`core/track_matcher.py`).

use crate::ffmpeg::Stream;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Per-stream translate/delete choice, used for carry-over across a batch.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct TrackPref {
    #[serde(default)]
    pub translate: bool,
    #[serde(default)]
    pub delete: bool,
}

/// Tuple `(language, title, codec)` identifying equivalent tracks across files.
pub fn stream_match_key(stream: &Stream) -> (String, String, String) {
    let lang = stream.tags.get("language").cloned().unwrap_or_else(|| "und".to_string());
    let lang = if lang.is_empty() { "und".to_string() } else { lang };
    let title = stream.title().to_string();
    let codec = stream.codec_name.clone();
    (lang, title, codec)
}

/// Return `{stream_index: TrackPref}` pre-filled from `previous_prefs`, keyed by
/// `(lang, title, codec)`. Carry-over is currently computed in the frontend;
/// kept here (with tests) as the authoritative reference for a later move.
#[allow(dead_code)]
pub fn match_initial_state(
    streams: &[Stream],
    previous_prefs: &BTreeMap<(String, String, String), TrackPref>,
) -> BTreeMap<i64, TrackPref> {
    let mut result = BTreeMap::new();
    for st in streams {
        let key = stream_match_key(st);
        let prefs = previous_prefs.get(&key).copied().unwrap_or_default();
        result.insert(st.index, prefs);
    }
    result
}

/// Pick the best source subtitle stream.
///
/// Priority: skip the target language; skip ASS/SSA; prefer English; within a
/// language prefer titles in {"", "full"} with SDH ranked last. Returns the
/// chosen stream's index, or `None`.
pub fn pick_source_subtitle_stream(streams: &[Stream], target_lang: &str) -> Option<i64> {
    let target_lower = target_lang.to_lowercase();
    let target_lower = target_lower.trim();

    let is_target = |lang: &str| -> bool {
        if target_lower.is_empty() {
            return false;
        }
        lang.starts_with(target_lower) || target_lower.starts_with(lang)
    };

    let title_rank = |title: &str, st: &Stream| -> i64 {
        let title_l = title.trim().to_lowercase();
        if title_l.contains("sdh") || title_l.contains("hearing") {
            return 2;
        }
        if st.disposition_flag("hearing_impaired") {
            return 2;
        }
        if title_l.is_empty() || title_l == "full" {
            return 0;
        }
        1
    };

    let mut candidates: Vec<((i64, i64, i64), i64)> = Vec::new();
    for s in streams {
        let codec = s.codec_name.to_lowercase();
        if codec == "ass" || codec == "ssa" {
            continue;
        }
        let lang = s.language().to_lowercase();
        let lang = lang.trim();
        if is_target(lang) {
            continue;
        }
        let t_rank = title_rank(s.title(), s);
        let l_rank = if lang == "eng" || lang == "en" { 0 } else { 1 };
        candidates.push(((l_rank, t_rank, s.index), s.index));
    }

    candidates.sort_by_key(|(k, _)| *k);
    candidates.first().map(|(_, idx)| *idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn stream(index: i64, lang: &str, title: &str, codec: &str) -> Stream {
        let mut tags = BTreeMap::new();
        if !lang.is_empty() {
            tags.insert("language".to_string(), lang.to_string());
        }
        if !title.is_empty() {
            tags.insert("title".to_string(), title.to_string());
        }
        Stream {
            index,
            codec_name: codec.to_string(),
            codec_type: "subtitle".to_string(),
            disposition: BTreeMap::new(),
            tags,
        }
    }

    #[test]
    fn carry_over_exact_match() {
        let streams = vec![stream(2, "eng", "Full", "subrip")];
        let mut prev = BTreeMap::new();
        prev.insert(
            ("eng".to_string(), "Full".to_string(), "subrip".to_string()),
            TrackPref { translate: true, delete: false },
        );
        let state = match_initial_state(&streams, &prev);
        assert!(state.get(&2).unwrap().translate);
    }

    #[test]
    fn empty_lang_tag_becomes_und() {
        let s = stream(1, "", "", "subrip");
        let key = stream_match_key(&s);
        assert_eq!(key.0, "und");
    }

    #[test]
    fn codec_diff_breaks_match() {
        let streams = vec![stream(2, "eng", "Full", "ass")];
        let mut prev = BTreeMap::new();
        prev.insert(
            ("eng".to_string(), "Full".to_string(), "subrip".to_string()),
            TrackPref { translate: true, delete: false },
        );
        let state = match_initial_state(&streams, &prev);
        assert!(!state.get(&2).unwrap().translate);
    }

    #[test]
    fn pick_prefers_english_full_over_sdh() {
        let streams = vec![
            stream(0, "eng", "SDH", "subrip"),
            stream(1, "eng", "Full", "subrip"),
            stream(2, "ger", "", "subrip"),
        ];
        assert_eq!(pick_source_subtitle_stream(&streams, "ru"), Some(1));
    }

    #[test]
    fn pick_skips_target_and_ass() {
        let streams = vec![
            stream(0, "rus", "", "subrip"), // target -> skip
            stream(1, "eng", "", "ass"),    // ass -> skip
            stream(2, "ger", "", "subrip"),
        ];
        assert_eq!(pick_source_subtitle_stream(&streams, "rus"), Some(2));
    }

    #[test]
    fn pick_none_when_nothing_fits() {
        let streams = vec![stream(0, "rus", "", "subrip")];
        assert_eq!(pick_source_subtitle_stream(&streams, "rus"), None);
    }
}
