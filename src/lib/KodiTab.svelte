<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { api } from "./tauri";
  import KodiFollow from "./KodiFollow.svelte";
  import NumberField from "./NumberField.svelte";
  import type { AppSettings, KodiEntry, KodiInstance } from "./types";

  let { settings = $bindable() }: { settings: AppSettings } = $props();

  let status = $state("● Disconnected");
  let statusOk = $state(false);
  let testing = $state(false);

  let discovering = $state(false);
  let found = $state<KodiInstance[]>([]);

  let preview = $state("");
  let showFollow = $state(false);

  // Browse modal state.
  let browseOpen = $state(false);
  let browsePath = $state<string | null>(null); // null = sources root
  let browseStack = $state<string[]>([]);
  let browseEntries = $state<KodiEntry[]>([]);
  let browseError = $state("");
  let browseLoading = $state(false);

  async function test() {
    testing = true;
    status = "Connecting…";
    try {
      const r = await api.kodiPing(
        settings.kodi_host,
        settings.kodi_port,
        settings.kodi_user,
        settings.kodi_password,
      );
      statusOk = r.ok;
      status = (r.ok ? "● " : "● ") + r.message;
    } catch (e) {
      statusOk = false;
      status = "● " + String(e);
    } finally {
      testing = false;
    }
  }

  async function discover() {
    discovering = true;
    found = [];
    try {
      found = await api.kodiDiscover(settings.kodi_port || 8080);
      if (found.length === 0) status = "● No Kodi found on the network";
    } catch (e) {
      status = "● Discovery failed: " + String(e);
    } finally {
      discovering = false;
    }
  }

  function pickInstance(inst: KodiInstance) {
    settings.kodi_host = inst.ip;
    settings.kodi_port = inst.port;
    found = [];
    test();
  }

  async function refreshPreview() {
    try {
      preview = await api.kodiMapPreview(settings.local_parent_path, settings.kodi_source_path);
    } catch (e) {
      preview = String(e);
    }
  }
  // Keep the preview in sync with the two path fields.
  $effect(() => {
    void settings.local_parent_path;
    void settings.kodi_source_path;
    refreshPreview();
  });

  async function pickLocalParent() {
    const dir = await open({ directory: true });
    if (dir && !Array.isArray(dir)) settings.local_parent_path = dir;
  }

  // ---- Kodi folder browser ----
  async function openBrowse() {
    browseOpen = true;
    browsePath = null;
    browseStack = [];
    await loadBrowse();
  }
  async function loadBrowse() {
    browseLoading = true;
    browseError = "";
    try {
      browseEntries = await api.kodiBrowse(
        settings.kodi_host,
        settings.kodi_port,
        settings.kodi_user,
        settings.kodi_password,
        browsePath,
      );
    } catch (e) {
      browseError = String(e);
      browseEntries = [];
    } finally {
      browseLoading = false;
    }
  }
  async function enter(entry: KodiEntry) {
    if (!entry.is_dir) return;
    if (browsePath !== null) browseStack = [...browseStack, browsePath];
    browsePath = entry.file;
    await loadBrowse();
  }
  async function browseBack() {
    if (browseStack.length > 0) {
      browsePath = browseStack[browseStack.length - 1];
      browseStack = browseStack.slice(0, -1);
    } else {
      browsePath = null;
    }
    await loadBrowse();
  }
  function selectCurrent() {
    if (browsePath) settings.kodi_source_path = browsePath;
    browseOpen = false;
  }
</script>

<div class="kodi">
  <div class="section">
    <div class="section-title">Kodi connection</div>
    <label class="field-row"><span class="field-label colon">Host</span>
      <input type="text" bind:value={settings.kodi_host} placeholder="192.168.1.50" /></label>
    <label class="field-row"><span class="field-label colon">Port</span>
      <NumberField bind:value={settings.kodi_port} min={1} /></label>
    <label class="field-row"><span class="field-label colon">User</span>
      <input type="text" bind:value={settings.kodi_user} /></label>
    <label class="field-row"><span class="field-label colon">Password</span>
      <input type="password" bind:value={settings.kodi_password} /></label>
    <div class="field-row">
      <span class="field-label"></span>
      <button onclick={discover} disabled={discovering}>
        {discovering ? "Searching…" : "Find Kodi on network"}
      </button>
      <button onclick={test} disabled={testing}>Test connection</button>
    </div>
    <div class="field-row">
      <span class="field-label colon">Status</span>
      <span class="status" class:ok={statusOk}>{status}</span>
    </div>
    {#if found.length}
      <div class="field-row">
        <span class="field-label"></span>
        <div class="found">
          {#each found as inst (inst.ip + inst.port)}
            <button class="found-item" onclick={() => pickInstance(inst)}>
              {inst.name} <span class="muted">({inst.ip}:{inst.port} · {inst.source})</span>
            </button>
          {/each}
        </div>
      </div>
    {/if}
  </div>

  <div class="section">
    <div class="section-title">Path mapping</div>
    <label class="field-row"><span class="field-label colon">Kodi source</span>
      <input type="text" bind:value={settings.kodi_source_path} placeholder="smb://nas/movies" />
      <button onclick={openBrowse}>Pick Kodi folder</button></label>
    <label class="field-row"><span class="field-label colon">Local parent</span>
      <input type="text" bind:value={settings.local_parent_path} placeholder="/Volumes/movies" />
      <button onclick={pickLocalParent}>Pick local folder</button></label>
    <div class="field-row preview-row">
      <span class="field-label colon">Mapping preview</span>
      <pre class="preview">{preview}</pre>
    </div>
  </div>

  <div class="section">
    <div class="section-title">Live mode</div>
    <label class="field-row"><span class="field-label colon">Poll interval (s)</span>
      <NumberField bind:value={settings.live_poll_interval} min={1} /></label>
    <label class="field-row"><span class="field-label colon">Stable finish (s)</span>
      <NumberField bind:value={settings.live_stable_threshold} min={1} /></label>
    <label class="field-row"><span class="field-label colon">Follow buffer (min)</span>
      <NumberField bind:value={settings.kodi_follow_buffer_min} min={1} /></label>
  </div>

  <div class="actions">
    <button onclick={() => (showFollow = true)}>Follow Kodi playback…</button>
  </div>
</div>

{#if showFollow}
  <KodiFollow onClose={() => (showFollow = false)} />
{/if}

{#if browseOpen}
  <div class="backdrop">
    <div class="dialog">
      <h3>Browse Kodi folders</h3>
      <p class="crumb">{browsePath ?? "Sources"}</p>
      <div class="list">
        {#if browseLoading}
          <div class="hint">Loading…</div>
        {:else if browseError}
          <div class="hint err">{browseError}</div>
        {:else}
          {#if browsePath !== null}
            <button class="entry" onclick={browseBack}>⬅ Back</button>
          {/if}
          {#each browseEntries as e (e.file)}
            <button class="entry" onclick={() => enter(e)} disabled={!e.is_dir}>
              {e.is_dir ? "📁" : "📄"} {e.label}
            </button>
          {/each}
          {#if browseEntries.length === 0}
            <div class="hint">Empty.</div>
          {/if}
        {/if}
      </div>
      <div class="dialog-btns">
        <button onclick={() => (browseOpen = false)}>Cancel</button>
        <button class="primary" disabled={!browsePath} onclick={selectCurrent}>
          Select this folder
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .kodi {
    display: flex;
    flex-direction: column;
    gap: 15px;
    width: 100%;
  }
  .status {
    color: #c0392b;
  }
  .status.ok {
    color: #27ae60;
  }
  .found {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1 1 auto;
  }
  .found-item {
    text-align: left;
  }
  .muted {
    color: var(--muted, #888);
  }
  .preview-row {
    align-items: flex-start;
  }
  .preview {
    flex: 1 1 auto;
    min-width: 0;
    margin: 0;
    padding: 3px 8px;
    background: rgba(128, 128, 128, 0.1);
    border-radius: 5px;
    white-space: pre-wrap;
    word-break: break-all;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }
  .dialog {
    background: var(--bg, #fff);
    border-radius: 10px;
    padding: 18px 20px;
    width: min(560px, 92vw);
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 10px 40px rgba(0, 0, 0, 0.35);
  }
  h3 {
    margin: 0 0 4px;
  }
  .crumb {
    margin: 0 0 8px;
    opacity: 0.7;
    font-size: 0.85em;
    word-break: break-all;
  }
  .list {
    flex: 1;
    overflow-y: auto;
    border-top: 1px solid rgba(128, 128, 128, 0.2);
    padding: 6px 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-height: 160px;
  }
  .entry {
    text-align: left;
    border: none;
    background: transparent;
    padding: 6px 8px;
    border-radius: 6px;
  }
  .entry:hover:not(:disabled) {
    background: rgba(128, 128, 128, 0.14);
  }
  .hint {
    opacity: 0.6;
    padding: 12px;
  }
  .hint.err {
    color: #c0392b;
  }
  .dialog-btns {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 12px;
  }
</style>
