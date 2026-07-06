//! End-to-end test of the ffmpeg-critical path (probe → extract → remux) that
//! can't be verified "blind": it builds a real MKV with a subtitle track, then
//! exercises the actual ffmpeg wrappers. Skips gracefully if ffmpeg is missing
//! (e.g. a CI runner without it installed).

use std::path::PathBuf;
use std::process::Command;

use subtitle_translator_rust_lib::ffmpeg::probe::ffprobe_subs;
use subtitle_translator_rust_lib::ffmpeg::extract::extract_srt;
use subtitle_translator_rust_lib::ffmpeg::remux::remux_with_translated_srt;
use subtitle_translator_rust_lib::srt::{self, Subtitle};

fn ffmpeg_bin() -> Option<String> {
    let out = Command::new("ffmpeg").arg("-version").output().ok()?;
    if out.status.success() {
        Some("ffmpeg".to_string())
    } else {
        None
    }
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("st_rust_test_{name}"))
}

#[test]
fn probe_extract_remux_roundtrip() {
    let Some(ffmpeg) = ffmpeg_bin() else {
        eprintln!("ffmpeg not found — skipping pipeline test");
        return;
    };

    // 1) Write a source SRT and mux it + a tiny black video into an MKV.
    let src_srt = tmp("src.srt");
    let subs = vec![
        Subtitle { index: 1, start_ms: 0, end_ms: 1000, content: "Hello world".into() },
        Subtitle { index: 2, start_ms: 1000, end_ms: 2000, content: "Second line".into() },
    ];
    srt::write_srt_crlf(&src_srt, &subs).unwrap();

    let mkv = tmp("fixture.mkv");
    let status = Command::new(&ffmpeg)
        .args([
            "-y",
            "-f", "lavfi",
            "-i", "color=c=black:s=160x90:d=2",
            "-f", "srt",
            "-i", src_srt.to_str().unwrap(),
            "-map", "0:v",
            "-map", "1:0",
            "-c:v", "mpeg4",
            "-c:s", "srt",
            "-metadata:s:s:0", "language=eng",
            "-metadata:s:s:0", "title=Full",
            mkv.to_str().unwrap(),
        ])
        .output()
        .expect("run ffmpeg to build fixture");
    assert!(
        status.status.success(),
        "fixture build failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );

    // 2) Probe: exactly one subtitle stream, English.
    let streams = ffprobe_subs(mkv.to_str().unwrap()).expect("probe");
    assert_eq!(streams.len(), 1, "expected 1 subtitle stream");
    let orig_idx = streams[0].index;
    assert_eq!(streams[0].language(), "eng");

    // 3) Extract to SRT and confirm content survived.
    let extracted = tmp("extracted.srt");
    extract_srt(mkv.to_str().unwrap(), orig_idx, extracted.to_str().unwrap()).expect("extract");
    let text = std::fs::read_to_string(&extracted).unwrap();
    let parsed = srt::parse(&text);
    assert_eq!(parsed.len(), 2);
    assert!(parsed[0].content.contains("Hello world"));

    // 4) Remux a "translated" SRT as a new track AND drop the original.
    let translated = tmp("translated.srt");
    let tsubs = vec![
        Subtitle { index: 1, start_ms: 0, end_ms: 1000, content: "Привет мир".into() },
        Subtitle { index: 2, start_ms: 1000, end_ms: 2000, content: "Вторая строка".into() },
    ];
    srt::write_srt_crlf(&translated, &tsubs).unwrap();

    let out_mkv = tmp("out.mkv");
    let mut log = |_l: String| {};
    remux_with_translated_srt(
        mkv.to_str().unwrap(),
        translated.to_str().unwrap(),
        &streams,
        &[orig_idx], // delete the original English track
        "rus",
        "Translated [rus] (russian)",
        out_mkv.to_str().unwrap(),
        &mut log,
    )
    .expect("remux");

    // 5) Probe the result: exactly one subtitle stream, Russian, new content.
    let result = ffprobe_subs(out_mkv.to_str().unwrap()).expect("probe result");
    assert_eq!(result.len(), 1, "expected exactly the new track (original dropped)");
    assert_eq!(result[0].language(), "rus");

    let check = tmp("check.srt");
    extract_srt(out_mkv.to_str().unwrap(), result[0].index, check.to_str().unwrap()).unwrap();
    let check_text = std::fs::read_to_string(&check).unwrap();
    assert!(check_text.contains("Привет мир"), "translated content missing in muxed track");

    // Cleanup best-effort.
    for f in [src_srt, mkv, extracted, translated, out_mkv, check] {
        let _ = std::fs::remove_file(f);
    }
}
