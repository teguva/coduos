<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';

  type Status = {
    privileged: boolean;
    enabled: boolean;
    provider: string;
    hostname: string;
    zone: string;
    username: string;
    token_set: boolean;
    password_set: boolean;
    generic_url: string;
    interval_secs: number;
    ipv6: boolean;
    proxied: boolean;
    last_ipv4: string;
    last_ipv6: string;
    last_ok_at: string;
    last_checked_at: string;
    last_error?: string | null;
    last_result: string;
  };

  let { go } = $props<{ go: (to: string) => void }>();

  let status = $state<Status | null>(null);
  let error = $state('');
  let notice = $state('');
  let busy = $state('');
  let hostname = $state('');
  let zone = $state('');
  let username = $state('');
  let token = $state('');
  let password = $state('');
  let genericUrl = $state('');
  let provider = $state('cloudflare');
  let intervalMin = $state(5);
  let ipv6 = $state(false);
  let proxied = $state(false);

  const providers = [
    { id: 'cloudflare', label: 'Cloudflare' },
    { id: 'duckdns', label: 'DuckDNS' },
    { id: 'noip', label: 'No-IP' },
    { id: 'dynu', label: 'Dynu' },
    { id: 'namecheap', label: 'Namecheap' },
    { id: 'generic', label: 'HTTP URL' }
  ];

  function applyStatus(s: Status, form = false) {
    status = s;
    if (!form) return;
    hostname = s.hostname;
    zone = s.zone;
    username = s.username;
    genericUrl = s.generic_url;
    provider = s.provider || 'cloudflare';
    intervalMin = Math.max(1, Math.round((s.interval_secs || 300) / 60));
    ipv6 = s.ipv6;
    proxied = s.proxied;
    token = '';
    password = '';
  }

  async function load(form = false) {
    applyStatus(await api<Status>('/api/ddns'), form);
  }

  onMount(() => {
    load(true).catch((e) => (error = e.message));
    const t = setInterval(() => load(false).catch(() => {}), 8000);
    return () => clearInterval(t);
  });

  async function save(extra: Record<string, unknown> = {}) {
    error = '';
    notice = '';
    busy = 'save';
    try {
      const body: Record<string, unknown> = {
        provider,
        hostname,
        zone,
        username,
        generic_url: genericUrl,
        interval_secs: Math.round(intervalMin * 60),
        ipv6,
        proxied,
        ...extra
      };
      if (token.trim()) body.token = token.trim();
      if (password.trim()) body.password = password.trim();
      applyStatus(await api<Status>('/api/ddns', { method: 'PUT', body: JSON.stringify(body) }), true);
      notice = extra.enabled === false ? 'Updater off.' : extra.enabled ? 'Updater on.' : 'Saved.';
    } catch (e: any) {
      error = e.message;
      await load(true).catch(() => {});
    } finally {
      busy = '';
    }
  }

  async function refresh() {
    error = '';
    notice = '';
    busy = 'refresh';
    try {
      applyStatus(await api<Status>('/api/ddns/refresh', { method: 'POST' }), true);
      notice = status?.last_result === 'unchanged' ? 'Address unchanged.' : 'DNS updated.';
    } catch (e: any) {
      error = e.message;
      await load(true).catch(() => {});
    } finally {
      busy = '';
    }
  }

  function resultLabel(s: Status | null) {
    if (!s?.last_result) return 'Not run yet';
    if (s.last_result === 'ok') return 'Updated';
    if (s.last_result === 'unchanged') return 'Unchanged';
    if (s.last_result === 'error') return 'Failed';
    return s.last_result;
  }

  function when(iso: string) {
    if (!iso) return '—';
    const d = new Date(iso);
    return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
  }
</script>

<p class="hint">Keeps a hostname pointed at this NAS when the ISP address changes. Used for VPN, Proxy, and remote access — no extra Docker app.</p>

<div class="toggle-row">
  <div>
    <strong>Enable updater</strong>
    <div class="meta">{status?.enabled ? `Every ${intervalMin} min` : 'Off'} · {resultLabel(status)}</div>
  </div>
  <button
    class="btn"
    class:secondary={!status?.enabled}
    disabled={!!busy}
    onclick={() => save({ enabled: !status?.enabled })}
  >{status?.enabled ? 'On' : 'Off'}</button>
</div>

{#if status?.last_error}<div class="err">{status.last_error}</div>{/if}
{#if error}<div class="err">{error}</div>{/if}
{#if notice}<p>{notice}</p>{/if}

<div class="fwd-grid">
  <div><span class="meta">Public IPv4</span><strong>{status?.last_ipv4 || '—'}</strong></div>
  <div><span class="meta">Last success</span><strong>{when(status?.last_ok_at || '')}</strong></div>
  <div><span class="meta">Last check</span><strong>{when(status?.last_checked_at || '')}</strong></div>
  <div><span class="meta">IPv6</span><strong>{status?.last_ipv6 || (status?.ipv6 ? 'looking up' : 'off')}</strong></div>
</div>

<form
  onsubmit={(e) => {
    e.preventDefault();
    save();
  }}
>
  <label class="field"><span>Provider</span>
    <select bind:value={provider}>
      {#each providers as p}
        <option value={p.id}>{p.label}</option>
      {/each}
    </select>
  </label>

  {#if provider !== 'generic'}
    <label class="field"><span>Hostname</span>
      <input bind:value={hostname} placeholder={provider === 'duckdns' ? 'home or home.duckdns.org' : 'nas.example.com'} required={provider !== 'generic'} />
    </label>
  {/if}

  {#if provider === 'cloudflare'}
    <label class="field"><span>Zone (optional)</span>
      <input bind:value={zone} placeholder="example.com — blank uses the hostname" />
    </label>
    <label class="field"><span>API token {status?.token_set ? '(saved)' : ''}</span>
      <input type="password" bind:value={token} placeholder={status?.token_set ? 'leave blank to keep' : 'Zone DNS edit token'} autocomplete="off" />
    </label>
    <label class="toggle-row"><span>Cloudflare proxy (orange cloud)</span>
      <input type="checkbox" bind:checked={proxied} />
    </label>
    <p class="hint">Leave proxy off if this hostname is the WireGuard endpoint. Token needs Zone DNS Edit.</p>
  {:else if provider === 'duckdns'}
    <label class="field"><span>Token {status?.token_set ? '(saved)' : ''}</span>
      <input type="password" bind:value={token} placeholder={status?.token_set ? 'leave blank to keep' : 'DuckDNS token'} autocomplete="off" />
    </label>
  {:else if provider === 'noip'}
    <label class="field"><span>Username</span>
      <input bind:value={username} placeholder="No-IP account email" autocomplete="username" />
    </label>
    <label class="field"><span>Password {status?.password_set ? '(saved)' : ''}</span>
      <input type="password" bind:value={password} placeholder={status?.password_set ? 'leave blank to keep' : 'No-IP password'} autocomplete="off" />
    </label>
  {:else if provider === 'dynu'}
    <label class="field"><span>Username (optional)</span>
      <input bind:value={username} placeholder="Dynu username" />
    </label>
    <label class="field"><span>Password or API key {status?.password_set || status?.token_set ? '(saved)' : ''}</span>
      <input type="password" bind:value={password} placeholder="leave blank to keep" autocomplete="off" />
    </label>
  {:else if provider === 'namecheap'}
    <label class="field"><span>Dynamic DNS password {status?.password_set ? '(saved)' : ''}</span>
      <input type="password" bind:value={password} placeholder={status?.password_set ? 'leave blank to keep' : 'from Namecheap DNS'} autocomplete="off" />
    </label>
    <p class="hint">Hostname is the full record, e.g. nas.example.com. Dynamic DNS must be enabled on the domain.</p>
  {:else}
    <label class="field"><span>Update URL</span>
      <input bind:value={genericUrl} placeholder="https://example.com/nic/update?hostname={hostname}&myip={ip}" />
    </label>
    <p class="hint">Placeholders: {'{ip}'}, {'{ipv4}'}, {'{ipv6}'}, {'{hostname}'}.</p>
  {/if}

  <label class="field"><span>Check every (minutes)</span>
    <input type="number" min="1" max="60" bind:value={intervalMin} />
  </label>
  <label class="toggle-row">
    <span>Also update AAAA (IPv6)</span>
    <input type="checkbox" bind:checked={ipv6} />
  </label>

  <div class="row">
    <button class="btn" disabled={!!busy}>Save</button>
    <button type="button" class="btn secondary" disabled={!!busy || !status?.enabled} onclick={refresh}>
      {busy === 'refresh' ? 'Updating…' : 'Update now'}
    </button>
    <button type="button" class="btn secondary" onclick={() => go('/settings/vpn')}>VPN endpoint</button>
  </div>
</form>
