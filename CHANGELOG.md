# Changelog

All notable changes to this project are documented here. Each released version
**must** have its own section — the GitHub release notes are generated from this
file by `scripts/extract_changelog.mjs`.

Format: one `## <version>` header per release, newest first.

## 2.0.2

### Fixed
- Update checks always failed ("Could not fetch a valid release JSON") — the
  updater endpoint pointed at the wrong repository. It now points at
  `subtitle-translator-rust` releases.
- The app name was shown both in the OS title bar and again inside the window;
  removed the in-window duplicate (the name stays in the title bar, the version
  on the Settings tab).

## 2.0.1

### Added
- **Theme switcher** in Settings: System (follow OS) / Light / Dark, applied
  instantly and persisted.
- **Download ffmpeg** button in the "ffmpeg not found" banner — fetches
  ffmpeg/ffprobe next to the app on Windows, with progress.
- Built-in **auto-updater** wired into the UI: startup check (toggleable),
  a "Check for updates now" button in Settings, and an "Install & Restart"
  banner when a newer signed release is available.

### Fixed
- App version was duplicated in the window title and inside the window; it now
  shows only on the Settings tab, and the OS title bar just reads
  "Subtitle Translator".
- Settings layout: dropdowns and text fields now share the same height, and
  Workers / Window / Overlap are full-width rows again.

## 2.0.0

Complete rewrite of Subtitle Translator on **Rust + Tauri** (the previous
1.x line was Python + PySide6/Qt). This first Rust release ships the core
batch-translation workflow with feature parity for the main use case.

### Added
- Native Tauri desktop app with a Svelte + TypeScript frontend; tiny signed
  binaries and a built-in auto-updater.
- Batch translation of MKV subtitle tracks: pick one or more `.mkv` files, a
  per-file dialog lists every subtitle track (via ffprobe), tick one track to
  **Translate** and any to **Delete**, with **Save & Continue / Skip / Cancel**.
- Selection carry-over across a batch: tracks matching `(language, title, codec)`
  are pre-filled from the previous file.
- Translation via any OpenAI-compatible Chat Completions API, with parallel
  windowed translation (configurable workers / window / overlap) and overlap
  context for consistency across chunk boundaries.
- Re-mux of the translated SRT back into the MKV as a new track (with inferred
  ISO 639-2 language tag and title), dropping tracks marked for deletion in the
  same pass. Overwrite-in-place or keep a `.translated.mkv` copy.
- Standalone `.srt` / `.str` files translated in place.
- Model picker in Settings populated from `/v1/models` with per-1M-token pricing
  shown inline, plus a **Custom** model id and **Refresh**.
- Light/dark theme-aware UI; live progress and cooperative cancellation.
- Settings stored in the same `~/.subtitle_translator_settings.json` as the 1.x
  app, so existing configs keep working.

### Not yet ported (planned for later passes)
- Kodi integration (JSON-RPC client, LAN discovery, path mapping, Kodi tab).
- Live-download mode and Kodi-follow mode.
- Windows automatic ffmpeg download.
