<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { bytes, bps } from '../lib/format';
  import Confirm from '../components/Confirm.svelte';
  import InfoTip from '../components/InfoTip.svelte';
  import Sparkline from '../components/Sparkline.svelte';
  import UiIcon from '../components/UiIcon.svelte';

  let { go } = $props<{ go: (to: string) => void }>();

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
  type Net = { ipv4?: string | null; operstate: string; virtual_iface: boolean };
  type Sample = { rx: number; tx: number };

  const ONLINE_SECS = 180;
  const HIST = 90;

  let status = $state<Status | null>(null);
  let lanIp = $state('');
  let error = $state('');
  let notice = $state('');
  let name = $state('');
  let tunnel = $state<'lan' | 'full'>('lan');
  let qrPeer = $state<string | null>(null);
  let revokeId = $state<string | null>(null);
  let endpoint = $state('');
  let dns = $state('');
  let copied = $state('');
  let busy = $state('');
  let hist = $state<Record<string, Sample[]>>({});
  let lastBytes = $state<Record<string, { rx: number; tx: number; at: number }>>({});
  let now = $state(Date.now() / 1000);

  function applyStatus(s: Status, form = false) {
    status = s;
    recordPeers(s.peers || []);
    if (form) {
      endpoint = s.endpoint;
      dns = s.dns;
    }
  }

  function recordPeers(peers: Peer[]) {
    const t = Date.now() / 1000;
    const next = { ...hist };
    const bytesNext = { ...lastBytes };
    const ids = new Set(peers.map((p) => p.id));
    for (const id of Object.keys(next)) {
      if (!ids.has(id)) delete next[id];
    }
    for (const p of peers) {
      const prev = bytesNext[p.id];
      let rxRate = 0;
      let txRate = 0;
      if (prev) {
        const dt = Math.max(0.5, t - prev.at);
        rxRate = Math.max(0, (p.rx_bytes - prev.rx) / dt);
        txRate = Math.max(0, (p.tx_bytes - prev.tx) / dt);
      }
      bytesNext[p.id] = { rx: p.rx_bytes, tx: p.tx_bytes, at: t };
      next[p.id] = [...(next[p.id] ?? []), { rx: rxRate, tx: txRate }].slice(-HIST);
    }
    hist = next;
    lastBytes = bytesNext;
  }

  async function load(form = false) {
    applyStatus(await api<Status>('/api/vpn'), form);
  }

  onMount(() => {
    load(true).catch((e) => (error = e.message));
    api<{ networks: Net[] }>('/api/system/summary')
      .then((s) => {
        const real = (s.networks || []).filter(
          (n) => !n.virtual_iface && n.ipv4 && !n.ipv4.startsWith('127.')
        );
        lanIp = (real.find((n) => n.operstate === 'up') || real[0])?.ipv4 || '';
      })
      .catch(() => {});
    const t = setInterval(() => load(false).catch(() => {}), 2000);
    const clock = setInterval(() => (now = Date.now() / 1000), 1000);
    return () => {
      clearInterval(t);
      clearInterval(clock);
    };
  });

  async function save(body: Record<string, unknown>) {
    error = '';
    notice = '';
    busy = 'save';
    try {
      applyStatus(await api<Status>('/api/vpn', { method: 'PUT', body: JSON.stringify(body) }), true);
      notice = body.enabled === false ? 'VPN off.' : body.enabled ? 'VPN on.' : 'Saved.';
    } catch (e: any) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  async function addPeer(e: Event) {
    e.preventDefault();
    error = '';
    busy = 'add';
    try {
      await api('/api/vpn/peers', { method: 'POST', body: JSON.stringify({ name, tunnel }) });
      name = '';
      await load(false);
      notice = 'Client added. Scan the QR in WireGuard.';
    } catch (e: any) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  async function togglePeer(id: string, enabled: boolean) {
    try {
      applyStatus(
        await api<Status>(`/api/vpn/peers/${id}`, {
          method: 'POST',
          body: JSON.stringify({ enabled })
        })
      );
    } catch (e: any) {
      error = e.message;
    }
  }

  async function doRevoke(id: string) {
    try {
      applyStatus(await api<Status>(`/api/vpn/peers/${id}`, { method: 'DELETE' }));
      if (qrPeer === id) qrPeer = null;
      revokeId = null;
    } catch (e: any) {
      error = e.message;
    }
  }

  function online(p: Peer) {
    if (!status?.enabled || !p.enabled || !p.latest_handshake) return false;
    return now - p.latest_handshake < ONLINE_SECS;
  }

  function hs(ts?: number | null) {
    if (!ts) return 'Never';
    const ago = Math.max(0, now - ts);
    if (ago < 15) return 'Just now';
    if (ago < 60) return `${Math.round(ago)}s ago`;
    if (ago < 3600) return `${Math.round(ago / 60)}m ago`;
    if (ago < 86400) return `${Math.round(ago / 3600)}h ago`;
    return `${Math.round(ago / 86400)}d ago`;
  }

  function rate(p: Peer, key: 'rx' | 'tx') {
    const row = hist[p.id] ?? [];
    return row.length ? row[row.length - 1][key] : 0;
  }

  function hue(id: string) {
    let h = 0;
    for (let i = 0; i < id.length; i++) h = (h * 33 + id.charCodeAt(i)) % 360;
    return h;
  }

  function shortKey(k: string) {
    if (!k || k.length < 16) return k || '—';
    return `${k.slice(0, 8)}…${k.slice(-6)}`;
  }

  function peerAddr(p: Peer) {
    return p.address.replace(/\/\d+$/, '');
  }

  async function copy(text: string, key: string) {
    if (!text) return;
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
      } else {
        const ta = document.createElement('textarea');
        ta.value = text;
        ta.setAttribute('readonly', '');
        ta.style.position = 'fixed';
        ta.style.left = '-9999px';
        document.body.appendChild(ta);
        ta.select();
        document.execCommand('copy');
        ta.remove();
      }
      copied = key;
      setTimeout(() => {
        if (copied === key) copied = '';
      }, 1400);
    } catch {
      copied = '';
    }
  }

  let wgPort = $derived(status?.listen_port || 51820);
  let peers = $derived(status?.peers ?? []);
  let onlineCount = $derived(peers.filter((p) => online(p)).length);
  let totalRx = $derived(peers.reduce((n, p) => n + p.rx_bytes, 0));
  let totalTx = $derived(peers.reduce((n, p) => n + p.tx_bytes, 0));
  let stacked = $derived.by(() => {
    const ids = peers.map((p) => p.id);
    const len = Math.max(0, ...ids.map((id) => (hist[id] ?? []).length));
    const rx = Array.from({ length: len }, () => 0);
    const tx = Array.from({ length: len }, () => 0);
    for (const id of ids) {
      const row = hist[id] ?? [];
      const off = len - row.length;
      row.forEach((s, i) => {
        rx[off + i] += s.rx;
        tx[off + i] += s.tx;
      });
    }
    return { rx, tx };
  });
  let lanEndpoint = $derived(lanIp ? `${lanIp}:${wgPort}` : '');
  let qr = $derived(peers.find((p) => p.id === qrPeer) || null);
</script>

{#if !status?.privileged}
  <div class="banner">VPN apply needs the installed daemon as root (coduos-wg.service). You can still review this page.</div>
{/if}
{#if status?.error}<div class="err">{status.error}</div>{/if}
{#if error}<div class="err">{error}</div>{/if}
{#if notice}<p class="ok-msg">{notice}</p>{/if}

<div class="vpn-hero mini-card">
  <div class="vpn-hero-main">
    <div class="vpn-hero-title">
      <span class="dot" class:on={!!status?.enabled} class:off={!status?.enabled}></span>
      <div>
        <strong>{status?.enabled ? 'VPN is on' : 'VPN is off'}</strong>
        <div class="meta">
          WireGuard · UDP {wgPort} · {status?.address || '10.8.0.1/24'}
        </div>
      </div>
    </div>
    <button
      class="btn"
      class:secondary={!status?.enabled}
      disabled={!status?.privileged || !!busy}
      onclick={() => save({ enabled: !status?.enabled })}
    >{status?.enabled ? 'On' : 'Off'}</button>
  </div>
  <div class="vpn-hero-stats">
    <div>
      <span class="meta">Clients</span>
      <strong>{peers.length}</strong>
      <span class="meta">{onlineCount} online</span>
    </div>
    <div>
      <span class="meta">From clients</span>
      <strong class="net-rx">{bytes(totalRx)}</strong>
    </div>
    <div>
      <span class="meta">To clients</span>
      <strong class="net-tx">{bytes(totalTx)}</strong>
    </div>
  </div>
  {#if stacked.rx.length > 1}
    <div class="vpn-hero-chart">
      <Sparkline rx={stacked.rx} tx={stacked.tx} showScale />
    </div>
  {/if}
</div>

<div class="vpn-setup">
  <label class="field">
    <span class="field-head">
      Public endpoint
      <InfoTip
        label="Endpoint help"
        text="Hostname or public IPv4 of the router. CoduOS puts the IPv4 in the QR so the phone can handshake without DNS. If the ISP address changes, set that hostname under DDNS."
      />
    </span>
    <span class="vpn-input-row">
      <input
        bind:value={endpoint}
        placeholder="vpn.example.com"
        onchange={() => save({ endpoint })}
        disabled={!status?.privileged}
      />
    </span>
    <p class="hint">Listen port is added automatically. Keep this hostname in <button type="button" class="linkish" onclick={() => go('/settings/ddns')}>DDNS</button> if the ISP address changes.</p>
  </label>
  <label class="field">
    <span class="field-head">
      DNS for clients
      <InfoTip
        label="DNS help"
        text="Leave 10.8.0.1. CoduOS answers names like immich.home on the VPN. Do not use the home router (192.168.1.1): Android queries it before handshake and the tunnel never comes up. After changing DNS, delete the old tunnel on the phone and scan the QR again. Test from cellular. Turn Private DNS off on Android."
      />
    </span>
    <input
      bind:value={dns}
      placeholder="10.8.0.1"
      onchange={() => save({ dns })}
      disabled={!status?.privileged}
    />
  </label>
</div>

<details class="vpn-details">
  <summary>Port forwarding</summary>
  <p class="hint">One UDP rule on the router (Virtual Server / NAT) to this NAS. Do not forward 80 or 443.</p>
  <div class="fwd-grid">
    <button type="button" class="vpn-copy-cell" onclick={() => copy('UDP', 'proto')}>
      <span class="meta">Protocol</span>
      <strong>UDP {copied === 'proto' ? 'copied' : ''}</strong>
    </button>
    <button type="button" class="vpn-copy-cell" onclick={() => copy(String(wgPort), 'ext')}>
      <span class="meta">External port</span>
      <strong>{wgPort}</strong>
    </button>
    <button type="button" class="vpn-copy-cell" onclick={() => copy(lanIp || '', 'lan')} disabled={!lanIp}>
      <span class="meta">Internal IP</span>
      <strong>{lanIp || 'this NAS'}</strong>
    </button>
    <button type="button" class="vpn-copy-cell" onclick={() => copy(String(wgPort), 'int')}>
      <span class="meta">Internal port</span>
      <strong>{wgPort}</strong>
    </button>
  </div>
  <ul class="tips">
    <li>Reserve {lanIp || 'this NAS'} in DHCP so the forward survives a reboot.</li>
    <li>Home Wi‑Fi cannot hairpin to the public IP. On Wi‑Fi, temporarily set Endpoint to {lanEndpoint || `the NAS LAN address:${wgPort}`}. On cellular, use the QR as-is.</li>
    <li>If the WAN address is 10.x, 100.64–100.127.x, or the ISP says CGNAT, port forwarding will not work from the internet.</li>
  </ul>
  {#if status?.public_key}
    <button type="button" class="vpn-copy-cell vpn-key" onclick={() => copy(status?.public_key || '', 'pub')}>
      <span class="meta">Server public key</span>
      <strong>{copied === 'pub' ? 'Copied' : shortKey(status.public_key)}</strong>
    </button>
  {/if}
</details>

<h3 class="group-title">Clients</h3>
<form class="vpn-add" onsubmit={addPeer}>
  <input bind:value={name} placeholder="Phone" required disabled={!status?.privileged} />
  <div class="segment vpn-seg" role="group" aria-label="Tunnel type">
    <button
      type="button"
      class="btn compact"
      class:secondary={tunnel !== 'lan'}
      onclick={() => (tunnel = 'lan')}
    >LAN only</button>
    <button
      type="button"
      class="btn compact"
      class:secondary={tunnel !== 'full'}
      onclick={() => (tunnel = 'full')}
    >Full tunnel</button>
  </div>
  <button class="btn" disabled={!status?.privileged || !!busy || !name.trim()}>
    <UiIcon name="add" size={18} /> Add
  </button>
</form>
<p class="hint">
  {#if tunnel === 'full'}
    Full tunnel sends all phone traffic through the house (Firefly’s default). After connecting, open immich.home or 10.8.0.1 — not the public hostname.
  {:else}
    LAN only sends home traffic through the VPN (this NAS at 10.8.0.1 and 192.168.1.x). Cellular internet stays on the phone.
  {/if}
</p>

{#each peers as p (p.id)}
  {@const live = online(p)}
  {@const series = hist[p.id] ?? []}
  <article class="vpn-peer mini-card" class:offline={!live} class:disabled={!p.enabled}>
    <header class="vpn-peer-head">
      <div
        class="vpn-avatar"
        style="background: hsl({hue(p.id)} 42% 42%)"
        aria-hidden="true"
      >{(p.name.trim()[0] || '?').toUpperCase()}</div>
      <div class="vpn-peer-id">
        <div class="vpn-peer-name">
          <strong>{p.name}</strong>
          <span class="status-pill" class:running={live} class:error={!p.enabled}>
            <span class="dot" class:on={live} class:off={!p.enabled || !live}></span>
            {!p.enabled ? 'Disabled' : live ? 'Online' : 'Offline'}
          </span>
          <span class="chip {p.tunnel === 'full' ? 'ok' : 'muted'}">{p.tunnel === 'full' ? 'Full' : 'LAN'}</span>
        </div>
        <div class="meta">
          <button type="button" class="linkish" onclick={() => copy(peerAddr(p), p.id)}>
            {copied === p.id ? 'Copied' : peerAddr(p)}
          </button>
          · handshake {hs(p.latest_handshake)}
        </div>
      </div>
      <div class="vpn-peer-actions">
        <button class="btn secondary compact" onclick={() => (qrPeer = p.id)}>QR</button>
        <a class="btn secondary compact" href={`/api/vpn/peers/${p.id}/config`}>
          <UiIcon name="download" size={16} /> Config
        </a>
        <button
          class="btn secondary compact"
          disabled={!status?.privileged}
          onclick={() => togglePeer(p.id, !p.enabled)}
        >{p.enabled ? 'Disable' : 'Enable'}</button>
        <button
          class="btn danger compact"
          disabled={!status?.privileged}
          aria-label="Revoke {p.name}"
          onclick={() => (revokeId = p.id)}
        >
          <UiIcon name="delete" size={16} />
        </button>
      </div>
    </header>
    <div class="vpn-peer-chart">
      <Sparkline rx={series.map((s) => s.rx)} tx={series.map((s) => s.tx)} showScale />
    </div>
    <div class="vpn-peer-xfer">
      <span class="net-rx" title="Received from this client"><UiIcon name="arrow_downward" size={16} /> {bps(rate(p, 'rx'))} · {bytes(p.rx_bytes)}</span>
      <span class="net-tx" title="Sent to this client"><UiIcon name="arrow_upward" size={16} /> {bps(rate(p, 'tx'))} · {bytes(p.tx_bytes)}</span>
    </div>
  </article>
{:else}
  <div class="vpn-empty mini-card">
    <UiIcon name="vpn" size={28} />
    <div>
      <strong>No clients yet</strong>
      <div class="meta">Add a phone, then scan the QR in the official WireGuard app.</div>
    </div>
  </div>
{/each}

{#if qr}
  <div class="confirm-bg" role="presentation" onclick={(e) => { if (e.currentTarget === e.target) qrPeer = null; }}>
    <div class="confirm-card vpn-qr-card" role="dialog" aria-label="WireGuard QR for {qr.name}">
      <h3>Connect {qr.name}</h3>
      <p>Scan this in WireGuard on the phone. Delete any old tunnel first. Use cellular, not home Wi‑Fi.</p>
      <div class="qr-wrap">
        <img src={`/api/vpn/peers/${qr.id}/qr`} alt="WireGuard QR for {qr.name}" />
      </div>
      <div class="row">
        <a class="btn secondary" href={`/api/vpn/peers/${qr.id}/config`}>Download config</a>
        <button class="btn" onclick={() => (qrPeer = null)}>Done</button>
      </div>
    </div>
  </div>
{/if}

{#if revokeId}
  <Confirm
    title="Revoke client?"
    body="This client will no longer be able to connect. Add it again to make a new QR."
    confirmLabel="Revoke"
    danger
    onCancel={() => (revokeId = null)}
    onConfirm={() => doRevoke(revokeId!)}
  />
{/if}
