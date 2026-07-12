// TypeScript mirrors of the Rust structs exchanged over Tauri commands.

export interface AppSettings {
  api_key: string;
  api_base: string;
  model: string;
  workers: number;
  window: number;
  overlap: number;
  temperature: number;
  target_language: string;
  last_dir: string;
  fulllog: boolean;
  extra_prompt: string;
  overwrite_original: boolean;
  main_prompt_template: string;
  system_role: string;
  prompt_migration_declined: string;
  default_source_lang: string;
  default_source_title: string;
  cached_tag_lang: string;
  cached_iso3: string;
  cached_source_lang_input: string;
  cached_lang_meta: string;
  cached_models: string[];
  use_custom_model: boolean;
  kodi_host: string;
  kodi_port: number;
  kodi_user: string;
  kodi_password: string;
  kodi_source_path: string;
  local_parent_path: string;
  live_poll_interval: number;
  live_stable_threshold: number;
  kodi_follow_buffer_min: number;
  auto_check_updates: boolean;
  last_update_check: number;
  skip_version: string;
  show_kodi: boolean;
  theme: "system" | "light" | "dark";
}

export interface Stream {
  index: number;
  codec_name: string;
  codec_type: string;
  disposition: Record<string, number>;
  tags: Record<string, string>;
}

export interface ModelInfo {
  id: string;
  price: string | null;
  is_chat: boolean;
}

export interface FileDecision {
  filePath: string;
  translateStreamIndex: number | null;
  deleteStreamIndexes: number[];
}

// Per-stream translate/delete choice used inside the track dialog + carry-over.
export interface TrackPref {
  translate: boolean;
  delete: boolean;
}

export interface KodiInstance {
  ip: string;
  port: number;
  name: string;
  source: string;
}

export interface KodiEntry {
  label: string;
  file: string;
  is_dir: boolean;
}

export interface KodiPing {
  ok: boolean;
  message: string;
}

// A non-null placeholder so the settings state variable is always AppSettings
// (real values overwrite it right after loading from the backend).
export function emptySettings(): AppSettings {
  return {
    api_key: "",
    api_base: "https://api.openai.com/v1",
    model: "gpt-4o-mini",
    workers: 5,
    window: 25,
    overlap: 10,
    temperature: 0.2,
    target_language: "ru",
    last_dir: "",
    fulllog: false,
    extra_prompt: "",
    overwrite_original: true,
    main_prompt_template: "",
    system_role: "",
    prompt_migration_declined: "",
    default_source_lang: "eng",
    default_source_title: "Full",
    cached_tag_lang: "",
    cached_iso3: "",
    cached_source_lang_input: "",
    cached_lang_meta: "",
    cached_models: [],
    use_custom_model: false,
    kodi_host: "",
    kodi_port: 8080,
    kodi_user: "kodi",
    kodi_password: "",
    kodi_source_path: "",
    local_parent_path: "",
    live_poll_interval: 30,
    live_stable_threshold: 30,
    kodi_follow_buffer_min: 10,
    auto_check_updates: true,
    last_update_check: 0,
    skip_version: "",
    show_kodi: false,
    theme: "system",
  };
}
