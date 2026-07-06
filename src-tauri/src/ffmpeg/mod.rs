//! ffmpeg/ffprobe subprocess wrappers, ported from `subtitle_translator/ffmpeg/`.
//!
//! ffmpeg stays an external binary; these modules build the exact same argument
//! vectors the Python code used and parse ffprobe's JSON output.

pub mod extract;
pub mod probe;
pub mod remux;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A subtitle stream as reported by ffprobe. Extra JSON fields are ignored.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Stream {
    pub index: i64,
    #[serde(default)]
    pub codec_name: String,
    #[serde(default)]
    pub codec_type: String,
    #[serde(default)]
    pub disposition: BTreeMap<String, i64>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
}

impl Stream {
    pub fn language(&self) -> &str {
        self.tags.get("language").map(|s| s.as_str()).unwrap_or("")
    }
    pub fn title(&self) -> &str {
        self.tags.get("title").map(|s| s.as_str()).unwrap_or("")
    }
    pub fn disposition_flag(&self, key: &str) -> bool {
        self.disposition.get(key).copied().unwrap_or(0) != 0
    }
}
