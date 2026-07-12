# Subtitle Translator (Rust / Tauri edition)

A cross-platform desktop app for extracting, translating, and re-muxing
subtitles inside MKV files.

Built with **Rust + Tauri 2** (backend) and **Svelte + TypeScript** (UI), using
any OpenAI-compatible Chat Completions API for translation. This is the 2.x
rewrite of the original Python/PySide6 app.

## Features

- Pick one or more MKV files — a per-file dialog lists every subtitle track
  (via `ffprobe`).
- For each track, tick **Translate** (one per file) and/or **Delete**.
  **Save & Continue** moves to the next file; **Skip** leaves it alone;
  **Cancel** aborts the batch.
- Selection carry-over: the next file's tracks matching `(language, title,
  codec)` are pre-filled — one pass of setup for a whole TV-show folder.
- Translate via any OpenAI-compatible Chat Completions API (OpenAI, a local
  proxy, etc.).
- Re-mux the translated SRT back into the MKV as a new track (with inferred ISO
  639-2 language tag + title), dropping any tracks marked for deletion in the
  same pass. Overwrite the original in place, or keep a `.translated.mkv` copy.
- Standalone `.srt` / `.str` files are translated in place.
- Parallel windowed translation with configurable workers, window size, and
  overlap context.
- Model picker in Settings populated from `/v1/models` with input/output price
  per 1M tokens shown inline, a **Refresh** button, and a **Custom** model id.
- Light/dark theme-aware UI, live progress, cancellable operations.
- Built-in auto-updater (signed) via GitHub Releases.

> **Not yet ported** from the 1.x app (planned for later passes): Kodi
> integration, live-download mode, Kodi-follow mode, and Windows automatic
> ffmpeg download. See `CHANGELOG.md`.

## Requirements

- `ffmpeg` and `ffprobe` in `PATH` (or next to the executable).
  - macOS: `brew install ffmpeg`
  - Linux: `sudo apt install ffmpeg` (or your distro's equivalent)
  - Windows: download from <https://ffmpeg.org/download.html> and add to PATH.
- An OpenAI-compatible API key and endpoint.

## Development

Prerequisites: Node 18+, the Rust toolchain, and the
[Tauri system dependencies](https://tauri.app/start/prerequisites/) for your OS.

```bash
npm install
npm run tauri dev      # run the app with hot-reload
```

> **WSL / WSLg note:** if the window shows GL/EGL warnings or a blank view,
> launch with software rendering flags:
> `WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 npm run tauri dev`.

Useful commands:

```bash
npm run check                       # Svelte / TypeScript type-check
(cd src-tauri && cargo test)        # Rust unit tests (SRT, engine, pricing, …)
npm run tauri build                 # build installers for the current OS
```

## Configuration

Settings live in `~/.subtitle_translator_settings.json` (shared format with the
1.x app) and are editable in the **Settings** tab:

| Setting | Description | Default |
|---|---|---|
| API Key | Your OpenAI-compatible API key | — |
| API Base URL | API endpoint | `https://api.openai.com/v1` |
| Model | Chat model id (pick from the combo or tick **Custom**) | `gpt-4o-mini` |
| Target Language | Language to translate into | `ru` |
| Workers | Parallel translation threads (max 10) | `5` |
| Window | Subtitles per translation chunk | `25` |
| Overlap | Context overlap between chunks | `10` |
| Temperature | Sampling temperature (0–2); low keeps output faithful and on-language | `0.2` |
| Overwrite original | Replace the source MKV in place | on |

## How it works

1. **Select** — pick MKV file(s). For each, a dialog lists its subtitle tracks;
   tick one as the translation source and any for deletion.
2. **Extract** — ffmpeg pulls the chosen track to a temporary SRT.
3. **Translate** — the SRT is split into overlapping windows translated in
   parallel via the Chat API; overlap keeps translations consistent across
   chunk boundaries. Each window is checked against the target language's
   writing system; if it drifts into the wrong language it is re-run with rising
   temperature (up to 3 attempts), and any window that still looks wrong is kept
   as the best attempt with a `[Warning]` in the log.
4. **Re-mux** — the translated SRT is muxed back as a new track (deleted tracks
   excluded via explicit `-map` whitelisting). By default the original is
   replaced; otherwise a new `.translated.mkv` is created.

## Releases & auto-update

Pushing a `vX.Y.Z` tag triggers `.github/workflows/release.yml`
(`tauri-action`), which builds installers for macOS, Windows and Linux and
publishes a GitHub Release plus the updater `latest.json`. Release notes are
generated from `CHANGELOG.md`.

Required CI secrets: `TAURI_SIGNING_PRIVATE_KEY` (contents of the private
updater key) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The matching public key
is committed in `src-tauri/tauri.conf.json`.

## Project structure

```
src/                      # Svelte + TS frontend
  routes/+page.svelte     # app shell: tabs, event wiring
  lib/                    # MainTab, SettingsTab, TrackSelection, ModelPicker,
                          # tauri.ts (invoke/listen), stores.ts, types.ts
src-tauri/src/            # Rust backend
  settings.rs             # AppSettings (serde) + JSON persistence
  srt.rs                  # SRT parse/compose/reindex + sanitize
  services.rs             # OpenAI-compatible Chat API client
  engine.rs               # parallel windowed translation
  ffmpeg/{probe,extract,remux}.rs
  track_matcher.rs        # source-track picking + carry-over
  pricing.rs              # static model pricing table
  tools.rs                # ffmpeg/ffprobe discovery
  orchestrate.rs          # read → translate → write → remux flow
  commands.rs             # #[tauri::command] layer + progress events
```

## License

MIT
