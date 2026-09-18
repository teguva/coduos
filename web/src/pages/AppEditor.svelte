<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import {
    errorTip,
    isBusy,
    isLaunchable,
    overlayJob,
    subscribeAppJobs,
    type AppJob,
    type AppRecord,
    type AppStatus
  } from '../lib/apps';
  import {
    addService,
    classifyComposeFile,
    downloadText,
    envFileBody,
    exampleStack,
    interpolationsFromStack,
    mergeEnv,
    parseDotEnv,
    slug,
    stackEnvCount,
    stackToYaml,
    yamlToStack,
    type StackForm
  } from '../lib/compose';
  import AppWindow from '../components/AppWindow.svelte';
  import ComposeEditor from '../components/ComposeEditor.svelte';
  import ComposeEnv from '../components/ComposeEnv.svelte';
  import Confirm from '../components/Confirm.svelte';
  import InfoTip from '../components/InfoTip.svelte';
  import PathPicker from '../components/PathPicker.svelte';
  import ProgressStrip from '../components/ProgressStrip.svelte';
  import StatusPill from '../components/StatusPill.svelte';
  import UiIcon from '../components/UiIcon.svelte';

  let { go, id, onClose } = $props<{ go: (to: string) => void; id?: string; onClose: () => void }>();

  let stack = $state<StackForm>(exampleStack());
  let yaml = $state('');
  let mode = $state<'form' | 'yaml'>('form');
  let pane = $state<'app' | 'services' | 'env'>('app');
  let tab = $state(0);
  let appId = $state('');
  let appDir = $state('');
  let idCustom = $state(false);
  let logs = $state('');
  let error = $state('');
  let notice = $state('');
  let busy = $state('');
  let importEl: HTMLInputElement | undefined;
  let yamlEl: HTMLTextAreaElement | undefined;
  let pendingImport = $state<{ name: string; text: string }[] | null>(null);
  let pathPick = $state<{ start: string; onPick: (path: string) => void } | null>(null);
  let needsUpdate = $state(false);
  let status = $state<AppStatus>({
    running: false,
    installed: false,
    phase: 'not_installed'
  });

  const extras = $derived(Object.keys(stack.extraDoc).sort());
  const envTotal = $derived(stackEnvCount(stack));
  const svc = $derived(stack.services[tab]);
  const jobBusy = $derived(isBusy(status.phase));
  const primary = $derived.by(() => {
    if (!id || jobBusy) return null;
    if (needsUpdate && status.installed) return 'update' as const;
    if (status.phase === 'not_installed') return 'install' as const;
    if (status.phase === 'error') return status.installed ? ('start' as const) : ('install' as const);
    if (status.phase === 'stopped') return 'start' as const;
    return null;
  });
  const primaryLabel = $derived(
    status.phase === 'error' && (primary === 'install' || primary === 'start')
      ? 'Retry'
      : primary === 'install'
        ? 'Install'
        : primary === 'start'
          ? 'Start'
          : primary === 'update'
            ? 'Update'
            : ''
  );

  function svcMeta(s: StackForm['services'][number]) {
    const bits: string[] = [];
    if (s.ports.length) bits.push(`${s.ports.length} port${s.ports.length === 1 ? '' : 's'}`);
    const env = s.env.filter((e) => e.key.trim()).length;
    if (env) bits.push(`${env} env`);
    if (s.volumes.length) bits.push(`${s.volumes.length} vol${s.volumes.length === 1 ? '' : 's'}`);
    return bits.join(' · ');
  }

  function imageTail(image: string) {
    const s = image.trim();
    if (!s) return 'No image';
    const noDigest = s.split('@')[0];
    const name = noDigest.split('/').pop() || noDigest;
    return name;
  }

  function syncYaml() {
    stack.dotEnv = mergeEnv(interpolationsFromStack(stack), stack.dotEnv);
    yaml = stackToYaml(stack);
  }
  function syncForm() {
    const keep = stack.services[tab]?.serviceName;
    stack = yamlToStack(yaml);
    const i = stack.services.findIndex((s) => s.serviceName === keep);
    tab = i >= 0 ? i : 0;
    if (stack.services.length > 1 && pane === 'app') pane = 'services';
  }

  function setMode(next: 'form' | 'yaml') {
    if (next === 'yaml' && mode === 'form') syncYaml();
    if (next === 'form' && mode === 'yaml') syncForm();
    mode = next;
  }

  function add() {
    stack = addService(stack);
    tab = stack.services.length - 1;
    pane = 'services';
  }

  function removeAt(i: number) {
    if (stack.services.length < 2) return;
    stack = { ...stack, services: stack.services.filter((_, n) => n !== i) };
    if (tab >= stack.services.length) tab = stack.services.length - 1;
    else if (tab > i) tab -= 1;
  }

  function applyApp(app: AppRecord, jobs: Record<string, AppJob>) {
    yaml = app.compose_yaml || yaml;
    stack = yamlToStack(yaml);
    if (!stack.title) stack.title = app.name;
    if (app.icon_url) stack.iconUrl = app.icon_url;
    if (app.web_port && !stack.webPort) stack.webPort = String(app.web_port);
    appId = app.id;
    if (app.app_dir) appDir = app.app_dir;
    status = overlayJob(app.status, jobs[app.id]);
  }

  async function loadApp(jobs: Record<string, AppJob>) {
    if (!id) return;
    const app = await api<AppRecord>('/api/apps/' + id);
    applyApp(app, jobs);
  }

  async function refreshStatus(jobs: Record<string, AppJob>) {
    if (!id) return;
    const app = await api<AppRecord>('/api/apps/' + id);
    status = overlayJob(app.status, jobs[app.id]);
  }

  function suggestedId() {
    return slug(stack.title);
  }

  function browsePath(start: string, pick: (path: string) => void) {
    pathPick = { start, onPick: pick };
  }

  function browseYamlPath() {
    const el = yamlEl;
    let from = yaml.length;
    let to = yaml.length;
    let start = '';
    if (el) {
      from = el.selectionStart ?? yaml.length;
      to = el.selectionEnd ?? from;
      const sel = yaml.slice(from, to).trim();
      if (sel.startsWith('/') || sel.startsWith('./')) start = sel;
    }
    browsePath(start, (p) => {
      yaml = yaml.slice(0, from) + p + yaml.slice(to);
      requestAnimationFrame(() => {
        yamlEl?.focus();
        const pos = from + p.length;
        yamlEl?.setSelectionRange(pos, pos);
      });
    });
  }

  function fileBase() {
    return (appId.trim() || suggestedId() || 'app').replace(/[^a-z0-9._-]+/gi, '-');
  }

  function currentYaml() {
    if (mode === 'form') {
      stack.dotEnv = mergeEnv(interpolationsFromStack(stack), stack.dotEnv);
      yaml = stackToYaml(stack);
    }
    return yaml;
  }

  function exportYaml() {
    notice = '';
    const body = currentYaml();
    if (!body.trim()) {
      error = 'Nothing to export yet.';
      return;
    }
    downloadText(`${fileBase()}-compose.yml`, body, 'text/yaml;charset=utf-8');
  }

  function exportEnv() {
    notice = '';
    if (mode === 'form') {
      stack.dotEnv = mergeEnv(interpolationsFromStack(stack), stack.dotEnv);
    } else {
      try {
        const next = yamlToStack(yaml);
        next.dotEnv = mergeEnv(next.dotEnv, stack.dotEnv);
        stack = next;
      } catch (err: any) {
        error = err.message || 'YAML is not valid.';
        return;
      }
    }
    const body = envFileBody(stack);
    if (!body.trim()) {
      error = 'No environment variables to export.';
      return;
    }
    downloadText(`${fileBase()}.env`, body);
  }

  async function onImportFiles(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const files = [...(input.files || [])];
    input.value = '';
    error = '';
    notice = '';
    if (!files.length) return;
    const loaded: { name: string; text: string }[] = [];
    for (const file of files) {
      if (file.size > 2 * 1024 * 1024) {
        error = `${file.name} is larger than 2 MB.`;
        return;
      }
      loaded.push({ name: file.name, text: await file.text() });
    }
    const hasYaml = loaded.some((f) => classifyComposeFile(f.name, f.text) === 'yaml');
    if (id && hasYaml) {
      pendingImport = loaded;
      return;
    }
    applyImport(loaded);
  }

  function applyImport(files: { name: string; text: string }[]) {
    pendingImport = null;
    const yamlFiles = files.filter((f) => classifyComposeFile(f.name, f.text) === 'yaml');
    const envFiles = files.filter((f) => classifyComposeFile(f.name, f.text) === 'env');
    const unknown = files.filter((f) => classifyComposeFile(f.name, f.text) === 'unknown');
    if (unknown.length && !yamlFiles.length && !envFiles.length) {
      error = `Could not read ${unknown[0].name} as compose YAML or .env.`;
      return;
    }
    const prevEnv = stack.dotEnv;
    const names: string[] = [];
    if (yamlFiles.length) {
      const src = yamlFiles[yamlFiles.length - 1];
      try {
        const next = yamlToStack(src.text);
        if (!next.services.some((s) => s.image.trim())) {
          error = `${src.name} has no services with a Docker image.`;
          return;
        }
        next.dotEnv = mergeEnv(next.dotEnv, prevEnv);
        stack = next;
        yaml = src.text.endsWith('\n') ? src.text : src.text + '\n';
        if (!id && !idCustom && next.title) appId = slug(next.title);
        names.push(src.name);
      } catch (err: any) {
        error = `${src.name} is not valid YAML: ${err.message || err}`;
        return;
      }
    }
    if (envFiles.length) {
      let rows = yamlFiles.length ? stack.dotEnv : prevEnv;
      for (const src of envFiles) {
        const parsed = parseDotEnv(src.text);
        if (!parsed.length) {
          error = `${src.name} had no KEY=value lines.`;
          return;
        }
        rows = mergeEnv(interpolationsFromStack(stack), rows, parsed);
        names.push(src.name);
      }
      stack.dotEnv = rows;
      if (mode === 'yaml') yaml = stackToYaml(stack);
    }
    if (yamlFiles.length && mode === 'form') {
      pane = envFiles.length ? 'env' : stack.services.length > 1 ? 'services' : 'app';
    } else if (envFiles.length && mode === 'form') {
      pane = 'env';
    }
    notice = `Imported ${names.join(' and ')}. Save to keep the change.`;
    if (unknown.length) {
      notice += ` Skipped ${unknown.map((f) => f.name).join(', ')}.`;
    }
  }

  $effect(() => {
    if (id || idCustom) return;
    appId = suggestedId();
  });

  function onIdInput(ev: Event) {
    const v = (ev.currentTarget as HTMLInputElement).value;
    appId = v;
    idCustom = v.trim() !== '' && v !== suggestedId();
  }

  onMount(() => {
    let jobs: Record<string, AppJob> = {};
    if (!id) {
      syncYaml();
    } else {
      loadApp(jobs)
        .then(() => {
          pane = stack.services.length > 1 ? 'services' : 'app';
          return loadLogs();
        })
        .catch((e: Error) => (error = e.message));
    }
    const stop = subscribeAppJobs((next) => {
      const prev = id ? jobs[id] : undefined;
      jobs = next;
      if (!id) return;
      if (prev && !next[id]) {
        refreshStatus(jobs)
          .then(() => loadLogs())
          .catch(() => {});
        return;
      }
      if (next[id]) status = overlayJob(status, next[id]);
    });
    return stop;
  });

  async function save(e: Event) {
    e.preventDefault();
    error = '';
    if (mode === 'form') {
      if (!stack.title.trim()) {
        pane = 'app';
        error = 'Give the app a title.';
        return;
      }
      const bad = stack.services.findIndex((s) => !s.serviceName.trim() || !s.image.trim());
      if (bad >= 0) {
        tab = bad;
        pane = 'services';
        error = 'Each service needs a name and a Docker image.';
        return;
      }
    }
    busy = 'save';
    try {
      if (mode === 'form') syncYaml();
      else syncForm();
      const body = {
        name: stack.title || stack.services[0]?.serviceName || 'app',
        id: (appId.trim() || suggestedId()) || undefined,
        compose_yaml: yaml,
        icon_url: stack.iconUrl || null,
        web_port: stack.webPort ? Number(stack.webPort) : null
      };
      if (id) {
        const app = await api<AppRecord>('/api/apps/' + id, { method: 'PUT', body: JSON.stringify(body) });
        status = app.status;
        if (app.app_dir) appDir = app.app_dir;
        if (app.status.installed) needsUpdate = true;
      } else {
        const app = await api<AppRecord>('/api/apps', { method: 'POST', body: JSON.stringify(body) });
        try {
          await api<AppRecord>('/api/apps/' + app.id + '/install', { method: 'POST' });
        } catch (err: any) {
          error = err.message;
        }
        go('/apps/' + app.id);
        return;
      }
    } catch (err: any) {
      error = err.message;
    } finally {
      busy = '';
    }
  }

  async function act(kind: 'install' | 'start' | 'stop' | 'restart' | 'update') {
    if (!id) return;
    busy = kind;
    error = '';
    try {
      const app = await api<AppRecord>(`/api/apps/${id}/${kind}`, { method: 'POST' });
      status = app.status;
      if (kind === 'update' || kind === 'install' || kind === 'start') needsUpdate = false;
    } catch (err: any) {
      error = err.message;
    } finally {
      busy = '';
    }
  }

  async function loadLogs() {
    if (!id) return;
    try {
      const res = await api<{ logs: string }>(`/api/apps/${id}/logs`);
      logs = (res.logs || '').trim() ? res.logs : '(empty)';
    } catch (err: any) {
      logs = err.message || 'Could not load logs.';
    }
  }

  async function remove() {
    if (!id || !confirm('Remove this app and stop its containers?')) return;
    await api('/api/apps/' + id, { method: 'DELETE' });
    go('/');
  }
</script>

<AppWindow title={id ? 'App settings' : 'Install app'} icon={id ? 'apps' : 'install'} size="xl" {onClose}>
  {#snippet actions()}
    <div class="os-tabs" role="tablist" aria-label="Editor mode">
      <button type="button" class="os-tab" class:active={mode === 'form'} role="tab" aria-selected={mode === 'form'} onclick={() => setMode('form')}>Form</button>
      <button type="button" class="os-tab" class:active={mode === 'yaml'} role="tab" aria-selected={mode === 'yaml'} onclick={() => setMode('yaml')}>YAML</button>
    </div>
    {#if id}
      <StatusPill {status} />
      {#if status.phase === 'error'}
        <InfoTip label="Why this failed" text={errorTip(status)} />
      {/if}
      {#if primary}
        <button class="btn compact" disabled={!!busy || jobBusy} onclick={() => act(primary)}>{primaryLabel}</button>
        {#if primary === 'install'}
          <InfoTip label="What Install does" text="Downloads the images, then starts the app. Large apps can take several minutes." />
        {:else if primary === 'update'}
          <InfoTip label="What Update does" text="Downloads newer images and restarts this app." />
        {/if}
      {/if}
      {#if status.running}
        <button class="btn secondary compact" disabled={!!busy || jobBusy} onclick={() => act('stop')}>Stop</button>
      {/if}
      <button class="btn secondary compact" disabled={!!busy || jobBusy || !status.installed} onclick={() => act('restart')}>Restart</button>
      {#if stack.webPort && isLaunchable(status)}
        <a class="btn secondary compact" href="{stack.scheme}://{stack.webHost || location.hostname}:{stack.webPort}{stack.webPath || '/'}" target="_blank" rel="noreferrer">Open</a>
      {/if}
    {/if}
  {/snippet}

{#if id && jobBusy}
  <ProgressStrip percent={status.percent ?? null} pulse={status.percent == null} />
  {#if status.message}
    <p class="hint">{status.message}</p>
  {/if}
{/if}
{#if error}<div class="err">{error}</div>{/if}
{#if notice}<p class="hint">{notice}</p>{/if}

<div class="compose-io">
  <button type="button" class="btn secondary compact" onclick={() => importEl?.click()}>
    <UiIcon name="upload" size={18} /> Import
  </button>
  <input
    bind:this={importEl}
    type="file"
    hidden
    multiple
    accept=".yml,.yaml,.env,text/yaml,text/plain,.txt,application/yaml"
    onchange={onImportFiles}
  />
  <button type="button" class="btn secondary compact" onclick={exportYaml}>
    <UiIcon name="download" size={18} /> Export YAML
  </button>
  <button type="button" class="btn secondary compact" onclick={exportEnv}>
    <UiIcon name="download" size={18} /> Export .env
  </button>
  <p class="hint">Import compose.yml and .env together (Immich ships both). Export downloads the files Docker Compose uses.</p>
</div>

<form onsubmit={save}>
  {#if mode === 'form'}
    <div class="os-tabs form-tabs" role="tablist" aria-label="Compose sections">
      <button type="button" class="os-tab" class:active={pane === 'app'} role="tab" aria-selected={pane === 'app'} onclick={() => (pane = 'app')}>App</button>
      <button type="button" class="os-tab" class:active={pane === 'services'} role="tab" aria-selected={pane === 'services'} onclick={() => (pane = 'services')}>
        Services{#if stack.services.length > 1}<span class="tab-count">{stack.services.length}</span>{/if}
      </button>
      <button type="button" class="os-tab" class:active={pane === 'env'} role="tab" aria-selected={pane === 'env'} onclick={() => (pane = 'env')}>
        Environment{#if envTotal}<span class="tab-count">{envTotal}</span>{/if}
      </button>
    </div>

    {#if pane === 'app'}
      <section class="form-sec">
        <div class="app-grid">
          <label class="field"><span>Title</span><input bind:value={stack.title} placeholder="Immich" required /></label>
          {#if !id}
            <label class="field">
              <span class="field-head">Id
                <InfoTip label="About the app id" text="Filled from the title: lowercase, spaces become dashes. You can change it; after that it stays put. This is the folder and Docker name and cannot change later." />
              </span>
              <input value={appId} placeholder="jellyfin" oninput={onIdInput} />
            </label>
          {:else}
            <label class="field">
              <span class="field-head">Id
                <InfoTip label="About the app id" text="Folder and Docker name. Relative paths like ./library live inside this folder. The title can change later; this cannot." />
              </span>
              <input value={appId} disabled />
            </label>
          {/if}
          <label class="field span-2"><span>Icon URL</span>
            <div class="icon-row">
              {#if stack.iconUrl}
                <img class="app-icon" src={stack.iconUrl} alt="" />
              {:else}
                <img class="app-icon" src="/icons/docker.svg" alt="" />
              {/if}
              <input bind:value={stack.iconUrl} placeholder="https://…" />
            </div>
          </label>
        </div>
        <div class="field">
          <span>Web UI</span>
          <div class="webui">
            <label class="field"><span>Scheme</span>
              <select bind:value={stack.scheme}>
                <option value="http">http://</option>
                <option value="https">https://</option>
              </select>
            </label>
            <label class="field"><span>Host</span>
              <input bind:value={stack.webHost} placeholder={typeof location !== 'undefined' ? location.hostname : 'host'} />
            </label>
            <label class="field"><span>Port</span>
              <input bind:value={stack.webPort} placeholder="2283" />
            </label>
            <label class="field"><span>Path</span>
              <input bind:value={stack.webPath} placeholder="/" />
            </label>
          </div>
        </div>
        {#if extras.length}
          <p class="hint">Kept from YAML: {extras.join(', ')}</p>
        {/if}
        {#if appDir}
          <p class="hint">App folder: <code>{appDir}</code>. Relative host paths like <code>./library</code> are created inside it. New apps go in <code>/DATA/AppData</code> when that disk is mounted.</p>
        {/if}
        <p class="hint">{id ? (status.message || '') : 'Name the app, then set services and environment.'}</p>
      </section>
    {:else if pane === 'services'}
      <div class="compose-split">
        <nav class="svc-rail" aria-label="Services">
          {#each stack.services as s, i}
            <div class="svc-item" class:active={tab === i}>
              <button type="button" class="svc-pick" onclick={() => (tab = i)}>
                <span class="svc-name">{s.serviceName || `service-${i + 1}`}</span>
                <span class="svc-meta">{svcMeta(s) || imageTail(s.image)}</span>
              </button>
              {#if stack.services.length > 1}
                <button type="button" class="svc-x" title="Remove service" onclick={() => removeAt(i)}>
                  <UiIcon name="close" size={16} />
                </button>
              {/if}
            </div>
          {/each}
          <button type="button" class="svc-add" onclick={add}>
            <UiIcon name="add" size={18} /> Add service
          </button>
        </nav>
        <div class="svc-body">
          {#key tab}
            {#if svc}
              <ComposeEditor bind:service={stack.services[tab]} {appDir} env={stack.dotEnv} onBrowse={browsePath} />
            {/if}
          {/key}
        </div>
      </div>
    {:else}
      <ComposeEnv bind:stack {appDir} fileBase={fileBase()} onBrowse={browsePath} />
    {/if}
  {:else}
    <div class="field">
      <span class="field-head">compose.yml
        <button type="button" class="btn secondary compact" onclick={browseYamlPath}>
          <UiIcon name="folder_open" size={18} /> Browse path
        </button>
      </span>
      <textarea class="yaml-editor" bind:this={yamlEl} bind:value={yaml} required></textarea>
    </div>
  {/if}
  <div class="row">
    <button class="btn" disabled={!!busy || jobBusy}>{id ? 'Save' : 'Install'}</button>
    {#if !id}
      <InfoTip label="What Install does" text="Downloads the images, then starts the app. Large apps can take several minutes." />
    {/if}
    {#if id}
      {#if stack.webPort}
        <button type="button" class="btn secondary" onclick={() => go(`/proxy?app=${encodeURIComponent(id)}&port=${stack.webPort}`)}>Add to Proxy</button>
      {/if}
      <button type="button" class="btn danger" disabled={jobBusy} onclick={remove}>Delete</button>
    {/if}
  </div>
</form>

{#if id}
  <div class="top" style="margin-top:24px">
    <h2>Logs</h2>
    <button class="btn secondary compact" onclick={loadLogs}>Refresh logs</button>
  </div>
  <pre class="logs">{logs || 'Loading logs…'}</pre>
{/if}
</AppWindow>

{#if pathPick}
  <PathPicker
    start={pathPick.start}
    title="Choose path"
    onPick={(p) => {
      pathPick?.onPick(p);
      pathPick = null;
    }}
    onClose={() => (pathPick = null)}
  />
{/if}
{#if pendingImport}
  <Confirm
    title="Replace compose YAML?"
    body="The imported file replaces this app’s compose. Matching environment keys are kept. Save to write it to disk."
    confirmLabel="Replace"
    onConfirm={() => applyImport(pendingImport || [])}
    onCancel={() => (pendingImport = null)}
  />
{/if}
