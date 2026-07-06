<script lang="ts">
  import type { FileDecision, Stream, TrackPref } from "./types";

  let {
    filePath,
    streams,
    initialState,
    isLast,
    onSave,
    onSkip,
    onCancel,
  }: {
    filePath: string;
    streams: Stream[];
    initialState: Record<number, TrackPref>;
    isLast: boolean;
    onSave: (decision: FileDecision, prefs: Record<number, TrackPref>) => void;
    onSkip: () => void;
    onCancel: () => void;
  } = $props();

  interface Row {
    index: number;
    lang: string;
    title: string;
    codec: string;
    flags: string;
    translate: boolean;
    delete: boolean;
  }

  const rows = $state<Row[]>(
    streams.map((s) => {
      const init = initialState[s.index] ?? { translate: false, delete: false };
      const disp = s.disposition ?? {};
      const flags: string[] = [];
      if (disp.default) flags.push("default");
      if (disp.forced) flags.push("forced");
      if (disp.hearing_impaired) flags.push("SDH");
      if (disp.visual_impaired) flags.push("VI");
      return {
        index: s.index,
        lang: s.tags?.language ?? "und",
        title: s.tags?.title ?? "",
        codec: s.codec_name || "?",
        flags: flags.join(" · "),
        translate: init.translate,
        delete: init.delete,
      };
    }),
  );

  // Radio-style: only one Translate checked at a time.
  function onTranslate(i: number) {
    if (rows[i].translate) {
      rows.forEach((r, j) => {
        if (j !== i) r.translate = false;
      });
    }
  }

  function base(p: string): string {
    return p.split(/[/\\]/).pop() ?? p;
  }

  function collectPrefs(): Record<number, TrackPref> {
    const prefs: Record<number, TrackPref> = {};
    for (const r of rows) prefs[r.index] = { translate: r.translate, delete: r.delete };
    return prefs;
  }

  function save() {
    const translateRow = rows.find((r) => r.translate);
    const decision: FileDecision = {
      filePath,
      translateStreamIndex: translateRow ? translateRow.index : null,
      deleteStreamIndexes: rows.filter((r) => r.delete).map((r) => r.index),
    };
    onSave(decision, collectPrefs());
  }
</script>

<div class="backdrop">
  <div class="dialog">
    <h2>{base(filePath)}</h2>
    <p class="subtitle">
      {rows.length} subtitle track(s) — choose which to translate and/or delete.
    </p>

    {#if rows.length === 0}
      <p class="empty">No subtitle tracks found in this file.</p>
    {:else}
      <div class="table">
        <div class="thead">
          <span class="c-stream">STREAM</span>
          <span class="c-title">TITLE / FLAGS</span>
          <span class="c-chk">TRANSLATE</span>
          <span class="c-chk">DELETE</span>
        </div>
        {#each rows as row, i (row.index)}
          <div class="trow" class:alt={i % 2 === 0}>
            <span class="c-stream mono">#{row.index} {row.lang} · {row.codec}</span>
            <span class="c-title">
              {#if row.title}“{row.title}”{/if}
              {#if row.flags}<span class="flags">[{row.flags}]</span>{/if}
              {#if !row.title && !row.flags}—{/if}
            </span>
            <span class="c-chk">
              <input type="checkbox" bind:checked={row.translate} onchange={() => onTranslate(i)} />
            </span>
            <span class="c-chk">
              <input type="checkbox" bind:checked={row.delete} />
            </span>
          </div>
        {/each}
      </div>
    {/if}

    <div class="buttons">
      <button onclick={onCancel} title="Abort the whole batch.">Cancel</button>
      <button onclick={onSkip} title="Skip this file only.">Skip</button>
      <span class="spacer"></span>
      <button class="primary" disabled={rows.length === 0} onclick={save}>
        {isLast ? "Save & Remux" : "Save & Continue"}
      </button>
    </div>
  </div>
</div>

<style>
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
    color: inherit;
    border-radius: 10px;
    padding: 22px 24px;
    width: min(780px, 92vw);
    max-height: 88vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 10px 40px rgba(0, 0, 0, 0.35);
  }
  h2 {
    margin: 0 0 4px;
    font-size: 1.2em;
    word-break: break-all;
  }
  .subtitle {
    margin: 0 0 12px;
    opacity: 0.65;
    font-size: 0.9em;
  }
  .empty {
    text-align: center;
    padding: 32px;
    opacity: 0.6;
  }
  .table {
    overflow-y: auto;
    border-top: 1px solid rgba(128, 128, 128, 0.25);
  }
  .thead,
  .trow {
    display: grid;
    grid-template-columns: 200px 1fr 100px 80px;
    align-items: center;
    padding: 6px 8px;
  }
  .thead {
    font-size: 0.72em;
    letter-spacing: 1px;
    opacity: 0.6;
    position: sticky;
    top: 0;
    background: var(--bg, #fff);
  }
  .trow.alt {
    background: rgba(128, 128, 128, 0.08);
    border-radius: 6px;
  }
  .c-chk {
    text-align: center;
  }
  .mono {
    font-family: monospace;
    font-size: 0.9em;
  }
  .flags {
    opacity: 0.6;
    margin-left: 4px;
  }
  .buttons {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 16px;
  }
  .spacer {
    flex: 1;
  }
  .primary {
    font-weight: 600;
  }
</style>
