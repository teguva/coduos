<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { bytes } from '../lib/format';
  import Confirm from '../components/Confirm.svelte';

  type Peer = {
    id: string;
    name: string;
    public_key: string;
    address: string;
    enabled: boolean;
    tunnel: string;
    latest_handshake?: number | null;
    rx_bytes: number;
    tx_bytes: number;
  };
  type Status = {
    privileged: boolean;
    wg_available: boolean;
    enabled: boolean;
    listen_port: number;
    endpoint: string;
    dns: string;
    public_key: string;
    address: string;
    peers: Peer[];
    error?: string | null;
  };

  let status = $state<Status | null>(null);
  let error = $state('');
  let notice = $state('');
  let name = $state('');
  let tunnel = $state<'lan' | 'full'>('lan');
  let qrPeer = $state<string | null>(null);
  let revokeId = $state<string | null>(null);

  async function load() {
    status = await api<Status>('/api/vpn');
  }

  onMount(() => {
    load().catch((e) => (error = e.message));
    const t = setInterval(() => load().catch(() => {}), 5000);
    return () => clearInterval(t);
  });

  async function save(body: Record<string, unknown>) {
    error = '';
    try {
      status = await api<Status>('/api/vpn', { method: 'PUT', body: JSON.stringify(body) });
      notice = 'Saved.';
    } catch (e: any) {
      error = e.message;
    }
  }

  async function addPeer(e: Event) {
    e.preventDefault();
    error = '';
    try {
      await api('/api/vpn/peers', { method: 'POST', body: JSON.stringify({ name, tunnel }) });
      name = '';
      await load();
    } catch (e: any) {
      error = e.message;
    }
  }

  async function togglePeer(id: string, enabled: boolean) {
    try {
      status = await api<Status>(`/api/vpn/peers/${id}`, {
        method: 'POST',
        body: JSON.stringify({ enabled })
      });
    } catch (e: any) {
      error = e.message;
    }
  }

  async function doRevoke(id: string) {
    try {
      status = await api<Status>(`/api/vpn/peers/${id}`, { method: 'DELETE' });
      revokeId = null;
    } catch (e: any) {
      error = e.message;
    }
  }

  function hs(ts?: number | null) {
    if (!ts) return 'never';
    const ago = Math.max(0, Date.now() / 1000 - ts);
    if (ago < 120) return 'just now';
    if (ago < 3600) return `${Math.round(ago / 60)}m ago`;
    return `${Math.round(ago / 3600)}h ago`;
  }
</script>

{#if !status?.privileged}
  <div class="banner">VPN apply needs the installed daemon as root (coduos-wg.service). You can still review this page.</div>
{/if}
{#if status?.error}<div class="err">{status.error}</div>{/if}
{#if error}<div class="err">{error}</div>{/if}
{#if notice}<p>{notice}</p>{/if}

<div class="toggle-row">
  <div>
    <strong>Enable VPN</strong>
    <div class="meta">WireGuard UDP {status?.listen_port ?? 51820} · {status?.address ?? ''}</div>
  </div>
  <button
    class="btn"
    class:secondary={!status?.enabled}
    disabled={!status?.privileged}
    onclick={() => save({ enabled: !status?.enabled })}
  >{status?.enabled ? 'On' : 'Off'}</button>
</div>

<label class="field"><span>Public endpoint</span>
  <input
    value={status?.endpoint ?? ''}
    placeholder="nas.example.com"
    onchange={(e) => save({ endpoint: (e.currentTarget as HTMLInputElement).value })}
    disabled={!status?.privileged}
  />
</label>
<label class="field"><span>DNS for clients</span>
  <input
    value={status?.dns ?? ''}
    placeholder="1.1.1.1"
    onchange={(e) => save({ dns: (e.currentTarget as HTMLInputElement).value })}
    disabled={!status?.privileged}
  />
</label>

<h3 class="group-title">Clients</h3>
<form class="row" onsubmit={addPeer} style="margin-bottom:12px">
  <input bind:value={name} placeholder="Phone" required disabled={!status?.privileged} />
  <select bind:value={tunnel} disabled={!status?.privileged}>
    <option value="lan">LAN only</option>
    <option value="full">Full tunnel</option>
  </select>
  <button class="btn" disabled={!status?.privileged}>Add</button>
</form>

{#each status?.peers ?? [] as p}
  <div class="mini-card">
    <div>
      <strong>{p.name}</strong>
      <div class="meta">{p.address} · {p.tunnel === 'full' ? 'Full tunnel' : 'LAN only'} · handshake {hs(p.latest_handshake)} · ↓{bytes(p.rx_bytes)} ↑{bytes(p.tx_bytes)}</div>
    </div>
    <div class="row">
      <button class="btn secondary" onclick={() => (qrPeer = qrPeer === p.id ? null : p.id)}>QR</button>
      <a class="btn secondary" href={`/api/vpn/peers/${p.id}/config`}>Download</a>
      <button class="btn secondary" disabled={!status?.privileged} onclick={() => togglePeer(p.id, !p.enabled)}>{p.enabled ? 'Disable' : 'Enable'}</button>
      <button class="btn danger" disabled={!status?.privileged} onclick={() => (revokeId = p.id)}>Revoke</button>
    </div>
  </div>
  {#if qrPeer === p.id}
    <div class="qr-wrap">
      <img src={`/api/vpn/peers/${p.id}/qr`} alt="WireGuard QR for {p.name}" />
    </div>
  {/if}
{/each}

{#if revokeId}
  <Confirm
    title="Revoke client?"
    body="This client will no longer be able to connect."
    confirmLabel="Revoke"
    danger
    onCancel={() => (revokeId = null)}
    onConfirm={() => doRevoke(revokeId!)}
  />
{/if}
