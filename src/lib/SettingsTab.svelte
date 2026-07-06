<script lang="ts">
  import ModelPicker from "./ModelPicker.svelte";
  import type { AppSettings } from "./types";
  import { api } from "./tauri";

  let {
    settings = $bindable(),
    version = "",
    updateStatus = "",
    onCheckUpdates,
  }: {
    settings: AppSettings;
    version?: string;
    updateStatus?: string;
    onCheckUpdates?: () => void;
  } = $props();

  let savedFlash = $state(false);

  async function save() {
    await api.saveSettings($state.snapshot(settings));
    savedFlash = true;
    setTimeout(() => (savedFlash = false), 1200);
  }

  async function resetMainPrompt() {
    settings.main_prompt_template = (await api.defaultPrompts()).main_prompt_template;
  }
  async function resetSystemRole() {
    settings.system_role = (await api.defaultPrompts()).system_role;
  }
</script>

<div class="settings">
  <label class="row">
    <span class="lbl">Theme</span>
    <select bind:value={settings.theme}>
      <option value="system">System (default)</option>
      <option value="light">Light</option>
      <option value="dark">Dark</option>
    </select>
  </label>

  <label class="row">
    <span class="lbl">API Key</span>
    <input type="password" bind:value={settings.api_key} placeholder="sk-…" />
  </label>

  <label class="row">
    <span class="lbl">API Base URL</span>
    <input type="text" bind:value={settings.api_base} />
  </label>

  <ModelPicker bind:settings />

  <label class="row">
    <span class="lbl">Target Language</span>
    <input type="text" bind:value={settings.target_language} placeholder="ru" />
  </label>

  <label class="row">
    <span class="lbl">Workers</span>
    <input type="number" min="1" max="10" bind:value={settings.workers} />
  </label>
  <label class="row">
    <span class="lbl">Window</span>
    <input type="number" min="1" bind:value={settings.window} />
  </label>
  <label class="row">
    <span class="lbl">Overlap</span>
    <input type="number" min="0" bind:value={settings.overlap} />
  </label>

  <label class="check">
    <input type="checkbox" bind:checked={settings.overwrite_original} />
    Overwrite the original file
  </label>
  <label class="check">
    <input type="checkbox" bind:checked={settings.fulllog} />
    Full request/response log (debug)
  </label>

  <label class="row">
    <span class="lbl">Extra instruction</span>
    <input
      type="text"
      bind:value={settings.extra_prompt}
      placeholder="e.g. keep it formal"
    />
  </label>

  <details>
    <summary>Advanced prompt overrides</summary>
    <label class="col">
      <span>Main prompt template (use {"{header}"}, {"{extra}"}, {"{src_block}"})</span>
      <textarea rows="6" bind:value={settings.main_prompt_template}></textarea>
    </label>
    <button class="reset" onclick={resetMainPrompt}>Reset main prompt to default</button>

    <label class="col">
      <span>System role (chat system message)</span>
      <textarea rows="2" bind:value={settings.system_role}></textarea>
    </label>
    <button class="reset" onclick={resetSystemRole}>Reset system role to default</button>
  </details>

  <div class="actions">
    <button class="primary" onclick={save}>Save settings</button>
    {#if savedFlash}<span class="flash">Saved ✓</span>{/if}
  </div>

  <hr />

  <label class="check">
    <input type="checkbox" bind:checked={settings.show_kodi} />
    Show Kodi integration
  </label>
  <label class="check">
    <input type="checkbox" bind:checked={settings.auto_check_updates} />
    Automatically check for updates on startup
  </label>
  <div class="actions">
    <button onclick={() => onCheckUpdates?.()}>Check for updates now</button>
    {#if updateStatus}<span class="update-status">{updateStatus}</span>{/if}
  </div>

  {#if version}
    <p class="version">Version {version}</p>
  {/if}
</div>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    gap: 12px;
    max-width: 720px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .lbl {
    width: 120px;
    flex: 0 0 auto;
  }
  .row input,
  .row select {
    flex: 1 1 auto;
    min-width: 0;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .col {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 8px;
  }
  textarea {
    width: 100%;
    font-family: monospace;
  }
  .reset {
    align-self: flex-start;
    margin-top: 6px;
    font-size: 0.85em;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 8px;
  }
  .flash {
    color: #27ae60;
  }
  .version {
    margin-top: 4px;
    color: var(--muted, #888);
    font-size: 0.85em;
  }
  hr {
    width: 100%;
    border: none;
    border-top: 1px solid rgba(128, 128, 128, 0.25);
    margin: 6px 0;
  }
  .update-status {
    color: var(--muted, #888);
    font-size: 0.9em;
  }
</style>
