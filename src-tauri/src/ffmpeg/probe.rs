//! ffprobe wrappers for listing subtitle streams (`ffmpeg/probe.py`).

use super::Stream;
use crate::tools::{find_tool, tool_command};
use serde::Deserialize;

#[derive(Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<Stream>,
}

/// Strict ffprobe: list all subtitle streams of a complete MKV.
pub fn ffprobe_subs(mkv_path: &str) -> Result<Vec<Stream>, String> {
    let ffprobe = find_tool("ffprobe")?;
    let output = tool_command(&ffprobe)
        .args([
            "-v", "error",
            "-select_streams", "s",
            "-show_entries",
            "stream=index,codec_name,codec_type,disposition:stream_tags=language,title",
            "-of", "json",
            mkv_path,
        ])
        .output()
        .map_err(|e| format!("failed to run ffprobe: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.trim().is_empty() {
            "ffprobe failed".to_string()
        } else {
            stderr.trim().to_string()
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: ProbeOutput =
        serde_json::from_str(&stdout).map_err(|e| format!("ffprobe JSON parse: {e}"))?;
    Ok(parsed.streams)
}

/// Lenient ffprobe for a possibly still-downloading MKV. (Live mode — wired in
/// a later pass.)
#[allow(dead_code)]
pub fn ffprobe_subs_partial(mkv_path: &str) -> Result<Vec<Stream>, String> {
    let ffprobe = find_tool("ffprobe")?;
    let output = tool_command(&ffprobe)
        .args([
            "-v", "error",
            "-err_detect", "ignore_err",
            "-select_streams", "s",
            "-show_entries",
            "stream=index,codec_name,codec_type,disposition,bit_rate:stream_tags=language,title",
            "-of", "json",
            mkv_path,
        ])
        .output()
        .map_err(|e| format!("failed to run ffprobe: {e}"))?;

    if !output.status.success() && output.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.trim().is_empty() {
            "ffprobe failed".to_string()
        } else {
            stderr.trim().to_string()
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: ProbeOutput = serde_json::from_str(&stdout).unwrap_or(ProbeOutput { streams: vec![] });
    Ok(parsed.streams)
}
