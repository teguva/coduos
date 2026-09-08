<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { addService, exampleStack, slug, stackToYaml, yamlToStack, type StackForm } from '../lib/compose';
  import ComposeEditor from '../components/ComposeEditor.svelte';

  let { go, id } = $props<{ go: (to: string) => void; id?: string }>();

  let stack = $state<StackForm>(exampleStack());
  let yaml = $state('');
  let mode = $state<'form' | 'yaml'>('form');
  let tab = $state(0);
  let appId = $state('');
  let logs = $state('');
  let error = $state('');
  let busy = $state('');
  let running = $state(false);
  let statusError = $state('');

  const extras = $derived(Object.keys(stack.extraDoc).sort());

  function syncYaml() {
    yaml = stackToYaml(stack);
  }
  function syncForm() {
    const keep = stack.services[tab]?.serviceName;
    stack = yamlToStack(yaml);
    const i = stack.services.findIndex((s) => s.serviceName === keep);
    tab = i >= 0 ? i : 0;
  }

  function setMode(next: 'form' | 'yaml') {
    if (next === 'yaml' && mode === 'form') syncYaml();
    if (next === 'form' && mode === 'yaml') syncForm();
    mode = next;
  }

  function add() {
    stack = addService(stack);
    tab = stack.services.length - 1;
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
    } catch (e: any) {
      error = e.message;
    }
  });

  async function save(e: Event) {
    e.preventDefault();
    busy = 'save';
    error = '';
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

<div class="top">
  <div>
    <div class="sub">{id ? (running ? 'Running' : 'Stopped') : 'Visual compose editor — Form or YAML'}</div>
  </div>
  <div class="row">
    <button type="button" class="btn secondary" class:active={mode === 'form'} onclick={() => setMode('form')}>Form</button>
    <button type="button" class="btn secondary" class:active={mode === 'yaml'} onclick={() => setMode('yaml')}>YAML</button>
    {#if id}
      <button class="btn" disabled={!!busy} onclick={() => act('start')}>Start</button>
      <button class="btn secondary" disabled={!!busy} onclick={() => act('stop')}>Stop</button>
      <button class="btn secondary" disabled={!!busy} onclick={() => act('restart')}>Restart</button>
      {#if stack.webPort}
        <a class="btn secondary" href="{stack.scheme}://{stack.webHost || location.hostname}:{stack.webPort}{stack.webPath || '/'}" target="_blank" rel="noreferrer">Open</a>
        <button type="button" class="btn secondary" onclick={() => go(`/proxy?app=${encodeURIComponent(id)}&port=${stack.webPort}`)}>Add to Proxy</button>
      {/if}
      <button class="btn danger" onclick={remove}>Delete</button>
    {/if}
  </div>
</div>

{#if statusError}<div class="err">{statusError}</div>{/if}
{#if error}<div class="err">{error}</div>{/if}

<form onsubmit={save}>
  {#if !id}
    <label class="field"><span>Id (optional)</span><input bind:value={appId} placeholder="immich" /></label>
  {/if}
  {#if mode === 'form'}
    <section class="form-sec">
      <label class="field"><span>Title</span><input bind:value={stack.title} placeholder="Immich" required /></label>
      <label class="field"><span>Icon URL</span>
        <div class="icon-row">
          {#if stack.iconUrl}
            <img class="app-icon" src={stack.iconUrl} alt="" />
          {:else}
            <img class="app-icon" src="/icons/docker.svg" alt="" />
          {/if}
          <input bind:value={stack.iconUrl} placeholder="https://…" />
        </div>
      </label>
      <div class="field">
        <span>Web UI</span>
        <div class="webui">
          <select bind:value={stack.scheme}>
            <option value="http">http://</option>
            <option value="https">https://</option>
          </select>
          <input bind:value={stack.webHost} placeholder={typeof location !== 'undefined' ? location.hostname : 'host'} />
          <input bind:value={stack.webPort} placeholder="2283" />
          <input bind:value={stack.webPath} placeholder="/" />
        </div>
      </div>
      {#if extras.length}
        <p class="hint">Kept from YAML: {extras.join(', ')}</p>
      {/if}
    </section>

    <div class="stack-tabs" role="tablist" aria-label="Services in this stack">
      {#each stack.services as svc, i}
        <div class="stack-tab" class:active={tab === i} role="tab" aria-selected={tab === i}>
          <button type="button" class="stack-tab-name" onclick={() => (tab = i)}>
            {svc.serviceName || `service-${i + 1}`}
          </button>
          {#if stack.services.length > 1}
            <button type="button" class="stack-tab-x" title="Remove service" onclick={() => removeAt(i)}>×</button>
          {/if}
        </div>
      {/each}
      <button type="button" class="stack-tab add" onclick={add}>+ Add service</button>
    </div>

    {#key tab}
      {#if stack.services[tab]}
        <ComposeEditor bind:service={stack.services[tab]} />
      {/if}
    {/key}
  {:else}
    <label class="field"><span>compose.yml</span><textarea class="yaml-editor" bind:value={yaml} required></textarea></label>
  {/if}
  <button class="btn" disabled={!!busy}>{id ? 'Save' : 'Create'}</button>
</form>

{#if id}
  <div class="top" style="margin-top:24px">
    <h2>Logs</h2>
    <button class="btn secondary" onclick={loadLogs}>Refresh logs</button>
  </div>
  <pre class="logs">{logs || 'Click refresh to load logs.'}</pre>
{/if}
