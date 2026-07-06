# Agent instructions

Universal instructions for AI coding agents working in this repository
(Claude Code, and any tool that reads `AGENTS.md`). These rules are
**mandatory**, not suggestions.

## 1. Every release must document what changed

Releases are cut by pushing a `vX.Y.Z` git tag, which triggers
`.github/workflows/release.yml` (`tauri-action`). The GitHub release notes are
generated from `CHANGELOG.md` by `scripts/extract_changelog.mjs`.

**Before tagging a release you MUST:**
1. Add a `## X.Y.Z` section at the top of `CHANGELOG.md` (newest first)
   describing what was **Added / Changed / Fixed**, in user-facing terms.
2. The version is injected from the tag at build time by
   `scripts/set_version.mjs` (into `package.json`, `src-tauri/tauri.conf.json`
   and `src-tauri/Cargo.toml`). Keep these in sync locally too.
3. Use the same version for the tag and the `CHANGELOG.md` header.

`scripts/extract_changelog.mjs` exits non-zero if `CHANGELOG.md` has no section
for the tag — the release fails. Do not fake an entry; write real notes.

## 2. Keep README.md in sync

On every commit / PR, verify `README.md` still matches the app (features,
install steps, settings, requirements, platform support). A change that alters
user-visible behavior is not complete until the README reflects it.

## 3. Versioning

- Semantic `MAJOR.MINOR.PATCH`. Bump PATCH for fixes, MINOR for features.
- Single runtime source of truth is `src-tauri/Cargo.toml` (`version`), surfaced
  via the `app_version` command in the window title and Settings.

## 4. Before committing

- Run `cd src-tauri && cargo test` and keep it green.
- Run `npm run check` (Svelte/TypeScript) and keep it clean.
- Prefer `cargo build` / `npm run tauri build` to confirm the app still
  compiles when touching the Tauri boundary.

## 5. Updater signing key

The private updater key is **never** committed (`.gitignore`d as
`updater_key.key`). Its contents belong in the CI secret
`TAURI_SIGNING_PRIVATE_KEY`. The public key lives in
`src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.
