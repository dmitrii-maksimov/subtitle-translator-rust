//! ffmpeg wrappers for extracting a subtitle stream to SRT (`ffmpeg/extract.py`).

use crate::tools::{find_tool, tool_command};
use std::path::Path;

/// Strict extraction of a subtitle stream to SRT for the normal batch flow.
pub fn extract_srt(mkv_path: &str, stream_index: i64, out_path: &str) -> Result<String, String> {
    let ffmpeg = find_tool("ffmpeg")?;
    let output = tool_command(&ffmpeg)
        .args([
            "-y",
            "-i", mkv_path,
            "-map", &format!("0:{stream_index}"),
            "-c:s", "srt",
            out_path,
        ])
        .output()
        .map_err(|e| format!("failed to run ffmpeg: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.trim().is_empty() {
            "ffmpeg extract failed".to_string()
        } else {
            stderr.trim().to_string()
        });
    }
    if !file_has_content(out_path) {
        return Err("ffmpeg produced no SRT output".to_string());
    }
    Ok(out_path.to_string())
}

/// Lenient extraction from a partial/still-downloading MKV. Returns `None` on
/// hard failure (matches the Python return-None contract used by live mode).
/// (Live mode — wired in a later pass.)
#[allow(dead_code)]
pub fn extract_srt_lenient(
    mkv_path: &str,
    stream_index: i64,
    out_path: Option<&str>,
) -> Option<String> {
    let default_out;
    let out = match out_path {
        Some(p) => p.to_string(),
        None => {
            let base = strip_extension(mkv_path);
            default_out = format!("{base}.live.stream{stream_index}.srt");
            default_out.clone()
        }
    };
    let ffmpeg = find_tool("ffmpeg").ok()?;
    let status = tool_command(&ffmpeg)
        .args([
            "-y",
            "-err_detect", "ignore_err",
            "-fflags", "+genpts+igndts",
            "-i", mkv_path,
            "-map", &format!("0:{stream_index}"),
            "-c:s", "srt",
            &out,
        ])
        .output()
        .ok()?;

    if !status.status.success() && !Path::new(&out).exists() {
        return None;
    }
    if !file_has_content(&out) {
        return None;
    }
    Some(out)
}

fn file_has_content(path: &str) -> bool {
    std::fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false)
}

fn strip_extension(path: &str) -> String {
    match path.rfind('.') {
        Some(pos) if pos > path.rfind(['/', '\\']).map(|p| p + 1).unwrap_or(0) => {
            path[..pos].to_string()
        }
        _ => path.to_string(),
    }
}
