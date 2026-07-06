<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { api, subscribeProgress } from "$lib/tauri";
  import { fileProgress, batchProgress, batchLabel, appendLog } from "$lib/stores";
  import MainTab from "$lib/MainTab.svelte";
  import SettingsTab from "$lib/SettingsTab.svelte";
  import KodiTab from "$lib/KodiTab.svelte";
  import { emptySettings } from "$lib/types";
  import { checkForUpdate, applyUpdate, type UpdateAvailable } from "$lib/updater";
  import { marked } from "marked";

  let version = $state("");

  let tab = $state<"main" | "settings" | "kodi">("main");
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

  // Auto-save settings whenever anything changes (debounced), so toggles like
  // "Show Kodi integration" persist without an explicit Save.
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const snap = $state.snapshot(appSettings); // deep-tracks all fields
    if (!loaded) return;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      api.saveSettings(snap);
    }, 400);
  });

  const notesHtml = $derived(
    update?.notes ? (marked.parse(update.notes, { async: false }) as string) : "",
  );
</script>

<svelte:head>
  <title>Subtitle Translator</title>
</svelte:head>

<div class="app">
  <header>
    <nav>
      <button class:active={tab === "main"} onclick={() => (tab = "main")}>Main</button>
      {#if appSettings.show_kodi}
        <button class:active={tab === "kodi"} onclick={() => (tab = "kodi")}>Kodi</button>
      {/if}
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
      <div class="update-head">
        <span>⬆ Update <b>{update.version}</b> is available.</span>
        <span class="spacer"></span>
        <button class="primary" onclick={installUpdate} disabled={updating}>
          {updating ? "Installing…" : "Install & Restart"}
        </button>
        <button onclick={() => (update = null)} disabled={updating}>Later</button>
      </div>
      {#if notesHtml}
        <!-- Release notes come from our own GitHub release body. -->
        <div class="update-notes">{@html notesHtml}</div>
      {/if}
    </div>
  {/if}

  {#if loaded}
    <main>
      {#if tab === "main"}
        <MainTab settings={appSettings} />
      {:else if tab === "kodi"}
        <KodiTab bind:settings={appSettings} />
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
    font-family: Inter, system-ui, Avenir, Helvetica, Arial, sans-serif;
    font-size: 12.5px;
    font-weight: 400;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
  }
  /* Subtle, rounded, theme-neutral scrollbars. */
  :global(*::-webkit-scrollbar) {
    width: 12px;
    height: 12px;
  }
  :global(*::-webkit-scrollbar-track) {
    background: transparent;
  }
  :global(*::-webkit-scrollbar-corner) {
    background: transparent;
  }
  :global(*::-webkit-scrollbar-thumb) {
    background: rgba(128, 128, 128, 0.35);
    border-radius: 8px;
    border: 3px solid transparent;
    background-clip: padding-box;
  }
  :global(*::-webkit-scrollbar-thumb:hover) {
    background: rgba(128, 128, 128, 0.6);
    background-clip: padding-box;
  }
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    padding: 14px 18px;
    box-sizing: border-box;
  }
  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 12px;
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
    flex-direction: column;
    gap: 8px;
    background: rgba(59, 130, 246, 0.12);
    padding: 8px 12px;
    border-radius: 6px;
    margin-bottom: 10px;
    font-size: 0.9em;
  }
  .update-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .update-head .spacer {
    flex: 1 1 auto;
  }
  .update-notes {
    margin: 0;
    max-height: 200px;
    overflow-y: auto;
    background: rgba(128, 128, 128, 0.12);
    border-radius: 6px;
    padding: 6px 12px;
    font-size: 0.95em;
    line-height: 1.4;
  }
  .update-notes :global(h1),
  .update-notes :global(h2),
  .update-notes :global(h3) {
    font-size: 1em;
    font-weight: 600;
    margin: 8px 0 4px;
  }
  .update-notes :global(ul) {
    margin: 4px 0;
    padding-left: 20px;
  }
  .update-notes :global(li) {
    margin: 2px 0;
  }
  .update-notes :global(p) {
    margin: 4px 0;
  }
  .update-notes :global(code) {
    background: rgba(128, 128, 128, 0.2);
    padding: 0 4px;
    border-radius: 4px;
  }
  main {
    flex: 1;
    overflow-y: auto;
    /* Extend to the window's right edge so the scrollbar sits there, while
       keeping content off the scrollbar via padding. */
    margin-right: -18px;
    padding-right: 16px;
    padding-top: 8px;
  }
  .loading {
    opacity: 0.6;
  }
  :global(button.primary) {
    background: #3b82f6;
    color: #fff;
    border: none;
    padding: 6px 15px;
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
  /* ---- Shared, reusable form system (one font size + spacing everywhere) ---- */
  :global(.section) {
    position: relative;
    border: 1px solid rgba(128, 128, 128, 0.3);
    border-radius: 8px;
    padding: 10px 12px;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  /* Title floats onto the top border (classic fieldset look); equal top/bottom
     inner padding. */
  :global(.section-title) {
    position: absolute;
    top: -0.72em;
    left: 10px;
    padding: 0 5px;
    background: var(--bg);
    font-weight: 600;
    color: inherit;
  }
  :global(.field-row) {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  :global(.field-label) {
    width: 140px;
    flex: 0 0 auto;
    text-align: right;
    color: var(--muted, #888);
  }
  :global(.field-label.colon::after) {
    content: ":";
  }
  :global(.field-row > input),
  :global(.field-row > select) {
    flex: 1 1 auto;
    min-width: 0;
  }
  :global(.field-check) {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  /* Normalize form controls so inputs and dropdowns share one height/border. */
  :global(input),
  :global(select),
  :global(textarea) {
    box-sizing: border-box;
    height: 22px;
    padding: 0 7px;
    border-radius: 5px;
    border: 1px solid rgba(128, 128, 128, 0.4);
    background: var(--bg);
    color: inherit;
    font: inherit;
    font-weight: 400;
  }
  /* Inline buttons sitting in a field row match the input height + text font. */
  :global(.field-row button) {
    height: 22px;
    padding: 0 10px;
    flex: 0 0 auto;
    font: inherit;
  }
  /* Custom dropdown arrow, detached from the right edge. */
  :global(select) {
    appearance: none;
    -webkit-appearance: none;
    padding-right: 26px;
    background-image: url('data:image/svg+xml;utf8,<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 10 10"><path d="M2 3.5 L5 6.5 L8 3.5" fill="none" stroke="%23888" stroke-width="1.4"/></svg>');
    background-repeat: no-repeat;
    background-position: right 9px center;
    background-size: 10px;
  }
  :global(textarea) {
    height: auto;
    padding: 6px 7px;
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
