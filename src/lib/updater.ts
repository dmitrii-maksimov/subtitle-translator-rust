// Thin wrapper over the Tauri updater + process plugins.
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export interface UpdateAvailable {
  version: string;
  notes: string;
  update: Update;
}

/// Check GitHub Releases for a newer signed build. Returns the update handle,
/// or null when up to date. Never throws for the "no update" case; network /
/// endpoint errors propagate so the caller can decide whether to surface them.
export async function checkForUpdate(): Promise<UpdateAvailable | null> {
  const update = await check();
  if (!update) return null;
  return { version: update.version, notes: update.body ?? "", update };
}

/// Download + install the update, then relaunch. Does not return on success.
export async function applyUpdate(update: Update): Promise<void> {
  await update.downloadAndInstall();
  await relaunch();
}
