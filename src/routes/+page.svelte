<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { api, subscribeProgress } from "$lib/tauri";
  import { fileProgress, batchProgress, batchLabel, appendLog } from "$lib/stores";
  import MainTab from "$lib/MainTab.svelte";
  import SettingsTab from "$lib/SettingsTab.svelte";
  import { emptySettings } from "$lib/types";
  import { checkForUpdate, applyUpdate, type UpdateAvailable } from "$lib/updater";

  let version = $state("");

  let tab = $state<"main" | "settings">("main");
  let ffmpegOk = $state(true);
  let installingFfmpeg = $state(false);
  let ffmpegError = $state("");
  let loaded = $state(false);
  let appSettings = $state(emptySettings());
  let unlisten: UnlistenFn | null = null;

  // Updater state.
  let update = $state<UpdateAvailable | null>(null);
  let updating = $state(false);
  let updateStatus = $state("");

  async function checkUpdates(manual: boolean) {
    updateStatus = "Checking…";
    try {
      update = await checkForUpdate();
      updateStatus = update
        ? `Update ${update.version} available.`
        : manual
          ? "You're on the latest version."
          : "";
    } catch (e) {
      updateStatus = manual ? `Update check failed: ${e}` : "";
    }
  }

  async function installUpdate() {
    if (!update) return;
    updating = true;
    updateStatus = "Downloading update…";
    try {
      await applyUpdate(update.update); // relaunches on success; does not return
    } catch (e) {
      updateStatus = `Update failed: ${e}`;
      updating = false;
    }
  }

  async function installFfmpeg() {
    installingFfmpeg = true;
    ffmpegError = "";
    fileProgress.set(0);
    try {
      await api.installFfmpeg();
      ffmpegOk = await api.checkFfmpeg();
      if (!ffmpegOk) ffmpegError = "ffmpeg still not detected after install.";
    } catch (e) {
      ffmpegError = String(e);
    } finally {
      installingFfmpeg = false;
    }
  }

  onMount(async () => {
    appSettings = await api.loadSettings();
    loaded = true;
    version = await api.appVersion();
    ffmpegOk = await api.checkFfmpeg();
    if (appSettings.auto_check_updates) checkUpdates(false);

    unlisten = await subscribeProgress({
      onProgress: (v) => fileProgress.set(v),
      onStatus: (line) => appendLog(line),
      onLog: (line) => appendLog(line),
      onBatch: (e) => {
        batchProgress.set(e.value);
        batchLabel.set(e.text);
      },
    });
  });

  onDestroy(() => unlisten?.());

  // Apply the chosen theme: "system" removes the override (falls back to the
  // prefers-color-scheme media query); "light"/"dark" force it via a
  // data-theme attribute + color-scheme (so native controls follow too).
  function applyTheme(theme: string) {
    const root = document.documentElement;
    if (theme === "light" || theme === "dark") {
      root.dataset.theme = theme;
      root.style.colorScheme = theme;
    } else {
      delete root.dataset.theme;
      root.style.colorScheme = "light dark";
    }
  }

  $effect(() => {
    if (loaded) applyTheme(appSettings.theme);
  });
</script>

<svelte:head>
  <title>Subtitle Translator</title>
</svelte:head>

<div class="app">
  <header>
    <h1>Subtitle Translator</h1>
    <nav>
      <button class:active={tab === "main"} onclick={() => (tab = "main")}>Main</button>
      <button class:active={tab === "settings"} onclick={() => (tab = "settings")}>
        Settings
      </button>
    </nav>
  </header>

  {#if !ffmpegOk}
    <div class="warn">
      <span>⚠ ffmpeg / ffprobe not found. It's required to extract and remux MKV
        subtitle tracks.</span>
      <button onclick={installFfmpeg} disabled={installingFfmpeg}>
        {installingFfmpeg ? "Installing…" : "Download ffmpeg"}
      </button>
      {#if ffmpegError}<span class="warn-err">{ffmpegError}</span>{/if}
    </div>
  {/if}

  {#if update}
    <div class="update">
      <span>⬆ Update <b>{update.version}</b> is available.</span>
      <button class="primary" onclick={installUpdate} disabled={updating}>
        {updating ? "Installing…" : "Install & Restart"}
      </button>
      <button onclick={() => (update = null)} disabled={updating}>Later</button>
    </div>
  {/if}

  {#if loaded}
    <main>
      {#if tab === "main"}
        <MainTab settings={appSettings} />
      {:else}
        <SettingsTab
          bind:settings={appSettings}
          {version}
          {updateStatus}
          onCheckUpdates={() => checkUpdates(true)}
        />
      {/if}
    </main>
  {:else}
    <p class="loading">Loading…</p>
  {/if}
</div>

<style>
  /* Follow the OS light/dark preference. `color-scheme` makes native controls
     (select, checkbox, inputs, scrollbars) theme themselves; the variables
     drive our own surfaces (used here and in the dialogs via var(--bg)). */
  :global(:root) {
    color-scheme: light dark;
    --bg: #ffffff;
    --fg: #1a1a1a;
    --muted: #777;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg: #1e1e1e;
      --fg: #e8e8e8;
      --muted: #9a9a9a;
    }
  }
  /* Explicit override wins over the media query (higher specificity). */
  :global(:root[data-theme="light"]) {
    --bg: #ffffff;
    --fg: #1a1a1a;
    --muted: #777;
  }
  :global(:root[data-theme="dark"]) {
    --bg: #1e1e1e;
    --fg: #e8e8e8;
    --muted: #9a9a9a;
  }
  :global(body) {
    margin: 0;
    background: var(--bg);
    color: var(--fg);
  }
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    padding: 16px 20px;
    box-sizing: border-box;
    font-family: Inter, system-ui, Avenir, Helvetica, Arial, sans-serif;
  }
  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 12px;
  }
  h1 {
    font-size: 1.3em;
    margin: 0;
  }
  nav {
    display: flex;
    gap: 6px;
  }
  nav button {
    border: none;
    background: transparent;
    padding: 6px 14px;
    border-radius: 6px;
    cursor: pointer;
  }
  nav button.active {
    background: rgba(128, 128, 128, 0.18);
    font-weight: 600;
  }
  .warn {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    background: #fdecea;
    color: #922;
    padding: 8px 12px;
    border-radius: 6px;
    margin-bottom: 10px;
    font-size: 0.9em;
  }
  .warn span {
    flex: 1 1 auto;
  }
  .warn button {
    flex: 0 0 auto;
  }
  .warn-err {
    flex-basis: 100%;
    opacity: 0.85;
  }
  .update {
    display: flex;
    align-items: center;
    gap: 10px;
    background: rgba(59, 130, 246, 0.12);
    padding: 8px 12px;
    border-radius: 6px;
    margin-bottom: 10px;
    font-size: 0.9em;
  }
  .update span {
    flex: 1 1 auto;
  }
  main {
    flex: 1;
    overflow-y: auto;
  }
  .loading {
    opacity: 0.6;
  }
  :global(button.primary) {
    background: #3b82f6;
    color: #fff;
    border: none;
    padding: 7px 16px;
    border-radius: 6px;
    cursor: pointer;
    font-weight: 600;
  }
  :global(button) {
    padding: 6px 12px;
    border-radius: 6px;
    border: 1px solid rgba(128, 128, 128, 0.4);
    background: transparent;
    color: inherit;
    cursor: pointer;
  }
  :global(button:disabled) {
    opacity: 0.5;
    cursor: not-allowed;
  }
  /* Normalize form controls so inputs and dropdowns share one height/border. */
  :global(input),
  :global(select),
  :global(textarea) {
    box-sizing: border-box;
    height: 34px;
    padding: 0 10px;
    border-radius: 6px;
    border: 1px solid rgba(128, 128, 128, 0.4);
    background: var(--bg);
    color: inherit;
    font: inherit;
    font-size: 0.95em;
  }
  :global(textarea) {
    height: auto;
    padding: 8px 10px;
  }
  :global(input[type="checkbox"]) {
    height: auto;
    width: auto;
    padding: 0;
  }
  @media (prefers-color-scheme: dark) {
    .warn {
      background: #3a1f1d;
      color: #f3b0ab;
    }
  }
  :global(:root[data-theme="dark"]) .warn {
    background: #3a1f1d;
    color: #f3b0ab;
  }
  :global(:root[data-theme="light"]) .warn {
    background: #fdecea;
    color: #922;
  }
</style>
