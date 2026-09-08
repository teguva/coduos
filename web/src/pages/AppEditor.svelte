<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { exampleForm, formToYaml, yamlToForm, type ComposeForm } from '../lib/compose';
  import ComposeEditor from '../components/ComposeEditor.svelte';

  let { go, id } = $props<{ go: (to: string) => void; id?: string }>();

  let form = $state<ComposeForm>(exampleForm());
  let yaml = $state('');
  let mode = $state<'form' | 'yaml'>('form');
  let appId = $state('');
  let logs = $state('');
  let error = $state('');
  let busy = $state('');
  let running = $state(false);
  let statusError = $state('');

  function syncYaml() {
    yaml = formToYaml(form);
  }
  function syncForm() {
    form = yamlToForm(yaml);
  }

  function setMode(next: 'form' | 'yaml') {
    if (next === 'yaml' && mode === 'form') syncYaml();
    if (next === 'form' && mode === 'yaml') syncForm();
    mode = next;
  }

  onMount(async () => {
    if (!id) {
      appId = form.serviceName;
      syncYaml();
      return;
    }
    try {
      const app = await api<any>('/api/apps/' + id);
      yaml = app.compose_yaml;
      form = yamlToForm(yaml);
      if (!form.title) form.title = app.name;
      if (app.icon_url) form.iconUrl = app.icon_url;
      if (app.web_port && !form.webPort) form.webPort = String(app.web_port);
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
        name: form.title || form.serviceName,
        id: appId || undefined,
        compose_yaml: yaml,
        icon_url: form.iconUrl || null,
        web_port: form.webPort ? Number(form.webPort) : null
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
      {#if form.webPort}
        <a class="btn secondary" href="{form.scheme}://{form.webHost || location.hostname}:{form.webPort}{form.webPath || '/'}" target="_blank" rel="noreferrer">Open</a>
        <button type="button" class="btn secondary" onclick={() => go(`/proxy?app=${encodeURIComponent(id)}&port=${form.webPort}`)}>Add to Proxy</button>
      {/if}
      <button class="btn danger" onclick={remove}>Delete</button>
    {/if}
  </div>
</div>

{#if statusError}<div class="err">{statusError}</div>{/if}
{#if error}<div class="err">{error}</div>{/if}

<form onsubmit={save}>
  {#if !id}
    <label class="field"><span>Id (optional)</span><input bind:value={appId} placeholder="jellyfin" /></label>
  {/if}
  {#if !id}
    <label class="field"><span>Service name</span><input bind:value={form.serviceName} placeholder="jellyfin" /></label>
  {/if}
  {#if mode === 'form'}
    <ComposeEditor bind:form />
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
