<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { api } from "./tauri";
  import type { Stream } from "./types";
  import { fileProgress, logLines, running, resetJob, appendLog } from "./stores";

  let { onClose }: { onClose: () => void } = $props();

  let mkvPath = $state("");
  let streams = $state<Stream[]>([]);
  let selected = $state<number | null>(null);
  let probing = $state(false);
  let error = $state("");

  function base(p: string): string {
    return p.split(/[/\\]/).pop() ?? p;
  }

  async function pick() {
    const sel = await open({ multiple: false, filters: [{ name: "MKV", extensions: ["mkv"] }] });
    if (!sel || Array.isArray(sel)) return;
    mkvPath = sel;
    streams = [];
    selected = null;
    error = "";
    probing = true;
    try {
      streams = await api.probeSubsPartial(mkvPath);
      if (streams.length) selected = streams[0].index;
      if (streams.length === 0) error = "No subtitle tracks visible yet (file may be too small so far).";
    } catch (e) {
      error = String(e);
    } finally {
      probing = false;
    }
  }

  async function start() {
    if (selected === null) return;
    resetJob();
    running.set(true);
    appendLog(`Live mode on ${base(mkvPath)}, track #${selected}`);
    try {
      await api.startLive(mkvPath, selected);
    } catch (e) {
      appendLog(`Error: ${e}`);
    } finally {
      running.set(false);
    }
  }

  async function stop() {
    await api.cancelJob();
    appendLog("Stop requested…");
  }

  function streamLabel(s: Stream): string {
    const lang = s.tags?.language ?? "und";
    const title = s.tags?.title ? ` “${s.tags.title}”` : "";
    return `#${s.index} · ${lang} · ${s.codec_name}${title}`;
  }
</script>

<div class="backdrop">
  <div class="dialog">
    <h3>Live mode — still-downloading file</h3>
    <p class="hint">
      Translate subtitles from an MKV that is still downloading (sequential
      download). New full windows are translated as they arrive; the tail is
      finished once the file stops growing.
    </p>

    <div class="pick">
      <button onclick={pick} disabled={$running}>Pick MKV…</button>
      <span class="path">{mkvPath ? base(mkvPath) : "no file selected"}</span>
    </div>

    {#if probing}
      <p class="hint">Probing…</p>
    {/if}
    {#if error}
      <p class="err">{error}</p>
    {/if}

    {#if streams.length}
      <label class="row">
        <span class="lbl">Source track</span>
        <select bind:value={selected} disabled={$running}>
          {#each streams as s (s.index)}
            <option value={s.index}>{streamLabel(s)}</option>
          {/each}
        </select>
      </label>
    {/if}

    <div class="progress">
      <progress max="100" value={$fileProgress}></progress>
    </div>
    <div class="log">
      {#each $logLines as line}<div>{line}</div>{/each}
    </div>

    <div class="btns">
      <button onclick={onClose} disabled={$running}>Close</button>
      <span class="spacer"></span>
      {#if $running}
        <button class="danger" onclick={stop}>Stop</button>
      {:else}
        <button class="primary" disabled={selected === null} onclick={start}>Start</button>
      {/if}
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
    border-radius: 10px;
    padding: 20px 22px;
    width: min(640px, 94vw);
    max-height: 88vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 10px 40px rgba(0, 0, 0, 0.35);
  }
  h3 {
    margin: 0 0 6px;
  }
  .hint {
    opacity: 0.7;
    font-size: 0.88em;
    margin: 0 0 10px;
  }
  .pick {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 10px;
  }
  .path {
    opacity: 0.7;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 10px;
  }
  .lbl {
    width: 110px;
    flex: 0 0 auto;
  }
  .row select {
    flex: 1 1 auto;
    min-width: 0;
  }
  .progress progress {
    width: 100%;
    height: 12px;
  }
  .log {
    margin-top: 8px;
    height: 200px;
    overflow-y: auto;
    background: rgba(128, 128, 128, 0.1);
    border-radius: 6px;
    padding: 8px 10px;
    font-family: monospace;
    font-size: 0.82em;
    white-space: pre-wrap;
  }
  .err {
    color: #c0392b;
  }
  .btns {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 12px;
  }
  .spacer {
    flex: 1;
  }
</style>
