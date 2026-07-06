<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "./tauri";
  import type { AppSettings, ModelInfo } from "./types";

  let { settings = $bindable() }: { settings: AppSettings } = $props();

  let models = $state<ModelInfo[]>([]);
  let open = $state(false);
  let loading = $state(false);
  let error = $state("");

  // Restore the cached list on startup and re-attach prices from the local
  // pricing table (the API never returns prices).
  onMount(async () => {
    const ids = settings.cached_models ?? [];
    if (ids.length) {
      try {
        models = await api.modelsInfo(ids);
      } catch {
        models = ids.map((id) => ({ id, price: null, is_chat: true }));
      }
    }
  });

  async function refresh() {
    loading = true;
    error = "";
    try {
      const fetched = await api.listModels();
      models = fetched.filter((m) => m.is_chat);
      settings.cached_models = models.map((m) => m.id);
      // Persist immediately so the list + selection survive a restart without
      // requiring a manual "Save settings".
      await api.saveSettings($state.snapshot(settings));
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  const currentPrice = $derived(
    models.find((m) => m.id === settings.model)?.price ?? null,
  );

  function select(id: string) {
    settings.model = id;
    open = false;
  }
</script>

<div class="model-picker">
  <div class="field-row">
    <span class="field-label colon">Model</span>

    {#if settings.use_custom_model}
      <input
        class="grow"
        type="text"
        bind:value={settings.model}
        placeholder="custom model id, e.g. gpt-4o-mini"
      />
    {:else}
      <div class="combo-wrap">
        <button type="button" class="combo" onclick={() => (open = !open)}>
          <span class="combo-id">{settings.model || "Select a model"}</span>
          {#if currentPrice}<span class="combo-price">{currentPrice}</span>{/if}
          <span class="chevron">▾</span>
        </button>

        {#if open}
          <button class="backdrop" aria-label="Close" onclick={() => (open = false)}
          ></button>
          <div class="popup" role="listbox">
            {#if models.length === 0}
              <div class="empty">No models cached — press Refresh.</div>
            {/if}
            {#each models as m (m.id)}
              <button
                type="button"
                class="opt"
                class:selected={m.id === settings.model}
                role="option"
                aria-selected={m.id === settings.model}
                onclick={() => select(m.id)}
              >
                <span class="opt-id">{m.id}</span>
                <span class="opt-price">{m.price ?? ""}</span>
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <button class="refresh" onclick={refresh} disabled={loading}>
        {loading ? "Refreshing…" : "Refresh"}
      </button>
    {/if}

    <label class="custom">
      <input type="checkbox" bind:checked={settings.use_custom_model} />
      Custom
    </label>
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
  .grow {
    flex: 1 1 auto;
    min-width: 0;
  }
  .refresh {
    flex: 0 0 auto;
  }
  .custom {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .combo-wrap {
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
  }
  .combo {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 22px;
    padding: 0 10px;
    text-align: left;
    cursor: pointer;
  }
  .combo-id {
    flex: 1 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .combo-price {
    color: var(--muted, #888);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .chevron {
    opacity: 0.6;
    flex: 0 0 auto;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    background: transparent;
    border: none;
    padding: 0;
    z-index: 40;
  }
  .popup {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    max-height: 320px;
    overflow-y: auto;
    background: var(--bg, #fff);
    border: 1px solid rgba(128, 128, 128, 0.4);
    border-radius: 8px;
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.28);
    z-index: 50;
    padding: 4px;
  }
  .opt {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 16px;
    width: 100%;
    height: auto;
    border: none;
    background: transparent;
    border-radius: 6px;
    padding: 6px 10px;
    cursor: pointer;
    text-align: left;
  }
  .opt:hover {
    background: rgba(128, 128, 128, 0.14);
  }
  .opt.selected {
    background: rgba(59, 130, 246, 0.18);
  }
  .opt-id {
    flex: 1 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .opt-price {
    flex: 0 0 auto;
    color: var(--muted, #888);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .empty {
    padding: 10px;
    opacity: 0.6;
  }
  .err {
    color: #c0392b;
    margin: 4px 0 0 148px;
    font-size: 0.9em;
  }
</style>
