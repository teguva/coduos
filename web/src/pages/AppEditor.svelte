<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';

  let { go, id } = $props<{ go: (to: string) => void; id?: string }>();

  const example = `services:
  whoami:
    image: traefik/whoami:v1.11
    ports:
      - "8088:80"
    restart: unless-stopped
`;

  let name = $state('whoami');
  let appId = $state('');
  let yaml = $state(example);
  let webPort = $state('');
  let logs = $state('');
  let error = $state('');
  let busy = $state('');
  let running = $state(false);
  let statusError = $state('');

  onMount(async () => {
    if (!id) return;
    try {
      const app = await api<any>('/api/apps/' + id);
      name = app.name;
      appId = app.id;
      yaml = app.compose_yaml;
      webPort = app.web_port ? String(app.web_port) : '';
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
      const body = {
        name,
        id: appId || undefined,
        compose_yaml: yaml,
        web_port: webPort ? Number(webPort) : null
      };
      const app = id
        ? await api<any>('/api/apps/' + id, { method: 'PUT', body: JSON.stringify(body) })
        : await api<any>('/api/apps', { method: 'POST', body: JSON.stringify(body) });
      go('/apps/' + app.id);
      id = app.id;
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
    <h2>{id ? name : 'Install app'}</h2>
    <div class="sub">{id ? (running ? 'Running' : 'Stopped') : 'Paste a Docker Compose file'}</div>
  </div>
  <div class="row">
    {#if id}
      <button class="btn" disabled={!!busy} onclick={() => act('start')}>Start</button>
      <button class="btn secondary" disabled={!!busy} onclick={() => act('stop')}>Stop</button>
      <button class="btn secondary" disabled={!!busy} onclick={() => act('restart')}>Restart</button>
      {#if webPort}
        <a class="btn secondary" href="http://{location.hostname}:{webPort}" target="_blank" rel="noreferrer">Open</a>
      {/if}
      <button class="btn danger" onclick={remove}>Delete</button>
    {/if}
  </div>
</div>

{#if statusError}<div class="err">{statusError}</div>{/if}
{#if error}<div class="err">{error}</div>{/if}

<form class="card" onsubmit={save}>
  <label class="field"><span>Name</span><input bind:value={name} required /></label>
  {#if !id}
    <label class="field"><span>Id (optional)</span><input bind:value={appId} placeholder="whoami" /></label>
  {/if}
  <label class="field"><span>Web port (optional)</span><input bind:value={webPort} placeholder="8088" /></label>
  <label class="field"><span>compose.yml</span><textarea bind:value={yaml} required></textarea></label>
  <button class="btn" disabled={!!busy}>{id ? 'Save' : 'Create'}</button>
</form>

{#if id}
  <div class="top" style="margin-top:24px">
    <h2>Logs</h2>
    <button class="btn secondary" onclick={loadLogs}>Refresh logs</button>
  </div>
  <pre class="logs">{logs || 'Click refresh to load logs.'}</pre>
{/if}
