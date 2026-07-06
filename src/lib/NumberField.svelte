<script lang="ts">
  let {
    value = $bindable(),
    min,
    max,
  }: { value: number; min?: number; max?: number } = $props();

  function step(delta: number) {
    let v = (Number(value) || 0) + delta;
    if (min !== undefined) v = Math.max(min, v);
    if (max !== undefined) v = Math.min(max, v);
    value = v;
  }
</script>

<div class="num">
  <input type="number" bind:value {min} {max} />
  <div class="steps">
    <button type="button" tabindex="-1" aria-label="increase" onclick={() => step(1)}>▲</button>
    <button type="button" tabindex="-1" aria-label="decrease" onclick={() => step(-1)}>▼</button>
  </div>
</div>

<style>
  .num {
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
  }
  .num input {
    flex: 1 1 auto;
    width: 100%;
    padding-right: 20px;
  }
  /* Hide the native (ugly) spinner. */
  .num input::-webkit-inner-spin-button,
  .num input::-webkit-outer-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
  .steps {
    position: absolute;
    right: 1px;
    top: 1px;
    bottom: 1px;
    width: 17px;
    display: flex;
    flex-direction: column;
    border-left: 1px solid rgba(128, 128, 128, 0.3);
  }
  .steps button {
    flex: 1;
    border: none;
    background: transparent;
    color: var(--muted, #888);
    font-size: 6px;
    line-height: 1;
    cursor: pointer;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .steps button:first-child {
    border-radius: 0 4px 0 0;
  }
  .steps button:last-child {
    border-radius: 0 0 4px 0;
  }
  .steps button:hover {
    color: var(--fg, #111);
    background: rgba(128, 128, 128, 0.18);
  }
</style>
