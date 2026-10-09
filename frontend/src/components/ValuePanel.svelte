<script lang="ts">
  import type { Parameter } from '../lib/types';
  let { title, values, params = [] }: { title: string; values: Record<string, number>; params?: Parameter[] } = $props();
  let rows = $derived(params.length ? params.filter((p) => p.name in values).map((p) => ({ name: p.name, value: values[p.name], min: p.min, max: p.max }))
    : Object.entries(values).sort(([a], [b]) => a.localeCompare(b)).map(([name, value]) => ({ name, value, min: name.startsWith('Head') ? -180 : 0, max: name.startsWith('Head') ? 180 : 1 })));
  const fraction = (value: number, min: number, max: number) => max <= min ? 0 : Math.min(1, Math.max(0, (value - min) / (max - min)));
</script>
<section class="card value-panel">
  <h2>{title}</h2>
  <div class="scroll">
    {#each rows as row, index (index)}
      {@const fill = fraction(row.value, row.min, row.max)}
      {@const zero = fraction(0, row.min, row.max)}
      <div class="value-row">
        <div><span title={row.name}>{row.name}</span><output>{row.value.toFixed(2)}</output></div>
        <div class="meter"><span style:left="{Math.min(fill, zero) * 100}%" style:width="{Math.abs(fill - zero) * 100}%" class:negative={fill < zero}></span></div>
      </div>
    {/each}
  </div>
</section>
<style>
  .value-panel { display: flex; flex-direction: column; min-width: 0; padding: 8px; }
  h2 { height: 20px; color: #b07a7a; font-size: 10px; letter-spacing: .5px; }
  .scroll { overflow: auto; flex: 1; min-height: 0; }
  .value-row { padding: 4px 2px; }
  .value-row > div:first-child { display: flex; gap: 4px; font-size: 11px; margin-bottom: 4px; }
  .value-row span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #8888aa; }
  output { margin-left: auto; color: #c8c8e0; font-variant-numeric: tabular-nums; }
  .meter { height: 5px; border-radius: 3px; background: var(--bg-input); position: relative; }
  .meter span { position: absolute; height: 5px; background: #c2bcc2; border-radius: 3px; }
  .meter span.negative { background: #8c868e; }
</style>
