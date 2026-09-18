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
    apps_dir?: string;
    github_owner: string;
    github_repo: string;
    file_roots: Root[];
    file_favorites?: { root: string; path: string; label: string }[];
    units: Unit[];
    version: string;
    hostname: string;
    privileged: boolean;
  };
  type Pkg = { name: string; command: string; reason: string; installed: boolean };
  type Update = {
    current: string;
    latest?: string | null;
    html_url?: string | null;
    up_to_date: boolean;
    can_apply?: boolean;
    can_install_packages?: boolean;
    packages?: Pkg[];
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
  type DisplayOut = {
    available: boolean;
    privileged: boolean;
    off: boolean;
    ignore_lid: boolean;
    lid_available: boolean;
    hint: string;
    lid_hint: string;
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
  let installPkgs = $state(true);
  let pkgBusy = $state(false);
  let display = $state<DisplayOut | null>(null);
  let displayBusy = $state(false);
  let powerConfirm = $state<'reboot' | 'shutdown' | null>(null);
  let powerBusy = $state('');

  let active = $derived(nav.some((n) => n.id === pane) ? pane : 'general');
  const missingPkgs = $derived((update?.packages || []).filter((p) => !p.installed));

  function pkgList(pkgs: Pkg[]) {
    return pkgs.map((p) => `${p.name} (${p.reason})`).join(', ');
  }

  function updateConfirmBody() {
    const ver = update?.latest || '';
    let body = `CoduOS ${ver} will be downloaded and installed. The dashboard restarts; you stay signed in.`;
    if (installPkgs) {
      if (missingPkgs.length) {
        body += ` Also installs: ${pkgList(missingPkgs)}, and upgrades listed apt packages.`;
      } else {
        body += ' Also refreshes listed apt packages (parted, e2fsprogs, openssl, Docker Compose).';
      }
    } else if (missingPkgs.length) {
      body += ` Missing packages will not be installed: ${missingPkgs.map((p) => p.name).join(', ')}.`;
    }
    return body;
  }

  function href(id: string) {
    return id === 'general' ? '/settings' : `/settings/${id}`;
  }

  async function load() {
    settings = await api<Settings>('/api/settings');
    update = await api<Update>('/api/update');
    try {
      battery = await api<BatteryPack>('/api/system/battery');
    } catch {
      battery = null;
    }
    try {
      display = await api<DisplayOut>('/api/system/display');
    } catch {
      display = null;
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

  async function setChargeLimit(limit_pct: number) {
    if (!battery?.limit.can_set) return;
    error = '';
    notice = '';
    try {
      battery = await api<BatteryPack>('/api/system/battery', {
        method: 'POST',
        body: JSON.stringify({ limit_pct })
      });
      notice = limit_pct >= 100 ? 'Battery will charge to full.' : `Charging stops at ${limit_pct}%.`;
    } catch (err: any) {
      error = err.message;
    }
  }

  async function patchDisplay(body: { off?: boolean; ignore_lid?: boolean }, noticeText: string) {
    if (!display) return;
    error = '';
    notice = '';
    displayBusy = true;
    try {
      display = await api<DisplayOut>('/api/system/display', {
        method: 'POST',
        body: JSON.stringify(body)
      });
      notice = noticeText;
    } catch (err: any) {
      error = err.message;
    } finally {
      displayBusy = false;
    }
  }

  async function setDisplayOff(off: boolean) {
    await patchDisplay(
      { off },
      off ? 'Built-in display and backlight are off.' : 'Built-in display is on.'
    );
  }

  async function setIgnoreLid(ignore: boolean) {
    await patchDisplay(
      { ignore_lid: ignore },
      ignore ? 'Lid close is ignored.' : 'Lid close uses the system default.'
    );
  }

  async function applyUpdate() {
    error = '';
    notice = '';
    applying = true;
    confirmUpdate = false;
    try {
      const res = await api<{
        ok: boolean;
        version: string;
        restarting: boolean;
        packages_installed?: string[];
        packages_error?: string | null;
      }>('/api/update', {
        method: 'POST',
        body: JSON.stringify({ install_packages: installPkgs })
      });
      const extra = res.packages_installed?.length
        ? ` Installed ${res.packages_installed.join(', ')}.`
        : '';
      if (res.packages_error) {
        error = res.packages_error;
      }
      notice = `Installed ${res.version}.${extra} Restarting…`;
      await waitForRestart();
    } catch (err: any) {
      error = err.message;
      applying = false;
    }
  }

  async function installMissingPackages() {
    error = '';
    notice = '';
    pkgBusy = true;
    try {
      const res = await api<{
        ok: boolean;
        installed: string[];
        packages: Pkg[];
        error?: string | null;
      }>('/api/update/packages', { method: 'POST' });
      if (update) update = { ...update, packages: res.packages };
      if (res.error) {
        error = res.error;
      } else if (res.installed.length) {
        notice = `Installed ${res.installed.join(', ')}.`;
      } else {
        notice = 'Listed packages are present and were refreshed.';
      }
    } catch (err: any) {
      error = err.message;
    } finally {
      pkgBusy = false;
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
            <p class="meta">
              {joinMeta([
                b.name || null,
                b.capacity_pct != null ? `${b.capacity_pct}%` : null,
                batteryLabel(b),
                b.health_pct != null ? `health ${b.health_pct}%` : null,
                b.cycle_count != null ? `${b.cycle_count} cycles` : null
              ])}
            </p>
          {/each}
          {#if battery.limit.supported}
            {#if !battery.privileged}
              <div class="banner">Changing this needs the installed daemon running as root.</div>
            {/if}
            <p class="hint">Stop charging at this level while plugged in. 80% is better if the laptop stays on power.</p>
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
          {:else}
            <p class="hint">This battery cannot be charge-limited from CoduOS.</p>
          {/if}
        </section>
      {/if}

      {#if display?.available || display?.lid_available}
        <section class="set-block" id="display">
          <h3>Laptop</h3>
          {#if !display.privileged}
            <div class="banner">These laptop options need the installed daemon running as root.</div>
          {/if}
          {#if display.available}
            <label class="toggle-row">
              <span>Switch off built-in display and backlight</span>
              <input
                type="checkbox"
                checked={display.off}
                disabled={!display.privileged || displayBusy}
                onchange={(e) => setDisplayOff(e.currentTarget.checked)}
              />
            </label>
            <p class="hint">{display.hint}</p>
          {/if}
          {#if display.lid_available}
            <label class="toggle-row">
              <span>Do not react to lid close</span>
              <input
                type="checkbox"
                checked={display.ignore_lid}
                disabled={!display.privileged || displayBusy}
                onchange={(e) => setIgnoreLid(e.currentTarget.checked)}
              />
            </label>
            <p class="hint">{display.lid_hint}</p>
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
          {#if missingPkgs.length}
            <p class="hint">Missing packages: {pkgList(missingPkgs)}</p>
          {:else if update.packages?.length}
            <p class="hint">Packages: {update.packages.map((p) => p.name).join(', ')} — all installed. Update refreshes them via apt.</p>
          {/if}
          {#if update.can_install_packages}
            {#if !update.up_to_date && !update.error}
              <label class="toggle-row">
                <span>Install missing packages and update listed apt packages</span>
                <input type="checkbox" bind:checked={installPkgs} disabled={applying || pkgBusy} />
              </label>
            {:else}
              <button class="btn secondary" disabled={pkgBusy} onclick={installMissingPackages}>
                {pkgBusy ? 'Updating packages…' : missingPkgs.length ? 'Install missing packages' : 'Update listed packages'}
              </button>
            {/if}
          {:else if !settings?.privileged && (missingPkgs.length || update.packages?.length)}
            <p class="hint">Installing packages needs the installed daemon running as root.</p>
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
            <dt>Apps dir</dt><dd>{settings.apps_dir || '—'}</dd>
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
    body={updateConfirmBody()}
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
