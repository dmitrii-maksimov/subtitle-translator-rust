# Changelog

All notable changes to this project are documented here. Each released version
**must** have its own section — the GitHub release notes are generated from this
file by `scripts/extract_changelog.mjs`.

Format: one `## <version>` header per release, newest first.

## 2.2.0

### Changed
- Translation windows are now handed to the worker pool strictly in order
  (workers pick up windows 1,2,3,… and grab the next as they free up) instead
  of being split into per-thread chunks, so progress reads roughly front-to-back.
  The final subtitles are still reassembled by index, so output order is
  unchanged.

### Added
- **Temperature** setting (Settings → Translation). Controls how creative the
  model is when translating; defaults to a low `0.2` for more faithful,
  consistent output.
- The main-window log now auto-scrolls to follow new lines, and stops following
  as soon as you scroll up to read history (resumes when you scroll back down).
- Every retry, wrong-language detection, and per-line fix is now reported in the
  log as it happens, so you can see exactly what the app is doing.
- The default translation prompt now updates itself: if you never customized it,
  you silently get the new one; if you did customize it, the app offers to switch
  to the new default (Settings → Prompt overrides) and remembers if you decline.

### Fixed
- Subtitles no longer drift into the wrong language. Previously whole batches
  could come back in English (or with stray Chinese characters) when translating
  to another language, and were saved silently. Each translated window is now
  checked against the target language's script and, if it looks wrong, re-run
  with rising temperature (up to 3 attempts) so a stuck line can escape the
  wrong language instead of deterministically repeating it.
- A second cleanup pass then re-translates any individual line that is still in
  the wrong language on its own (short lines sometimes drift only because of the
  surrounding window context). Anything that still can't be produced in the
  target language is kept as the best attempt and flagged with a `[Warning]`.
- Product, model and code names / alphanumeric identifiers (e.g. `Krieger-45`)
  are now kept in Latin letters instead of being translated or transliterated;
  ordinary personal names are still rendered naturally in the target language.
- The target language is now resolved to its proper name and writing system
  (looked up once and cached), so the translation prompt names the language
  explicitly instead of passing a raw code like `th`.

## 2.1.1

### Fixed
- All settings now auto-save on change — theme, "Show Kodi integration" and
  every other option persist reliably (previously some could be lost without a
  manual Save). The explicit Save buttons are no longer needed.
- The "Update available" banner now renders the release notes as formatted text
  instead of raw markdown.

### Changed
- Reworked the Settings and Kodi tabs into a compact, consistent layout:
  sectioned panels with a floating title, right-aligned labels, custom number
  steppers (▲/▼), thinner controls, tidy custom scrollbars, and a resizable
  prompt box without the stray corner artifact.

## 2.1.0

Feature parity with the 1.x app: Kodi integration and the live/follow modes
are now ported.

### Added
- **Kodi integration**: a Kodi tab (behind the "Show Kodi integration" toggle)
  with connection settings, "Test connection", network discovery (SSDP +
  subnet scan), a Kodi folder browser, local↔Kodi path mapping with live
  preview, and live-mode settings.
- **Live-download mode**: translate subtitles from a still-downloading MKV —
  new full windows are translated as they arrive and the tail is finished once
  the file stops growing; resumable. Launched from "Live (downloading file)…".
- **Kodi-follow mode**: watch the active Kodi player and keep the translated
  subtitle track running ahead of playback (by the follow buffer), pushing it
  to Kodi automatically; switches to an embedded target-language track if one
  exists.
- **Open folder** in the Main tab: recursively find and batch every `.mkv` in
  a folder.
- **Theme switcher** (System / Light / Dark) and prompt **reset-to-default**
  buttons in Settings.
- **Download ffmpeg** button (Windows) when ffmpeg isn't found.
- Low-disk-space warning before remux.

### Fixed
- Model picker now persists the cached list and shows aligned prices.
- Updater endpoint corrected; app title no longer duplicated.

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
