// Print the CHANGELOG.md section for a given version, used as GitHub release
// notes. Exits non-zero if the section is missing — a deliberate gate so no
// release ships without real notes (mirrors the Python project's rule).
// Run: node scripts/extract_changelog.mjs v2.0.0
import { readFileSync } from "node:fs";

const raw = process.argv[2];
if (!raw) {
  console.error("usage: node scripts/extract_changelog.mjs <version|vX.Y.Z>");
  process.exit(2);
}
const version = raw.replace(/^v/, "");
const text = readFileSync("CHANGELOG.md", "utf8");
const lines = text.split(/\r?\n/);

let capturing = false;
const out = [];
for (const line of lines) {
  const m = line.match(/^##\s+(\S+)/);
  if (m) {
    if (capturing) break; // next section starts
    if (m[1] === version) {
      capturing = true;
      continue;
    }
  }
  if (capturing) out.push(line);
}

const body = out.join("\n").trim();
if (!body) {
  console.error(`No CHANGELOG.md section for version ${version}`);
  process.exit(1);
}
console.log(body);
