<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import {
    addService,
    exampleStack,
    interpolationsFromStack,
    mergeEnv,
    slug,
    stackEnvCount,
    stackToYaml,
    yamlToStack,
    type StackForm
  } from '../lib/compose';
  import AppWindow from '../components/AppWindow.svelte';
  import ComposeEditor from '../components/ComposeEditor.svelte';
  import ComposeEnv from '../components/ComposeEnv.svelte';
  import UiIcon from '../components/UiIcon.svelte';

  let { go, id, onClose } = $props<{ go: (to: string) => void; id?: string; onClose: () => void }>();

  let stack = $state<StackForm>(exampleStack());
  let yaml = $state('');
  let mode = $state<'form' | 'yaml'>('form');
  let pane = $state<'app' | 'services' | 'env'>('app');
  let tab = $state(0);
  let appId = $state('');
  let logs = $state('');
  let error = $state('');
  let busy = $state('');
  let running = $state(false);
  let statusError = $state('');

  const extras = $derived(Object.keys(stack.extraDoc).sort());
  const envTotal = $derived(stackEnvCount(stack));
  const svc = $derived(stack.services[tab]);

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

  onMount(async () => {
    if (!id) {
      appId = slug(stack.title || stack.services[0]?.serviceName || 'app');
      syncYaml();
      return;
    }
    try {
      const app = await api<any>('/api/apps/' + id);
      yaml = app.compose_yaml;
      stack = yamlToStack(yaml);
      if (!stack.title) stack.title = app.name;
      if (app.icon_url) stack.iconUrl = app.icon_url;
      if (app.web_port && !stack.webPort) stack.webPort = String(app.web_port);
      appId = app.id;
      running = app.status?.running;
      statusError = app.status?.error || '';
      pane = stack.services.length > 1 ? 'services' : 'app';
    } catch (e: any) {
      error = e.message;
    }
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
        id: appId || undefined,
        compose_yaml: yaml,
        icon_url: stack.iconUrl || null,
        web_port: stack.webPort ? Number(stack.webPort) : null
      };
      const app = id
        ? await api<any>('/api/apps/' + id, { method: 'PUT', body: JSON.stringify(body) })
        : await api<any>('/api/apps', { method: 'POST', body: JSON.stringify(body) });
      go('/apps/' + app.id);
      running = app.status?.running;
    } catch (err: any) {
      error = err.message;
    } finally {
      busy = '';
    }
  }

  async function act(kind: 'start' | 'stop' | 'restart') {
    if (!id) return;
    busy = kind;
    error = '';
    try {
      const app = await api<any>(`/api/apps/${id}/${kind}`, { method: 'POST' });
      running = app.status?.running;
      statusError = app.status?.error || '';
    } catch (err: any) {
      error = err.message;
    } finally {
      busy = '';
    }
  }

  async function loadLogs() {
    if (!id) return;
    const res = await api<{ logs: string }>(`/api/apps/${id}/logs`);
    logs = res.logs;
  }

  async function remove() {
    if (!id || !confirm('Remove this app and stop its containers?')) return;
    await api('/api/apps/' + id, { method: 'DELETE' });
    go('/apps');
  }
</script>

<AppWindow title={id ? 'App settings' : 'Install app'} icon={id ? 'apps' : 'install'} size="xl" {onClose}>
  {#snippet actions()}
    <div class="os-tabs" role="tablist" aria-label="Editor mode">
      <button type="button" class="os-tab" class:active={mode === 'form'} role="tab" aria-selected={mode === 'form'} onclick={() => setMode('form')}>Form</button>
      <button type="button" class="os-tab" class:active={mode === 'yaml'} role="tab" aria-selected={mode === 'yaml'} onclick={() => setMode('yaml')}>YAML</button>
    </div>
    {#if id}
      {#if !running}
        <button class="btn compact" disabled={!!busy} onclick={() => act('start')}>Start</button>
      {:else}
        <button class="btn secondary compact" disabled={!!busy} onclick={() => act('stop')}>Stop</button>
      {/if}
      <button class="btn secondary compact" disabled={!!busy} onclick={() => act('restart')}>Restart</button>
      {#if stack.webPort}
        <a class="btn secondary compact" href="{stack.scheme}://{stack.webHost || location.hostname}:{stack.webPort}{stack.webPath || '/'}" target="_blank" rel="noreferrer">Open</a>
      {/if}
    {/if}
  {/snippet}

{#if statusError}<div class="err">{statusError}</div>{/if}
{#if error}<div class="err">{error}</div>{/if}

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
            <label class="field"><span>Id (optional)</span><input bind:value={appId} placeholder="immich" /></label>
          {:else}
            <label class="field"><span>Id</span><input value={appId} disabled /></label>
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
        <p class="hint">{id ? (running ? 'Running' : 'Stopped') : 'Name the app, then set services and environment.'}</p>
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
              <ComposeEditor bind:service={stack.services[tab]} />
            {/if}
          {/key}
        </div>
      </div>
    {:else}
      <ComposeEnv bind:stack />
    {/if}
  {:else}
    <label class="field"><span>compose.yml</span><textarea class="yaml-editor" bind:value={yaml} required></textarea></label>
  {/if}
  <div class="row">
    <button class="btn" disabled={!!busy}>{id ? 'Save' : 'Create'}</button>
    {#if id}
      {#if stack.webPort}
        <button type="button" class="btn secondary" onclick={() => go(`/proxy?app=${encodeURIComponent(id)}&port=${stack.webPort}`)}>Add to Proxy</button>
      {/if}
      <button type="button" class="btn danger" onclick={remove}>Delete</button>
    {/if}
  </div>
</form>

{#if id}
  <div class="top" style="margin-top:24px">
    <h2>Logs</h2>
    <button class="btn secondary compact" onclick={loadLogs}>Refresh logs</button>
  </div>
  <pre class="logs">{logs || 'Click refresh to load logs.'}</pre>
{/if}
</AppWindow>
