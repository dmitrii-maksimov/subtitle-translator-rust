<script lang="ts">
  import { api } from "./tauri";
  import type { AppSettings, ModelInfo } from "./types";

  let { settings = $bindable() }: { settings: AppSettings } = $props();

  let models = $state<ModelInfo[]>(
    (settings.cached_models ?? []).map((id) => ({ id, price: null, is_chat: true })),
  );
  let loading = $state(false);
  let error = $state("");

  async function refresh() {
    loading = true;
    error = "";
    try {
      const fetched = await api.listModels();
      models = fetched.filter((m) => m.is_chat);
      settings.cached_models = models.map((m) => m.id);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function label(m: ModelInfo): string {
    return m.price ? `${m.id}  —  ${m.price}` : m.id;
  }
</script>

<div class="model-picker">
  <label class="row">
    <span class="lbl">Model</span>
    {#if settings.use_custom_model}
      <input
        type="text"
        bind:value={settings.model}
        placeholder="custom model id (for local proxies / unlisted models)"
      />
    {:else}
      <select bind:value={settings.model}>
        {#if !models.some((m) => m.id === settings.model)}
          <option value={settings.model}>{settings.model} (current)</option>
        {/if}
        {#each models as m (m.id)}
          <option value={m.id}>{label(m)}</option>
        {/each}
      </select>
    {/if}
  </label>

  <div class="controls">
    <label class="custom">
      <input type="checkbox" bind:checked={settings.use_custom_model} />
      Custom
    </label>
    <button onclick={refresh} disabled={loading}>
      {loading ? "Refreshing…" : "Refresh"}
    </button>
  </div>

  {#if error}
    <p class="err">{error}</p>
  {/if}
</div>

<style>
  .model-picker {
    display: flex;
    flex-direction: column;
    gap: 6px;
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
  select,
  input[type="text"] {
    flex: 1 1 auto;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-left: 130px;
  }
  .custom {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .err {
    color: #c0392b;
    margin: 4px 0 0 130px;
    font-size: 0.9em;
  }
</style>
