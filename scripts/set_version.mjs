// Inject a version (from a git tag like "v2.0.0") into package.json,
// src-tauri/tauri.conf.json and src-tauri/Cargo.toml so the built binary,
// installer and updater manifest all agree. Run: node scripts/set_version.mjs v2.0.0
import { readFileSync, writeFileSync } from "node:fs";

const raw = process.argv[2];
if (!raw) {
  console.error("usage: node scripts/set_version.mjs <version|vX.Y.Z>");
  process.exit(1);
}
const version = raw.replace(/^v/, "");

function patch(path, fn) {
  const before = readFileSync(path, "utf8");
  const after = fn(before);
  writeFileSync(path, after);
  console.log(`updated ${path} -> ${version}`);
}

// package.json
patch("package.json", (s) => {
  const j = JSON.parse(s);
  j.version = version;
  return JSON.stringify(j, null, 2) + "\n";
});

// tauri.conf.json
patch("src-tauri/tauri.conf.json", (s) => {
  const j = JSON.parse(s);
  j.version = version;
  return JSON.stringify(j, null, 2) + "\n";
});

// Cargo.toml — replace the first `version = "..."` under [package].
patch("src-tauri/Cargo.toml", (s) =>
  s.replace(/^version = ".*"/m, `version = "${version}"`),
);
