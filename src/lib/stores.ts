import { writable } from "svelte/store";
import type { AppSettings } from "./types";

// Populated once at startup from the backend (which owns the JSON file).
export const settings = writable<AppSettings | null>(null);

// Live job state, driven by progress events.
export const fileProgress = writable(0);
export const batchProgress = writable(0);
export const batchLabel = writable("");
export const logLines = writable<string[]>([]);
export const running = writable(false);

export function appendLog(line: string) {
  logLines.update((lines) => {
    const next = [...lines, line];
    // Keep the log bounded so long batches don't grow unbounded.
    return next.length > 2000 ? next.slice(next.length - 2000) : next;
  });
}

export function resetJob() {
  fileProgress.set(0);
  batchProgress.set(0);
  batchLabel.set("");
  logLines.set([]);
}
