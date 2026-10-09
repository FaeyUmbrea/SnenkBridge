<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import type { Parameter, Preset, Preview, Settings, Snapshot } from './lib/types';
  import FacePreview from './components/FacePreview.svelte';
  import ValuePanel from './components/ValuePanel.svelte';
  import Editor from './components/Editor.svelte';
  import iconUrl from '../../resources/SnenkBridgeIcon.svg?url';

  let app = $state<Snapshot>();
  let draft = $state<Preset>();
  let settings = $state<Settings>();
  let preview = $state<Preview>();
  let tab = $state('preview');
  let mode = $state('input');
  let error = $state('');
  let dialog = $state<'new' | 'import' | 'guard' | 'variables' | 'about' | null>(null);
  let dialogElement: HTMLDialogElement;
  let title = $state('New preset');
  let author = $state('');
  let description = $state('');
  let base = $state('');
  let vitamins = $state(false);
  let swapXY = $state(false);
  let manualName = $state('JawOpen');
  let manualValue = $state(0);
  let pending: (() => Promise<void>) | undefined;
  let queue = Promise.resolve();
  let localDirty = $state(false);
  let busy = $state(false);
  let editing = $state(false);

  function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
    const task = queue.then(() => invoke<T>(name, args));
    queue = task.then(() => {}, () => {});
    return task;
  }
  function apply(snapshot: Snapshot, keepDraft = false) {
    app = snapshot;
    preview = snapshot.preview;
    if (!keepDraft) { draft = snapshot.preset; settings = snapshot.settings; localDirty = snapshot.dirty; }
    error = snapshot.error;
  }
  async function action(work: () => Promise<void>) {
    if (busy) return;
    busy = true;
    try { error = ''; await work(); } catch (e) { error = String(e); } finally { busy = false; }
  }
  function edit() {
    if (!draft) return;
    localDirty = true;
    const preset: Preset = JSON.parse(JSON.stringify(draft));
    // Do not send temporarily empty number fields as null to Rust.
    const finite = (p: Parameter) => [p.min, p.max, p.defaultValue, ...(p.delayBuffer ? [p.delayBuffer.smoothing, p.delayBuffer.delayCount, p.delayBuffer.inMin, p.delayBuffer.inMax, p.delayBuffer.outMin, p.delayBuffer.outMax] : [])].every((n) => typeof n === 'number' && Number.isFinite(n));
    editing = !preset.params.every(finite) || preset.params.some((p) => p.delayBuffer && (!Number.isInteger(p.delayBuffer.delayCount) || p.delayBuffer.delayCount < 0));
    if (editing) { error = 'Complete numeric fields with finite values; delay counts must be nonnegative integers.'; return; }
    void command<Snapshot>('update_preset', { preset }).then((snapshot) => apply(snapshot, true)).catch((e) => { error = String(e); });
  }
  async function save() {
    if (editing) throw new Error('Complete numeric fields before saving.');
    apply(await command<Snapshot>('save_preset'));
  }
  async function persist() {
    if (settings) await command('update_settings', { settings: JSON.parse(JSON.stringify(settings)) });
  }
  function show(kind: typeof dialog) {
    dialog = kind;
    dialogElement.showModal();
  }
  function dismiss() { dialogElement.close(); dialog = null; pending = undefined; }
  function guard(work: () => Promise<void>) {
    if (localDirty || app?.dirty) { pending = work; show('guard'); }
    else void action(work);
  }
  async function finishGuard(saveFirst: boolean) {
    await action(async () => {
      if (saveFirst) await save();
      const work = pending;
      dismiss();
      await work?.();
    });
  }
  function createDialog() {
    title = 'New preset'; author = ''; description = ''; base = ''; show('new');
  }
  async function importDialog() {
    const result = await command<{ preset: Preset; vitamins: boolean } | null>('pick_import');
    if (result) {
      title = result.preset.title; author = result.preset.author ?? ''; description = result.preset.description ?? '';
      vitamins = result.vitamins; swapXY = false; show('import');
    }
  }
  async function submitPreset() {
    await action(async () => {
      if (dialog === 'new') apply(await command<Snapshot>('new_preset', { title, author, description, base: base === '' ? null : Number(base), discard: true }));
      else apply(await command<Snapshot>('import_preset', { title, author, description, swapXy: swapXY, discard: true }));
      dismiss();
    });
  }
  function toggle(connection: 'source' | 'target') {
    void action(async () => {
      if (editing && connection === 'target') throw new Error('Complete numeric fields before starting output.');
      await persist(); await command(`toggle_${connection}`);
      apply(await command<Snapshot>('snapshot'), true);
    });
  }
  const statusClass = (status: string) => status.startsWith('Receiving') || status === 'Connected' ? 'good' : status === 'Stopped' ? 'muted' : 'warn';

  onMount(() => {
    let disposed = false;
    const unsubscribers: (() => void)[] = [];
    async function initialize() {
      const previewListener = await listen<Preview>('preview-update', (event) => { preview = event.payload; });
      if (disposed) { previewListener(); return; }
      unsubscribers.push(previewListener);
      const closeListener = await listen('close-requested', () => guard(async () => { await command('close_app'); }));
      if (disposed) { closeListener(); return; }
      unsubscribers.push(closeListener);
      apply(await command<Snapshot>('snapshot'));
    }
    void initialize().catch((e) => { error = `Could not start SnenkBridge: ${String(e)}`; });
    const keydown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') { event.preventDefault(); void action(save); }
    };
    window.addEventListener('keydown', keydown);
    return () => { disposed = true; unsubscribers.forEach((unsubscribe) => unsubscribe()); window.removeEventListener('keydown', keydown); };
  });
</script>

<div class="application">
  {#if app && draft && settings && preview}
    <header>
      <nav aria-label="Main tabs"><button class:primary={tab === 'preview'} onclick={() => tab = 'preview'}>Preview</button><button class:primary={tab === 'editor'} onclick={() => tab = 'editor'}>Editor</button></nav>
      <span class="group-tag">PRESET</span>
      <select aria-label="Preset" value={app.selected} disabled={preview.target_active || busy} onchange={(event) => { const index = Number(event.currentTarget.value); event.currentTarget.value = String(app?.selected); guard(async () => apply(await command<Snapshot>('switch_preset', { index, discard: true }))); }}>
        {#each app.choices as choice, index (index)}<option value={index}>{choice}</option>{/each}
      </select>
      <button disabled={preview.target_active || busy} onclick={() => guard(async () => createDialog())}>New</button>
      <button disabled={preview.target_active || busy} onclick={() => guard(importDialog)}>Import</button>
      <button disabled={preview.target_active || busy || editing} onclick={() => action(async () => { await command('export_preset'); })}>Export</button>
      {#if app.can_delete}<button class="danger" disabled={preview.target_active || busy} onclick={() => guard(async () => apply(await command<Snapshot>('delete_preset', { discard: true })))}>Delete</button>{/if}
      <button class="about" onclick={() => show('about')}>About</button>
    </header>
    <div class="manual-bar">
      <span class="muted">{preview.source_active ? 'Live preview' : 'Offline preview'}</span>
      <input aria-label="Manual variable" bind:value={manualName} disabled={preview.source_active} />
      <input aria-label="Manual value" type="number" step="any" bind:value={manualValue} disabled={preview.source_active} />
      <button disabled={preview.source_active || busy} onclick={() => action(async () => { await command('manual_input', { name: manualName, value: manualValue }); })}>Set input</button>
      <button disabled={preview.source_active || busy} onclick={() => action(async () => { await command('advance_preview'); })}>Advance one update</button>
      <span class="muted hint">Manual edits do not advance delays</span>
    </div>
    <main>
      {#if tab === 'preview'}
        <div class="preview-grid">
          <ValuePanel title="INPUT SHAPES" values={preview.inputs} />
          <section class="card center-preview">
            <div class="preview-toolbar"><button class:primary={mode === 'input'} onclick={() => mode = 'input'}>Input Shape Preview</button><button class:primary={mode === 'output'} onclick={() => mode = 'output'}>Output Shape Preview</button>{#if mode === 'input'}<span class="mesh-tag">3D ARKit Mesh (.glb)</span>{/if}</div>
            <div class="viewport">{#if mode === 'input'}<FacePreview values={preview.inputs} />{:else}<ValuePanel title="CALCULATED OUTPUT — SAME VALUES SENT TO VTUBE STUDIO" values={preview.outputs} params={draft.params} />{/if}</div>
          </section>
          <ValuePanel title="OUTPUT PARAMS" values={preview.outputs} params={draft.params} />
        </div>
      {:else}
        <Editor bind:preset={draft} errors={app.errors} disabled={preview.target_active || busy} dirty={localDirty} onchange={edit} onsave={() => action(save)} onvariables={() => show('variables')} />
      {/if}
    </main>
    {#if error}<div class="error-bar" role="alert">{error}<button aria-label="Dismiss error" onclick={() => error = ''}>×</button></div>{/if}
    <footer class="card connections">
      <div><span class="group-tag">SOURCE</span><label>Phone IP<input bind:value={settings.phone_ip} disabled={preview.source_active} onchange={() => action(persist)} /></label><label>Source<select bind:value={settings.tracking_type_index} disabled={preview.source_active} onchange={() => action(persist)}><option value={0}>VTubeStudio</option><option value={1}>IFacialMocap</option></select></label><label>Timeout<input class="small" bind:value={settings.face_search_timeout} disabled={preview.source_active} onchange={() => action(persist)} /><span class="muted">ms</span></label><span class="connection-status {statusClass(preview.source_status)}">{preview.source_status}</span><button class:connect={!preview.source_active} class:danger={preview.source_active} disabled={busy} onclick={() => toggle('source')}>{preview.source_active ? 'Disconnect' : 'Connect'}</button></div>
      <div><span class="group-tag">TARGET</span><label>VTube Studio<input bind:value={settings.vts_ip} disabled={preview.target_active} onchange={() => action(persist)} /></label><label>Port<input class="small" bind:value={settings.vts_port} disabled={preview.target_active} onchange={() => action(persist)} /></label><span class="connection-status {statusClass(preview.target_status)}">{preview.target_status}</span><button class:connect={!preview.target_active} class:danger={preview.target_active} disabled={busy || editing} onclick={() => toggle('target')}>{preview.target_active ? 'Disconnect' : 'Connect'}</button></div>
    </footer>
  {:else}
    <p class="startup" role="status">{error || 'Loading SnenkBridge…'}</p>
  {/if}
</div>

<dialog bind:this={dialogElement} oncancel={() => { dialog = null; pending = undefined; }}>
  {#if dialog === 'guard'}
    <h2>Unsaved changes</h2><p>You have unsaved changes. What would you like to do?</p>
    <div class="dialog-actions"><button disabled={busy} onclick={dismiss}>Cancel</button><button class="danger" disabled={busy} onclick={() => finishGuard(false)}>Discard</button><button class="primary" disabled={busy} onclick={() => finishGuard(true)}>Save</button></div>
  {:else if dialog === 'new' || dialog === 'import'}
    <form onsubmit={(event) => { event.preventDefault(); void submitPreset(); }}>
      <h2>{dialog === 'new' ? 'New preset' : 'Import preset'}</h2>
      <label>Title<input bind:value={title} required /></label><label>Author<input bind:value={author} /></label><label>Description<textarea bind:value={description}></textarea></label>
      {#if dialog === 'new'}<label>Base preset<select bind:value={base}><option value="">Empty</option>{#each app?.choices ?? [] as choice, index (index)}<option value={String(index)}>{choice}</option>{/each}</select></label>{/if}
      {#if dialog === 'import' && vitamins}<label class="checkbox"><input type="checkbox" bind:checked={swapXY} />Swap FaceAngleX / FaceAngleY</label>{/if}
      <div class="dialog-actions"><button type="button" disabled={busy} onclick={dismiss}>Cancel</button><button class="primary" type="submit" disabled={busy}>{dialog === 'new' ? 'Create' : 'Import'}</button></div>
    </form>
  {:else if dialog === 'variables'}
    <h2>Available variables</h2><div class="variables"><dl>{#each app?.variables ?? [] as variable (variable)}<dt>{variable}</dt><dd>{variable.startsWith('HeadRot') ? 'Degrees' : variable === 'FaceFound' ? '0 or 1' : 'Tracking input'}</dd>{/each}<dt>WaveN</dt><dd>Triangle 0 → 1 → 0; period N milliseconds</dd><dt>PingPongN</dt><dd>Ramp 0 → 1; period N milliseconds</dd></dl></div><div class="dialog-actions"><button onclick={dismiss}>Close</button></div>
  {:else if dialog === 'about'}
    <div class="about-title"><img src={iconUrl} alt="SnenkBridge" width="64" height="64" /><div><h2>SnenkBridge</h2><p>Version 1.2.1 · Void Monster · Faey Umbrea</p></div></div>
    <p>Open-source face tracking bridge for VTubers. GPL-3.0.</p><p class="muted">github.com/FaeyUmbrea/SnenkBridge</p><h3>Presets</h3><p class="muted">Maruseu VBridger and Maruseu Enhanced presets are based on presets by Maruseu (github.com/maruseu/VitaminsPresets), included with permission. These presets are not covered by the project’s GPL license.</p><h3>Dependencies</h3><pre>{app?.credits}
Svelte (MIT)
Three.js (MIT)
@tauri-apps/api (MIT / Apache-2.0)</pre><div class="dialog-actions"><button onclick={dismiss}>Close</button></div>
  {/if}
  {#if error && dialog}<p class="dialog-error" role="alert">{error}</p>{/if}
</dialog>
