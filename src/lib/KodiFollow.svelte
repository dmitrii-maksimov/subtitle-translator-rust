<script lang="ts">
  import { api } from "./tauri";
  import { fileProgress, logLines, running, resetJob, appendLog } from "./stores";

  let { onClose }: { onClose: () => void } = $props();

  async function start() {
    resetJob();
    running.set(true);
    appendLog("Following Kodi playback…");
    try {
      await api.startKodiFollow();
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
</script>

<div class="backdrop">
  <div class="dialog">
    <h3>Follow Kodi playback</h3>
    <p class="hint">
      Watches the active Kodi player and keeps a translated subtitle track
      running ahead of playback (by the "Follow buffer" minutes), pushing it to
      Kodi automatically. Configure the connection and path mapping above first.
    </p>

    <div class="progress"><progress max="100" value={$fileProgress}></progress></div>
    <div class="log">
      {#each $logLines as line}<div>{line}</div>{/each}
    </div>

    <div class="btns">
      <button onclick={onClose} disabled={$running}>Close</button>
      <span class="spacer"></span>
      {#if $running}
        <button class="danger" onclick={stop}>Stop</button>
      {:else}
        <button class="primary" onclick={start}>Start following</button>
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
  .progress progress {
    width: 100%;
    height: 12px;
  }
  .log {
    margin-top: 8px;
    height: 220px;
    overflow-y: auto;
    background: rgba(128, 128, 128, 0.1);
    border-radius: 6px;
    padding: 8px 10px;
    font-family: monospace;
    font-size: 0.82em;
    white-space: pre-wrap;
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
