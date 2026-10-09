<script lang="ts">
  import type { Parameter, Preset } from '../lib/types';
  let { preset = $bindable(), errors, disabled, dirty, onchange, onsave, onvariables }: {
    preset: Preset; errors: string[]; disabled: boolean; dirty: boolean;
    onchange: () => void; onsave: () => void; onvariables: () => void;
  } = $props();
  function add(delay: boolean) {
    let n = preset.params.length + 1;
    while (preset.params.some((p) => p.name === `Parameter${n}`)) n++;
    const parameter: Parameter = { name: `Parameter${n}`, func: delay ? '' : 'JawOpen', min: 0, max: 1, defaultValue: 0 };
    if (delay) parameter.delayBuffer = { refParam: preset.params[0]?.name ?? 'JawOpen', smoothing: 1, delayCount: 1, inMin: 0, inMax: 1, outMin: 0, outMax: 1 };
    preset.params.push(parameter);
    onchange();
  }
  function remove(index: number) { preset.params.splice(index, 1); onchange(); }
</script>
<section class="card editor">
  <div class="editor-heading"><h2>PRESET EDITOR</h2><span class="muted">{dirty ? 'Unsaved changes' : 'Saved'}</span><button onclick={onvariables}>Variables</button><button class="primary" onclick={onsave} disabled={disabled}>Save preset</button></div>
  <fieldset {disabled}>
    <div class="metadata">
      <label>Title<input bind:value={preset.title} oninput={onchange} /></label>
      <label>Author<input bind:value={preset.author} oninput={onchange} /></label>
      <label>Description<input bind:value={preset.description} oninput={onchange} /></label>
    </div>
    <div class="param-head"><span>Name</span><span>Expression / reference</span><span>Min</span><span>Max</span><span>Default</span><span></span></div>
    <div class="param-list">
      {#each preset.params as parameter, index (index)}
        <div class="parameter" class:delay={!!parameter.delayBuffer}>
          <div class="param-fields">
            <input aria-label="Parameter {index + 1} name" bind:value={parameter.name} oninput={onchange} />
            {#if parameter.delayBuffer}
              <input aria-label="Parameter {index + 1} reference" placeholder="follows parameter…" bind:value={parameter.delayBuffer.refParam} oninput={onchange} />
            {:else}
              <input aria-label="Parameter {index + 1} expression" class:invalid={!!errors[index]} bind:value={parameter.func} oninput={onchange} />
            {/if}
            <input aria-label="Parameter {index + 1} minimum" type="number" step="any" bind:value={parameter.min} oninput={onchange} />
            <input aria-label="Parameter {index + 1} maximum" type="number" step="any" bind:value={parameter.max} oninput={onchange} />
            <input aria-label="Parameter {index + 1} default" type="number" step="any" bind:value={parameter.defaultValue} oninput={onchange} />
            <button class="danger" aria-label="Delete parameter {index + 1}" onclick={() => remove(index)}>×</button>
          </div>
          {#if errors[index]}<p class="row-error">{errors[index]}</p>{/if}
          {#if parameter.delayBuffer}
            <div class="delay-fields">
              <label>Smoothing<input type="number" step="any" bind:value={parameter.delayBuffer.smoothing} oninput={onchange} /></label>
              <label>Delay<input type="number" step="1" min="0" bind:value={parameter.delayBuffer.delayCount} oninput={onchange} /></label>
              <label>In min<input type="number" step="any" bind:value={parameter.delayBuffer.inMin} oninput={onchange} /></label>
              <label>In max<input type="number" step="any" bind:value={parameter.delayBuffer.inMax} oninput={onchange} /></label>
              <label>Out min<input type="number" step="any" bind:value={parameter.delayBuffer.outMin} oninput={onchange} /></label>
              <label>Out max<input type="number" step="any" bind:value={parameter.delayBuffer.outMax} oninput={onchange} /></label>
            </div>
          {/if}
        </div>
      {/each}
    </div>
    <div class="editor-actions"><button onclick={() => add(false)}>+ Parameter</button><button onclick={() => add(true)}>+ Delay buffer</button><span class="muted">{preset.params.length} parameters</span></div>
  </fieldset>
</section>
<style>
  .editor { display: flex; flex-direction: column; padding: 12px; margin: 8px; min-height: 0; }
  .editor-heading { display: flex; gap: 10px; align-items: center; margin-bottom: 12px; }
  h2 { flex: 1; }
  fieldset { display: flex; flex: 1; flex-direction: column; min-height: 0; }
  .metadata { display: grid; grid-template-columns: 2fr 1fr 3fr; gap: 12px; padding-bottom: 14px; }
  .metadata label { display: flex; flex-direction: column; gap: 5px; }
  .param-head, .param-fields { display: grid; grid-template-columns: 160px minmax(200px, 1fr) 70px 70px 70px 32px; gap: 4px; align-items: center; }
  .param-head { padding: 6px; color: var(--text-dim); font-size: 11px; }
  .param-list { overflow: auto; flex: 1; min-height: 0; }
  .parameter { padding: 4px 6px; border-radius: 4px; margin-bottom: 3px; }
  .parameter:nth-child(even) { background: #1b1b1d; }
  .parameter.delay { border-left: 3px solid #b8706a; padding-left: 3px; }
  .delay-fields { display: flex; gap: 12px; margin-top: 6px; flex-wrap: wrap; }
  .delay-fields label { display: flex; align-items: center; gap: 5px; font-size: 11px; }
  .delay-fields input { width: 65px; }
  .row-error { color: var(--bad); margin: 4px 0 0 164px; font-size: 11px; }
  .editor-actions { display: flex; align-items: center; gap: 8px; padding-top: 10px; }
</style>
