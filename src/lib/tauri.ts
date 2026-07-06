// Typed wrappers over Tauri commands + progress events.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppSettings,
  FileDecision,
  KodiEntry,
  KodiInstance,
  KodiPing,
  ModelInfo,
  Stream,
} from "./types";

export const api = {
  loadSettings: () => invoke<AppSettings>("load_settings"),
  saveSettings: (settings: AppSettings) => invoke<void>("save_settings", { settings }),
  appVersion: () => invoke<string>("app_version"),
  defaultPrompts: () =>
    invoke<{ main_prompt_template: string; system_role: string }>("default_prompts"),
  checkFfmpeg: () => invoke<boolean>("check_ffmpeg"),
  installFfmpeg: () => invoke<string>("install_ffmpeg"),
  priceFor: (model: string) => invoke<string | null>("price_for", { model }),
  listModels: () => invoke<ModelInfo[]>("list_models"),
  modelsInfo: (ids: string[]) => invoke<ModelInfo[]>("models_info", { ids }),
  probeSubs: (path: string) => invoke<Stream[]>("probe_subs", { path }),
  probeSubsPartial: (path: string) => invoke<Stream[]>("probe_subs_partial", { path }),
  startLive: (mkvPath: string, streamIndex: number) =>
    invoke<void>("start_live", { mkvPath, streamIndex }),
  pickSourceStream: (streams: Stream[], targetLang: string) =>
    invoke<number | null>("pick_source_stream", { streams, targetLang }),
  cancelJob: () => invoke<void>("cancel_job"),
  kodiPing: (host: string, port: number, user: string, password: string) =>
    invoke<KodiPing>("kodi_ping", { host, port, user, password }),
  kodiDiscover: (portHint: number) =>
    invoke<KodiInstance[]>("kodi_discover", { portHint }),
  kodiBrowse: (
    host: string,
    port: number,
    user: string,
    password: string,
    path: string | null,
  ) => invoke<KodiEntry[]>("kodi_browse", { host, port, user, password, path }),
  kodiMapPreview: (localParent: string, kodiParent: string) =>
    invoke<string>("kodi_map_preview", { localParent, kodiParent }),
  startKodiFollow: () => invoke<void>("start_kodi_follow"),
  translateSrtFile: (path: string) => invoke<string>("translate_srt_file", { path }),
  runBatch: (decisions: FileDecision[]) => invoke<string>("run_batch", { decisions }),
};

export interface BatchEvent {
  value: number;
  text: string;
}

// Subscribe to all job progress events. Returns an unlisten function.
export async function subscribeProgress(handlers: {
  onProgress?: (v: number) => void;
  onStatus?: (s: string) => void;
  onLog?: (s: string) => void;
  onBatch?: (e: BatchEvent) => void;
}): Promise<UnlistenFn> {
  const unlisteners: UnlistenFn[] = [];
  if (handlers.onProgress)
    unlisteners.push(await listen<number>("progress", (e) => handlers.onProgress!(e.payload)));
  if (handlers.onStatus)
    unlisteners.push(await listen<string>("status", (e) => handlers.onStatus!(e.payload)));
  if (handlers.onLog)
    unlisteners.push(await listen<string>("log", (e) => handlers.onLog!(e.payload)));
  if (handlers.onBatch)
    unlisteners.push(await listen<BatchEvent>("batch", (e) => handlers.onBatch!(e.payload)));
  return () => unlisteners.forEach((u) => u());
}
