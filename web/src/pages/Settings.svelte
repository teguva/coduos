<script lang="ts">
  import { onMount } from 'svelte';
  import { api, isLight, setTheme } from '../lib/api';
  import { applyPower as sendPower } from '../lib/power';
  import { batteryLabel, joinMeta } from '../lib/format';
  import UiIcon from '../components/UiIcon.svelte';
  import AppWindow from '../components/AppWindow.svelte';
  import Confirm from '../components/Confirm.svelte';
  import Storage from './Storage.svelte';
  import Network from './Network.svelte';
  import Vpn from './Vpn.svelte';
  import Ddns from './Ddns.svelte';
  import Proxy from './Proxy.svelte';
  import Services from './Services.svelte';

  let {
    pane = 'general',
    go,
    prefill,
    onClose
  } = $props<{
    pane?: string;
    go: (to: string) => void;
    prefill?: { hostname?: string; port?: number; app_id?: string };
    onClose: () => void;
  }>();

  type Root = { id: string; label: string; path: string };
  type Unit = { id: string; unit: string; label: string };
  type Settings = {
    bind: string;
    data_dir: string;
    github_owner: string;
    github_repo: string;
    file_roots: Root[];
    file_favorites?: { root: string; path: string; label: string }[];
    units: Unit[];
    version: string;
    hostname: string;
    privileged: boolean;
  };
  type Update = {
    current: string;
    latest?: string | null;
    html_url?: string | null;
    up_to_date: boolean;
    can_apply?: boolean;
    error?: string | null;
  };
  type BatteryLimit = {
    supported: boolean;
    kind?: string | null;
    can_set: boolean;
    limit_pct?: number | null;
    start_pct?: number | null;
    min_pct: number;
    max_pct: number;
    presets: number[];
    charge_type?: string | null;
    conservation?: boolean | null;
    hint?: string | null;
  };
  type BatteryPack = {
    present: boolean;
    privileged: boolean;
    ac_online: boolean;
    batteries: {
      id: string;
      name: string;
      capacity_pct?: number | null;
      status: string;
      charging: boolean;
      ac_online: boolean;
      power_w?: number | null;
      health_pct?: number | null;
      cycle_count?: number | null;
      limit_pct?: number | null;
      start_pct?: number | null;
    }[];
    limit: BatteryLimit;
  };

  const nav = [
    { id: 'general', label: 'General', icon: 'settings' },
    { id: 'storage', label: 'Storage', icon: 'storage' },
    { id: 'network', label: 'Network', icon: 'network' },
    { id: 'vpn', label: 'VPN', icon: 'vpn' },
    { id: 'ddns', label: 'DDNS', icon: 'ddns' },
    { id: 'proxy', label: 'Proxy', icon: 'proxy' },
    { id: 'services', label: 'Services', icon: 'services' }
  ];

  let settings = $state<Settings | null>(null);
  let update = $state<Update | null>(null);
  let battery = $state<BatteryPack | null>(null);
  let error = $state('');
  let notice = $state('');
  let currentPw = $state('');
  let newPw = $state('');
  let confirmPw = $state('');
  let showPw = $state(false);
  let light = $state(isLight());
  let newRoot = $state({ id: '', label: '', path: '' });
  let addingRoot = $state(false);
  let applying = $state(false);
  let confirmUpdate = $state(false);
  let chargeDraft = $state(80);
  let startDraft = $state(75);
  let powerConfirm = $state<'reboot' | 'shutdown' | null>(null);
  let powerBusy = $state('');

  let active = $derived(nav.some((n) => n.id === pane) ? pane : 'general');

  function href(id: string) {
    return id === 'general' ? '/settings' : `/settings/${id}`;
  }

  async function load() {
    settings = await api<Settings>('/api/settings');
    update = await api<Update>('/api/update');
    try {
      battery = await api<BatteryPack>('/api/system/battery');
      if (battery?.limit.limit_pct != null) chargeDraft = battery.limit.limit_pct;
      if (battery?.limit.start_pct != null) startDraft = battery.limit.start_pct;
    } catch {
      battery = null;
    }
  }

  onMount(() => load().catch((e) => (error = e.message)));

  async function save(file_roots: Root[], units: Unit[]) {
    error = '';
    notice = '';
    settings = await api<Settings>('/api/settings', {
      method: 'PUT',
      body: JSON.stringify({ file_roots, units, file_favorites: settings?.file_favorites })
    });
    notice = 'Saved.';
  }

  async function addRoot(e: Event) {
    e.preventDefault();
    if (!settings) return;
    const id = (newRoot.id || newRoot.label).toLowerCase().replace(/[^a-z0-9-]+/g, '-').replace(/^-|-$/g, '');
    const root = { id, label: newRoot.label.trim(), path: newRoot.path.trim() };
    if (!root.id || !root.label || !root.path.startsWith('/')) {
      error = 'Need a label and an absolute path.';
      return;
    }
    try {
      await save([...settings.file_roots, root], settings.units);
      newRoot = { id: '', label: '', path: '' };
      addingRoot = false;
    } catch (err: any) {
      error = err.message;
    }
  }

  async function removeRoot(id: string) {
    if (!settings) return;
    try {
      await save(
        settings.file_roots.filter((r) => r.id !== id),
        settings.units
      );
    } catch (err: any) {
      error = err.message;
    }
  }

  async function changePw(e: Event) {
    e.preventDefault();
    error = '';
    notice = '';
    if (newPw !== confirmPw) {
      error = 'New passwords do not match.';
      return;
    }
    try {
      await api('/api/settings/password', {
        method: 'PUT',
        body: JSON.stringify({ current: currentPw, new_password: newPw })
      });
      currentPw = '';
      newPw = '';
      confirmPw = '';
      notice = 'Password updated.';
    } catch (err: any) {
      error = err.message;
    }
  }

  function theme(next: 'light' | 'dark') {
    setTheme(next);
    light = isLight();
  }

  async function waitForRestart() {
    notice = 'Restarting CoduOS…';
    for (let i = 0; i < 40; i++) {
      await new Promise((r) => setTimeout(r, 1000));
      try {
        const res = await fetch('/api/setup/status', { credentials: 'include' });
        if (res.ok) {
          location.reload();
          return;
        }
      } catch {
        /* still down */
      }
    }
    location.reload();
  }

  async function setChargeLimit(limit_pct: number, start_pct?: number | null) {
    if (!battery?.limit.can_set) return;
    error = '';
    notice = '';
    const dual = battery.limit.kind === 'thresholds';
    let start = dual ? (start_pct ?? startDraft) : undefined;
    if (dual && start != null && start >= limit_pct) start = Math.max(0, limit_pct - 1);
    try {
      battery = await api<BatteryPack>('/api/system/battery', {
        method: 'POST',
        body: JSON.stringify({ limit_pct, start_pct: start })
      });
      if (battery.limit.limit_pct != null) chargeDraft = battery.limit.limit_pct;
      if (battery.limit.start_pct != null) startDraft = battery.limit.start_pct;
      if (limit_pct >= 100) {
        notice = 'Charge limit removed (full).';
      } else if (battery.limit.start_pct != null) {
        notice = `Charge ${battery.limit.start_pct}–${limit_pct}%.`;
      } else {
        notice = `Charge limit set to ${limit_pct}%.`;
      }
    } catch (err: any) {
      error = err.message;
    }
  }

  async function applyUpdate() {
    error = '';
    notice = '';
    applying = true;
    confirmUpdate = false;
    try {
      const res = await api<{ ok: boolean; version: string; restarting: boolean }>('/api/update', {
        method: 'POST'
      });
      notice = `Installed ${res.version}. Restarting…`;
      await waitForRestart();
    } catch (err: any) {
      error = err.message;
      applying = false;
    }
  }

  async function applyPower(action: 'reboot' | 'shutdown') {
    error = '';
    notice = '';
    powerBusy = action;
    powerConfirm = null;
    try {
      await sendPower(action);
    } catch (err: any) {
      error = err.message;
      powerBusy = '';
    }
  }
</script>

<AppWindow title="Settings" icon="settings" size="sheet" flush {onClose}>
<div class="set-shell">
  <nav class="set-nav" aria-label="Settings">
    {#each nav as n}
      <button class="loc" class:active={active === n.id} onclick={() => go(href(n.id))}>
        <UiIcon name={n.icon} size={20} />
        {n.label}
      </button>
    {/each}
  </nav>
  <div class="set-pane">
    {#if error}<div class="err">{error}</div>{/if}
    {#if notice}<p class="hint">{notice}</p>{/if}

    {#if active === 'general'}
      <section class="set-block">
        <h3>Appearance</h3>
        <div class="segment">
          <button class="btn secondary" class:active={!light} onclick={() => theme('dark')}>
            <UiIcon name="dark_mode" size={18} /> Dark
          </button>
          <button class="btn secondary" class:active={light} onclick={() => theme('light')}>
            <UiIcon name="light_mode" size={18} /> Light
          </button>
        </div>
      </section>

      <section class="set-block">
        <h3>Account</h3>
        <form onsubmit={changePw}>
          <label class="field"><span>Current password</span>
            <input type={showPw ? 'text' : 'password'} bind:value={currentPw} required />
          </label>
          <label class="field"><span>New password</span>
            <input type={showPw ? 'text' : 'password'} bind:value={newPw} minlength="8" required />
          </label>
          <label class="field"><span>Confirm new password</span>
            <input type={showPw ? 'text' : 'password'} bind:value={confirmPw} minlength="8" required />
          </label>
          <label class="toggle-row">
            <span>Show passwords</span>
            <input type="checkbox" bind:checked={showPw} />
          </label>
          <button class="btn">Update password</button>
        </form>
      </section>

      {#if battery?.present}
        <section class="set-block" id="battery">
          <h3>Battery</h3>
          {#each battery.batteries as b}
            <div class="mini-card">
              <div>
                <strong>{b.name}</strong>
                <div class="meta">
                  {joinMeta([
                    b.capacity_pct != null ? `${b.capacity_pct}%` : null,
                    batteryLabel(b),
                    b.health_pct != null ? `health ${b.health_pct}%` : null,
                    b.cycle_count != null ? `${b.cycle_count} cycles` : null
                  ])}
                </div>
              </div>
            </div>
          {/each}
          {#if !battery.privileged}
            <div class="banner">Charge limits need the installed daemon running as root.</div>
          {/if}
          {#if battery.limit.supported}
            <div class="segment">
              {#each battery.limit.presets as p}
                <button
                  class="btn secondary"
                  class:active={battery.limit.limit_pct === p}
                  disabled={!battery.limit.can_set}
                  onclick={() => setChargeLimit(p)}
                >
                  {p >= 100 ? 'Full' : `${p}%`}
                </button>
              {/each}
            </div>
            {#if battery.limit.kind === 'thresholds'}
              <label class="field range">
                <span>Start charging below {startDraft}%</span>
                <input
                  type="range"
                  min="0"
                  max={Math.max(0, chargeDraft - 1)}
                  step="1"
                  bind:value={startDraft}
                  disabled={!battery.limit.can_set}
                  onchange={() => setChargeLimit(chargeDraft, startDraft)}
                />
              </label>
              <label class="field range">
                <span>Stop charging at {chargeDraft}%</span>
                <input
                  type="range"
                  min={Math.max(battery.limit.min_pct, startDraft + 1)}
                  max={battery.limit.max_pct}
                  step="1"
                  bind:value={chargeDraft}
                  disabled={!battery.limit.can_set}
                  onchange={() => setChargeLimit(chargeDraft, startDraft)}
                />
              </label>
            {:else if battery.limit.kind === 'end_only'}
              <label class="field range">
                <span>Stop charging at {chargeDraft}%</span>
                <input
                  type="range"
                  min={battery.limit.min_pct}
                  max={battery.limit.max_pct}
                  step="1"
                  bind:value={chargeDraft}
                  disabled={!battery.limit.can_set}
                  onchange={() => setChargeLimit(chargeDraft)}
                />
              </label>
            {/if}
            {#if battery.limit.hint}
              <p class="hint">{battery.limit.hint}</p>
            {/if}
          {:else}
            <p class="hint">This machine has a battery, but no charge limiter in sysfs.</p>
          {/if}
        </section>
      {/if}

      <section class="set-block">
        <h3>Files locations</h3>
        <p class="hint">Folders shown in the Files app.</p>
        <div class="card-list">
          {#each settings?.file_roots ?? [] as r}
            <div class="mini-card">
              <div>
                <strong>{r.label}</strong>
                <div class="meta">{r.path}</div>
              </div>
              <button class="btn secondary" onclick={() => removeRoot(r.id)}>Remove</button>
            </div>
          {/each}
        </div>
        {#if addingRoot}
          <form onsubmit={addRoot}>
            <label class="field"><span>Label</span><input bind:value={newRoot.label} placeholder="Media" required /></label>
            <label class="field"><span>Absolute path</span><input bind:value={newRoot.path} placeholder="/mnt/media" required /></label>
            <div class="row">
              <button class="btn">Add location</button>
              <button type="button" class="btn secondary" onclick={() => (addingRoot = false)}>Cancel</button>
            </div>
          </form>
        {:else}
          <button class="btn secondary" onclick={() => (addingRoot = true)}>Add location</button>
        {/if}
      </section>

      <section class="set-block">
        <h3>Updates</h3>
        {#if update}
          <div class="row">
            {#if update.error}
              <span class="chip warn">Check failed</span>
            {:else if update.up_to_date}
              <span class="chip ok">Up to date</span>
            {:else}
              <span class="chip warn">Update available</span>
            {/if}
            <span class="meta">Current {update.current}{#if update.latest} · latest {update.latest}{/if}</span>
          </div>
          {#if update.error}<div class="err">{update.error}</div>{/if}
          {#if !update.up_to_date && !update.error}
            {#if !settings?.privileged}
              <div class="banner">Updating from here needs the installed daemon running as root.</div>
            {:else if update.can_apply === false}
              <p class="hint">This copy is not the installed service. On the NAS, run <code>scripts/update.sh</code> as root.</p>
            {/if}
            <div class="row">
              <button
                class="btn"
                disabled={!update.can_apply || applying}
                onclick={() => (confirmUpdate = true)}
              >
                {applying ? 'Updating…' : `Update to ${update.latest}`}
              </button>
              {#if update.html_url}
                <a class="btn secondary" href={update.html_url} target="_blank" rel="noreferrer">Release notes</a>
              {/if}
            </div>
          {:else if update.html_url}
            <a class="btn secondary" href={update.html_url} target="_blank" rel="noreferrer">Open GitHub release</a>
          {/if}
          <p class="hint">github.com/{settings?.github_owner}/{settings?.github_repo}</p>
        {/if}
      </section>

      <section class="set-block">
        <h3>Power</h3>
        {#if !settings?.privileged}
          <div class="banner">Reboot and shutdown need the installed daemon running as root.</div>
        {/if}
        <div class="row">
          <button
            class="btn secondary"
            disabled={!settings?.privileged || !!powerBusy}
            onclick={() => (powerConfirm = 'reboot')}
          >
            <UiIcon name="power" size={18} />
            {powerBusy === 'reboot' ? 'Rebooting…' : 'Reboot'}
          </button>
          <button
            class="btn danger"
            disabled={!settings?.privileged || !!powerBusy}
            onclick={() => (powerConfirm = 'shutdown')}
          >
            <UiIcon name="power" size={18} />
            {powerBusy === 'shutdown' ? 'Shutting down…' : 'Shut down'}
          </button>
        </div>
      </section>

      {#if settings}
        <section class="set-block">
          <h3>About</h3>
          <dl class="about">
            <dt>Version</dt><dd>{settings.version}</dd>
            <dt>Hostname</dt><dd>{settings.hostname}</dd>
            <dt>Bind</dt><dd>{settings.bind}</dd>
            <dt>Data dir</dt><dd>{settings.data_dir}</dd>
          </dl>
          <p class="hint">When nginx is in front, the dashboard is on ports 80 and 443.</p>
        </section>
      {/if}
    {:else if active === 'storage'}
      <Storage {go} />
    {:else if active === 'network'}
      <Network />
    {:else if active === 'vpn'}
      <Vpn {go} />
    {:else if active === 'ddns'}
      <Ddns {go} />
    {:else if active === 'proxy'}
      <Proxy {prefill} />
    {:else if active === 'services'}
      <Services />
    {/if}
  </div>
</div>
</AppWindow>

{#if confirmUpdate && update?.latest}
  <Confirm
    title="Install update?"
    body={`CoduOS ${update.latest} will be downloaded and installed. The dashboard restarts; you stay signed in.`}
    confirmLabel={`Update to ${update.latest}`}
    onCancel={() => (confirmUpdate = false)}
    onConfirm={applyUpdate}
  />
{/if}
{#if powerConfirm === 'reboot'}
  <Confirm
    title="Reboot this computer?"
    body="CoduOS and every running app will stop. The machine starts again on its own."
    confirmLabel="Reboot"
    onCancel={() => (powerConfirm = null)}
    onConfirm={() => applyPower('reboot')}
  />
{/if}
{#if powerConfirm === 'shutdown'}
  <Confirm
    title="Shut down this computer?"
    body="The machine will power off. You will need to turn it on again at the device."
    confirmLabel="Shut down"
    danger
    onCancel={() => (powerConfirm = null)}
    onConfirm={() => applyPower('shutdown')}
  />
{/if}
