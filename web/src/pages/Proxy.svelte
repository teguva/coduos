<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import Confirm from '../components/Confirm.svelte';

  type Host = {
    id: string;
    hostname: string;
    target_host: string;
    target_port: number;
    tls: string;
    websocket: boolean;
    enabled: boolean;
    app_id?: string | null;
  };
  type Status = {
    privileged: boolean;
    nginx_available: boolean;
    certbot_available: boolean;
    dashboard_upstream: string;
    dashboard_tls: string;
    hosts: Host[];
    error?: string | null;
  };
  type App = { id: string; name: string; web_port?: number | null };

  let { prefill } = $props<{ prefill?: { hostname?: string; port?: number; app_id?: string } }>();

  let status = $state<Status | null>(null);
  let apps = $state<App[]>([]);
  let error = $state('');
  let notice = $state('');
  let adding = $state(false);
  let hostname = $state('');
  let targetHost = $state('127.0.0.1');
  let targetPort = $state(8080);
  let tls = $state<'off' | 'lan' | 'acme'>('off');
  let websocket = $state(true);
  let appId = $state('');
  let deleteId = $state<string | null>(null);

  async function load() {
    status = await api<Status>('/api/proxy');
    apps = await api<App[]>('/api/apps');
  }

  onMount(() => {
    if (prefill?.port) {
      adding = true;
      targetPort = prefill.port;
      appId = prefill.app_id || '';
      hostname = prefill.hostname || '';
    }
    load().catch((e) => (error = e.message));
  });

  async function applyDash(next: string) {
    error = '';
    try {
      status = await api<Status>('/api/proxy/dashboard', { method: 'PUT', body: JSON.stringify({ tls: next }) });
      notice = 'Dashboard proxy updated.';
    } catch (e: any) {
      error = e.message;
    }
  }

  async function addHost(e: Event) {
    e.preventDefault();
    error = '';
    try {
      status = await api<Status>('/api/proxy/hosts', {
        method: 'POST',
        body: JSON.stringify({
          hostname,
          target_host: targetHost,
          target_port: Number(targetPort),
          tls,
          websocket,
          app_id: appId || null
        })
      });
      adding = false;
      hostname = '';
      notice = 'Host saved.';
    } catch (err: any) {
      error = err.message;
    }
  }

  async function remove(id: string) {
    try {
      status = await api<Status>(`/api/proxy/hosts/${id}`, { method: 'DELETE' });
      deleteId = null;
    } catch (e: any) {
      error = e.message;
    }
  }

  function pickApp(id: string) {
    appId = id;
    const app = apps.find((a) => a.id === id);
    if (app?.web_port) targetPort = app.web_port;
    if (!hostname && app) hostname = `${app.id}.local`;
  }
</script>

{#if !status?.privileged}
  <div class="banner">Proxy apply needs the installed daemon as root, with nginx including CoduOS conf.d.</div>
{/if}
{#if status?.error}<div class="err">{status.error}</div>{/if}
{#if error}<div class="err">{error}</div>{/if}
{#if notice}<p>{notice}</p>{/if}

<section class="set-sec">
  <h3>Dashboard</h3>
  <p class="hint">nginx proxies this UI to {status?.dashboard_upstream ?? '127.0.0.1:13209'} on ports 80/443.</p>
  <div class="segment">
    <button class="btn secondary" class:active={status?.dashboard_tls === 'off'} disabled={!status?.privileged} onclick={() => applyDash('off')}>HTTP</button>
    <button class="btn secondary" class:active={status?.dashboard_tls === 'lan'} disabled={!status?.privileged} onclick={() => applyDash('lan')}>LAN TLS</button>
  </div>
</section>

<h3 class="group-title">Hosts</h3>
{#each status?.hosts ?? [] as h}
  <div class="mini-card">
    <div>
      <strong>{h.hostname}</strong>
      <div class="meta">{h.target_host}:{h.target_port} · TLS {h.tls}{#if h.websocket} · websocket{/if}</div>
    </div>
    <button class="btn danger" disabled={!status?.privileged} onclick={() => (deleteId = h.id)}>Remove</button>
  </div>
{/each}

{#if adding}
  <form class="card" onsubmit={addHost} style="margin-top:12px">
    <label class="field"><span>Hostname</span><input bind:value={hostname} placeholder="photos.home.arpa" required /></label>
    <label class="field"><span>App (optional)</span>
      <select value={appId} onchange={(e) => pickApp((e.currentTarget as HTMLSelectElement).value)}>
        <option value="">Custom host:port</option>
        {#each apps as a}
          <option value={a.id}>{a.name}{#if a.web_port} :{a.web_port}{/if}</option>
        {/each}
      </select>
    </label>
    <label class="field"><span>Upstream host</span><input bind:value={targetHost} required /></label>
    <label class="field"><span>Upstream port</span><input type="number" bind:value={targetPort} min="1" max="65535" required /></label>
    <label class="field"><span>TLS</span>
      <select bind:value={tls}>
        <option value="off">Off</option>
        <option value="lan">LAN self-signed</option>
        <option value="acme" disabled={!status?.certbot_available}>Let's Encrypt</option>
      </select>
    </label>
    <label class="toggle-row"><span>Websocket</span><input type="checkbox" bind:checked={websocket} /></label>
    <div class="row">
      <button class="btn">Save host</button>
      <button type="button" class="btn secondary" onclick={() => (adding = false)}>Cancel</button>
    </div>
  </form>
{:else}
  <button class="btn" onclick={() => (adding = true)}>Add host</button>
{/if}

{#if deleteId}
  <Confirm
    title="Remove host?"
    body="nginx will reload without this server block."
    confirmLabel="Remove"
    danger
    onCancel={() => (deleteId = null)}
    onConfirm={() => remove(deleteId!)}
  />
{/if}
