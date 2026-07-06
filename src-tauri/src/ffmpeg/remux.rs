//! ffmpeg remux helpers (`ffmpeg/remux.py`).
//!
//! Both functions push log lines through the `log` callback (the equivalent of
//! the Python generators yielding str lines) and return `Ok(())` on success.

use super::Stream;
use crate::tools::{find_tool, tool_command};
use std::process::Command;

/// Run ffmpeg, logging the command and (on failure) its stderr. Errors if
/// ffmpeg exits non-zero.
fn run_ffmpeg(mut cmd: Command, log: &mut dyn FnMut(String)) -> Result<(), String> {
    log("FFmpeg command:".to_string());
    let output = cmd
        .output()
        .map_err(|e| format!("failed to run ffmpeg: {e}"))?;
    if !output.status.success() {
        log(format!("FFmpeg exit code: {:?}", output.status.code()));
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            log("FFmpeg stderr:".to_string());
            for line in stderr.lines() {
                log(line.to_string());
            }
        }
        return Err(if stderr.trim().is_empty() {
            "ffmpeg failed".to_string()
        } else {
            stderr.trim().to_string()
        });
    }
    Ok(())
}

/// Copy an MKV excluding the given subtitle stream indexes. No new SRT track.
/// (Drop-only remux — wired in a later pass.)
#[allow(dead_code)]
pub fn remux_drop_streams(
    mkv_path: &str,
    streams: &[Stream],
    delete_indexes: &[i64],
    out_path: &str,
    log: &mut dyn FnMut(String),
) -> Result<(), String> {
    let ffmpeg = find_tool("ffmpeg")?;
    let kept: Vec<&Stream> = streams
        .iter()
        .filter(|st| !delete_indexes.contains(&st.index))
        .collect();

    let mut cmd = tool_command(&ffmpeg);
    cmd.args([
        "-y", "-i", mkv_path, "-map", "0:v?", "-map", "0:a?", "-map", "0:t?", "-map", "0:d?",
    ]);
    for st in &kept {
        cmd.args(["-map", &format!("0:{}", st.index)]);
    }
    cmd.args(["-c", "copy", "-max_interleave_delta", "0", out_path]);

    log_cmd(&cmd, log);
    run_ffmpeg(cmd, log)
}

/// Copy an MKV and mux `srt_path` as a new subtitle track, dropping any streams
/// whose index is in `delete_indexes`.
#[allow(clippy::too_many_arguments)]
pub fn remux_with_translated_srt(
    mkv_path: &str,
    srt_path: &str,
    streams: &[Stream],
    delete_indexes: &[i64],
    iso3: &str,
    title: &str,
    out_path: &str,
    log: &mut dyn FnMut(String),
) -> Result<(), String> {
    let ffmpeg = find_tool("ffmpeg")?;
    let kept: Vec<&Stream> = streams
        .iter()
        .filter(|st| !delete_indexes.contains(&st.index))
        .collect();
    let new_track_index = kept.len();

    let mut cmd = tool_command(&ffmpeg);
    if !delete_indexes.is_empty() {
        cmd.args([
            "-y", "-i", mkv_path, "-f", "srt", "-i", srt_path,
            "-map", "0:v?", "-map", "0:a?", "-map", "0:t?", "-map", "0:d?",
        ]);
        for st in &kept {
            cmd.args(["-map", &format!("0:{}", st.index)]);
        }
    } else {
        cmd.args([
            "-y", "-i", mkv_path, "-f", "srt", "-i", srt_path, "-map", "0",
        ]);
    }

    cmd.args([
        "-map", "1:0",
        "-c", "copy",
        "-max_interleave_delta", "0",
        &format!("-c:s:{new_track_index}"), "srt",
        &format!("-metadata:s:s:{new_track_index}"), &format!("language={iso3}"),
        &format!("-metadata:s:s:{new_track_index}"), &format!("title={title}"),
        out_path,
    ]);

    log_cmd(&cmd, log);
    run_ffmpeg(cmd, log)
}

/// Log a readable, shell-ish rendering of the command about to run.
fn log_cmd(cmd: &Command, log: &mut dyn FnMut(String)) {
    let mut parts: Vec<String> = vec![cmd.get_program().to_string_lossy().to_string()];
    for a in cmd.get_args() {
        let s = a.to_string_lossy().to_string();
        if s.contains(' ') || s.is_empty() {
            parts.push(format!("'{s}'"));
        } else {
            parts.push(s);
        }
    }
    log(parts.join(" "));
}
