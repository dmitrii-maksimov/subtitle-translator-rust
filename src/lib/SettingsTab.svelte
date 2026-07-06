<script lang="ts">
  import ModelPicker from "./ModelPicker.svelte";
  import NumberField from "./NumberField.svelte";
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

  async function resetMainPrompt() {
    settings.main_prompt_template = (await api.defaultPrompts()).main_prompt_template;
  }
  async function resetSystemRole() {
    settings.system_role = (await api.defaultPrompts()).system_role;
  }
</script>

<div class="settings">
  <div class="section">
    <div class="section-title">General</div>
    <label class="field-row">
      <span class="field-label colon">Theme</span>
      <select bind:value={settings.theme}>
        <option value="system">System (default)</option>
        <option value="light">Light</option>
        <option value="dark">Dark</option>
      </select>
    </label>
    <label class="field-row">
      <span class="field-label colon">Target Language</span>
      <input type="text" bind:value={settings.target_language} placeholder="ru" />
    </label>
  </div>

  <div class="section">
    <div class="section-title">API</div>
    <label class="field-row">
      <span class="field-label colon">API Key</span>
      <input type="password" bind:value={settings.api_key} placeholder="sk-…" />
    </label>
    <label class="field-row">
      <span class="field-label colon">API Base URL</span>
      <input type="text" bind:value={settings.api_base} />
    </label>
    <ModelPicker bind:settings />
  </div>

  <div class="section">
    <div class="section-title">Translation</div>
    <label class="field-row">
      <span class="field-label colon">Workers</span>
      <NumberField bind:value={settings.workers} min={1} max={10} />
    </label>
    <label class="field-row">
      <span class="field-label colon">Window</span>
      <NumberField bind:value={settings.window} min={1} />
    </label>
    <label class="field-row">
      <span class="field-label colon">Overlap</span>
      <NumberField bind:value={settings.overlap} min={0} />
    </label>
    <label class="field-row">
      <span class="field-label colon">Extra instruction</span>
      <input type="text" bind:value={settings.extra_prompt} placeholder="e.g. keep it formal" />
    </label>
  </div>

  <div class="section">
    <div class="section-title">Prompt overrides</div>
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
  </div>

  <div class="section">
    <div class="section-title">Options</div>
    <label class="field-check">
      <input type="checkbox" bind:checked={settings.overwrite_original} />
      Overwrite the original file
    </label>
    <label class="field-check">
      <input type="checkbox" bind:checked={settings.fulllog} />
      Full request/response log (debug)
    </label>
    <label class="field-check">
      <input type="checkbox" bind:checked={settings.show_kodi} />
      Show Kodi integration
    </label>
  </div>

  <div class="section">
    <div class="section-title">Updates</div>
    <label class="field-check">
      <input type="checkbox" bind:checked={settings.auto_check_updates} />
      Automatically check for updates on startup
    </label>
    <div class="field-check">
      <button onclick={() => onCheckUpdates?.()}>Check for updates now</button>
      {#if updateStatus}<span class="update-status">{updateStatus}</span>{/if}
    </div>
    {#if version}
      <div class="version">Version {version}</div>
    {/if}
  </div>
</div>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    gap: 15px;
    width: 100%;
  }
  .col {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .col > span {
    color: var(--muted, #888);
  }
  textarea {
    width: 100%;
    font-family: monospace;
    resize: vertical;
  }
  textarea::-webkit-resizer {
    display: none;
  }
  .reset {
    align-self: flex-start;
  }
  .update-status {
    color: var(--muted, #888);
  }
  .version {
    text-align: left;
    color: var(--muted, #888);
    font-size: 0.95em;
  }
</style>
