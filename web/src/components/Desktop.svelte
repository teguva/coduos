<script lang="ts">
  import { onMount } from 'svelte';
  import { api, toggleTheme, isLight } from '../lib/api';
  import {
    isBusy,
    isLaunchable,
    overlayJob,
    subscribeAppJobs,
    type AppJob,
    type AppRecord
  } from '../lib/apps';
  import { applyPower as sendPower } from '../lib/power';
  import { bytes, bps, pct, uptime, shortOs, prettyGpu, watts, joinMeta, batteryLabel } from '../lib/format';
  import { appIcons, appIcon, iconRev } from '../lib/icons';
  import BrandLogo from './BrandLogo.svelte';
  import Icon from './Icon.svelte';
  import UiIcon from './UiIcon.svelte';
  import ProgressStrip from './ProgressStrip.svelte';
  import StatusPill from './StatusPill.svelte';
  import Sparkline from './Sparkline.svelte';
  import Confirm from './Confirm.svelte';

  let { username, go, onLogout, path } = $props<{
    username: string;
    go: (to: string) => void;
    onLogout: () => void;
    path: string;
  }>();

  type Net = {
    name: string;
    rx_bps: number;
    tx_bps: number;
    ipv4?: string | null;
    operstate: string;
    speed_mbps?: number | null;
    virtual_iface: boolean;
  };
  type Gpu = {
    name: string;
    vendor: string;
    util_percent?: number | null;
    mem_used?: number | null;
    mem_total?: number | null;
    temp_c?: number | null;
    power_w?: number | null;
  };
  type Proc = { pid: number; name: string; cpu_percent: number; mem_bytes: number };
  type Battery = {
    id: string;
    name: string;
    capacity_pct?: number | null;
    status: string;
    charging: boolean;
    ac_online: boolean;
    power_w?: number | null;
    limit_pct?: number | null;
    start_pct?: number | null;
  };
  type Summary = {
    hostname: string;
    os: string;
    uptime_secs: number;
    cpu_percent: number;
    cpu_cores: number;
    cpu_temp_c?: number | null;
    cpu_power_w?: number | null;
    mem_used: number;
    mem_total: number;
    disks: { name: string; mount: string; total: number; used: number }[];
    networks: Net[];
    gpus: Gpu[];
    processes: Proc[];
    batteries?: Battery[];
    version: string;
    privileged?: boolean;
  };
  type App = AppRecord;

  let summary = $state<Summary | null>(null);
  let apps = $state<App[]>([]);
  let light = $state(isLight());
  let now = $state(new Date());
  let menu = $state(false);
  let netHist = $state<{ rx: number; tx: number }[]>([]);
  let cpuHist = $state<number[]>([]);
  let memHist = $state<number[]>([]);
  let query = $state('');
  let searchEl = $state<HTMLInputElement | null>(null);
  let powerConfirm = $state<'reboot' | 'shutdown' | null>(null);
  let powerBusy = $state('');

  const systemTiles = [
    { id: 'files', name: 'Files', icon: appIcons.files, to: '/files' },
    { id: 'settings', name: 'Settings', icon: appIcons.settings, to: '/settings' },
    { id: 'services', name: 'Services', icon: appIcons.services, to: '/services' },
    { id: 'install', name: 'Install', icon: appIcons.install, to: '/apps/new' }
  ];

  let q = $derived(query.trim().toLowerCase());
  let shownSystem = $derived(systemTiles.filter((t) => !q || t.name.toLowerCase().includes(q)));
  let shownApps = $derived(apps.filter((a) => !q || a.name.toLowerCase().includes(q)));

  let primaryNet = $derived(
    (summary?.networks ?? []).find((n) => !n.virtual_iface && n.operstate === 'up') ||
      (summary?.networks ?? []).find((n) => !n.virtual_iface)
  );
  let disks = $derived(summary?.disks ?? []);
  let topProc = $derived(
    (summary?.processes ?? []).find(
      (p) =>
        p.pid > 1 &&
        p.name !== 'coduosd' &&
        !p.name.startsWith('kworker') &&
        !p.name.startsWith('ksoftirqd')
    )
  );
  let gpu = $derived(summary?.gpus?.[0]);
  let battery = $derived(
    (summary?.batteries ?? []).find((b) => b.status.toLowerCase() === 'discharging') ||
      (summary?.batteries ?? []).find((b) => b.charging) ||
      summary?.batteries?.[0]
  );

  function loadTone(p: number) {
    if (p >= 90) return 'danger';
    if (p >= 75) return 'warn';
    return '';
  }

  function diskLabel(mount: string) {
    if (mount === '/') return 'System';
    const segs = mount.split('/').filter(Boolean);
    return segs[segs.length - 1] || mount;
  }

  function recordHist(s: Summary) {
    cpuHist = [...cpuHist, s.cpu_percent].slice(-90);
    memHist = [...memHist, pct(s.mem_used, s.mem_total)].slice(-90);
    const nets: Net[] = s.networks ?? [];
    const n =
      nets.find((x) => !x.virtual_iface && x.operstate === 'up') ||
      nets.find((x) => !x.virtual_iface);
    if (n) {
      netHist = [...netHist, { rx: n.rx_bps, tx: n.tx_bps }].slice(-90);
    }
  }

  function appActive(to: string) {
    return path === to || (to !== '/' && path.startsWith(to + '/'));
  }

  onMount(() => {
    const es = new EventSource('/api/system/summary/stream', { withCredentials: true });
    es.onmessage = (ev) => {
      try {
        summary = JSON.parse(ev.data);
        if (summary) recordHist(summary);
      } catch {
        /* ignore */
      }
    };
    api<Summary>('/api/system/summary')
      .then((s) => {
        summary = s;
        recordHist(s);
      })
      .catch(() => {});
    let jobs: Record<string, AppJob> = {};
    api<App[]>('/api/apps')
      .then((a) => (apps = a.map((app) => ({ ...app, status: overlayJob(app.status, jobs[app.id]) }))))
      .catch(() => {});
    const unsubJobs = subscribeAppJobs((next) => {
      const ended = Object.keys(jobs).filter((id) => !next[id]);
      jobs = next;
      apps = apps.map((app) =>
        next[app.id] ? { ...app, status: overlayJob(app.status, next[app.id]) } : app
      );
      if (ended.length) {
        api<App[]>('/api/apps')
          .then((a) => {
            apps = a.map((app) => ({ ...app, status: overlayJob(app.status, jobs[app.id]) }));
          })
          .catch(() => {});
      }
    });
    const c = setInterval(() => (now = new Date()), 30000);
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key !== '/' || ev.ctrlKey || ev.metaKey || ev.altKey) return;
      const t = ev.target as HTMLElement | null;
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
      ev.preventDefault();
      searchEl?.focus();
    };
    window.addEventListener('keydown', onKey);
    const onTheme = () => (light = isLight());
    window.addEventListener('coduos-theme', onTheme);
    return () => {
      es.close();
      unsubJobs();
      clearInterval(c);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('coduos-theme', onTheme);
    };
  });

  function openApp(app: App) {
    if (isLaunchable(app.status) && app.web_port) {
      window.open(`${location.protocol}//${location.hostname}:${app.web_port}`, '_blank');
    } else {
      go('/apps/' + app.id);
    }
  }

  function tap(to: string) {
    go(to);
  }

  async function applyPower(action: 'reboot' | 'shutdown') {
    powerBusy = action;
    powerConfirm = null;
    menu = false;
    try {
      await sendPower(action);
    } catch {
      powerBusy = '';
    }
  }
</script>

<div class="desktop" class:has-bottom={true}>
  <header class="topbar">
    <div class="topbar-brand">
      <BrandLogo kind="square" class="topbar-mark" />
      <div>
        <div class="host">{summary?.hostname ?? 'CoduOS'}</div>
        <div class="clock">
          {#if summary}{shortOs(summary.os)} · up {uptime(summary.uptime_secs)} · {/if}{now.toLocaleString()}
        </div>
      </div>
    </div>
    <div class="topbar-actions">
      <button class="icon-btn hit" onclick={() => { toggleTheme(); light = isLight(); }} aria-label={light ? 'Dark theme' : 'Light theme'}>
        <UiIcon name={light ? 'dark_mode' : 'light_mode'} size={18} />
      </button>
      <button class="icon-btn hit" onclick={() => (menu = !menu)}>
        <UiIcon name="person" size={18} /> {username}
      </button>
      <button class="icon-btn hit hide-phone" onclick={onLogout} aria-label="Sign out">
        <UiIcon name="logout" size={18} />
      </button>
    </div>
  </header>
  {#if menu}
    <div class="avatar-sheet">
      <button class="hit" onclick={() => { menu = false; go('/settings'); }}>
        <UiIcon name="settings" size={18} /> Settings
      </button>
      {#if summary?.privileged}
        <button class="hit" disabled={!!powerBusy} onclick={() => { menu = false; powerConfirm = 'reboot'; }}>
          <UiIcon name="power" size={18} />
          {powerBusy === 'reboot' ? 'Rebooting…' : 'Reboot'}
        </button>
        <button class="hit" disabled={!!powerBusy} onclick={() => { menu = false; powerConfirm = 'shutdown'; }}>
          <UiIcon name="power" size={18} />
          {powerBusy === 'shutdown' ? 'Shutting down…' : 'Shut down'}
        </button>
      {/if}
      <button class="hit" onclick={() => { menu = false; onLogout(); }}>
        <UiIcon name="logout" size={18} /> Sign out
      </button>
    </div>
  {/if}

  <div class="desk">
    <aside class="widget-col">
      {#if summary}
        <button class="widget hit" onclick={() => tap('/services')}>
          <div class="gauge {loadTone(summary.cpu_percent)}" style="--p:{summary.cpu_percent}">
            <span>{summary.cpu_percent.toFixed(0)}%</span>
          </div>
          <div>
            <h3><UiIcon name="cpu" size={14} /> CPU</h3>
            <div class="meta">
              {joinMeta([
                `${summary.cpu_cores} cores`,
                summary.cpu_temp_c != null ? `${summary.cpu_temp_c.toFixed(0)}°C` : null,
                summary.cpu_power_w != null ? watts(summary.cpu_power_w) : null
              ])}
            </div>
            {#if cpuHist.length > 1}
              <div class="widget-spark"><Sparkline rx={cpuHist} tx={[]} /></div>
            {/if}
          </div>
        </button>
        {#if gpu}
          <button class="widget hit" onclick={() => tap('/services')}>
            <div class="gauge {loadTone(gpu.util_percent ?? 0)}" style="--p:{gpu.util_percent ?? 0}">
              <span>{gpu.util_percent != null ? gpu.util_percent.toFixed(0) + '%' : '—'}</span>
            </div>
            <div>
              <h3><UiIcon name="gpu" size={14} /> GPU</h3>
              <div class="clip headline">{prettyGpu(gpu.name)}</div>
              <div class="meta">
                {joinMeta([
                  gpu.power_w != null ? watts(gpu.power_w) : null,
                  gpu.temp_c != null ? `${gpu.temp_c.toFixed(0)}°C` : null,
                  gpu.mem_used != null && gpu.mem_total ? `${bytes(gpu.mem_used)} / ${bytes(gpu.mem_total)}` : null
                ])}
              </div>
            </div>
          </button>
        {/if}
        <button class="widget hit" onclick={() => tap('/services')}>
          <div class="gauge {loadTone(pct(summary.mem_used, summary.mem_total))}" style="--p:{pct(summary.mem_used, summary.mem_total)}">
            <span>{pct(summary.mem_used, summary.mem_total)}%</span>
          </div>
          <div>
            <h3><UiIcon name="memory" size={14} /> Memory</h3>
            <div class="headline">{bytes(summary.mem_total - summary.mem_used)} free</div>
            <div class="meta">{bytes(summary.mem_used)} / {bytes(summary.mem_total)}</div>
            {#if memHist.length > 1}
              <div class="widget-spark"><Sparkline rx={memHist} tx={[]} /></div>
            {/if}
          </div>
        </button>
        {#if battery && battery.capacity_pct != null}
          <button
            class="widget widget-bat hit"
            class:low={battery.capacity_pct < 20 && !battery.charging && !battery.ac_online}
            onclick={() => tap('/settings')}
          >
            <div class="gauge {battery.capacity_pct < 20 && !battery.charging ? 'danger' : ''}" style="--p:{battery.capacity_pct}">
              <span>{battery.capacity_pct}%</span>
            </div>
            <div>
              <h3><UiIcon name="battery" size={14} /> Battery</h3>
              <div class="headline">{batteryLabel(battery)}</div>
              <div class="meta clip">
                {joinMeta([
                  battery.charging && battery.power_w ? watts(battery.power_w) : null,
                  battery.limit_pct != null && battery.limit_pct < 100 && battery.status.toLowerCase() !== 'not charging'
                    ? battery.start_pct != null && battery.start_pct < battery.limit_pct
                      ? `${battery.start_pct}–${battery.limit_pct}%`
                      : `limit ${battery.limit_pct}%`
                    : null
                ])}
              </div>
            </div>
          </button>
        {/if}
        {#if primaryNet}
          <button class="widget widget-net hit" onclick={() => tap('/settings/network')}>
            <div>
              <h3><UiIcon name="network" size={14} /> Network</h3>
              <div class="net-rates">
                <span class="net-rx"><UiIcon name="arrow_downward" size={16} /> {bps(primaryNet.rx_bps)}</span>
                <span class="net-tx"><UiIcon name="arrow_upward" size={16} /> {bps(primaryNet.tx_bps)}</span>
              </div>
              <div class="meta clip">
                {joinMeta([
                  primaryNet.ipv4,
                  primaryNet.name,
                  primaryNet.speed_mbps
                    ? primaryNet.speed_mbps >= 1000
                      ? `${primaryNet.speed_mbps / 1000} Gb/s`
                      : `${primaryNet.speed_mbps} Mb/s`
                    : null
                ])}
              </div>
            </div>
            <Sparkline rx={netHist.map((s) => s.rx)} tx={netHist.map((s) => s.tx)} showScale />
          </button>
        {/if}
        {#if disks.length}
          <button class="widget widget-storage hit" class:widget-storage-many={disks.length > 1} onclick={() => tap('/settings/storage')}>
            {#if disks.length === 1}
              <div class="gauge {loadTone(pct(disks[0].used, disks[0].total))}" style="--p:{pct(disks[0].used, disks[0].total)}">
                <span>{pct(disks[0].used, disks[0].total)}%</span>
              </div>
            {/if}
            <div>
              <h3><UiIcon name="storage" size={14} /> Storage</h3>
              {#if disks.length === 1}
                <div class="headline">{bytes(disks[0].total - disks[0].used)} free</div>
                <div class="meta clip">
                  {joinMeta([
                    disks[0].mount === '/' ? 'System disk' : disks[0].mount,
                    `${bytes(disks[0].used)} / ${bytes(disks[0].total)}`
                  ])}
                </div>
              {:else}
                <div class="disk-list">
                  {#each disks as d}
                    {@const p = pct(d.used, d.total)}
                    <div class="disk-row">
                      <div class="disk-row-head">
                        <span class="clip">{diskLabel(d.mount)}</span>
                        <span>{p}%</span>
                      </div>
                      <div class="bar {loadTone(p)}"><i style="width:{p}%"></i></div>
                      <div class="meta clip">{bytes(d.total - d.used)} free</div>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          </button>
        {/if}
        {#if topProc}
          <button class="widget hit" onclick={() => tap('/services')}>
            <div>
              <h3><Icon name={appIcons.services} size={16} /> Services</h3>
              <div class="clip headline">{topProc.name}</div>
              <div class="meta">{topProc.cpu_percent.toFixed(0)}% CPU · {bytes(topProc.mem_bytes)}</div>
            </div>
          </button>
        {/if}
      {/if}
    </aside>

    <section class="desk-main">
      <label class="desk-search">
        <span class="meta">Search</span>
        <UiIcon name="search" size={18} />
        <input
          bind:this={searchEl}
          bind:value={query}
          placeholder="Apps and tools"
          aria-label="Search apps"
        />
      </label>
      {#key $iconRev}
      <div class="desk-apps launch">
        {#each shownSystem as t}
          <button class="desk-app hit" class:active={appActive(t.to)} onclick={() => go(t.to)}>
            <Icon name={t.icon} size={64} class="tile-img" alt="" />
            <div class="label">{t.name}</div>
          </button>
        {/each}
        {#each shownApps as app}
          <div class="desk-app" class:active={appActive('/apps/' + app.id)}>
            <button type="button" class="desk-app-main hit" onclick={() => openApp(app)}>
              <div class="icon-wrap">
                <Icon name={appIcon(app)} size={64} class="tile-img" alt="" />
                {#if isBusy(app.status.phase)}
                  <ProgressStrip percent={app.status.percent ?? null} pulse={app.status.percent == null} />
                {/if}
              </div>
              <div class="label">{app.name}</div>
            </button>
            <button
              type="button"
              class="desk-app-status"
              onclick={() => go('/apps/' + app.id)}
              aria-label="{app.name} settings"
            >
              <StatusPill status={app.status} />
            </button>
          </div>
        {/each}
        {#if !q && shownSystem.length + shownApps.length === 0}
          <p class="hint">Nothing matches.</p>
        {:else if q && shownSystem.length + shownApps.length === 0}
          <p class="hint">No apps named “{query}”.</p>
        {:else if !q && shownApps.length === 0}
          <p class="hint desk-apps-empty">No apps yet. Open Install to add one from Compose.</p>
        {/if}
      </div>
      {/key}
    </section>
  </div>

  <nav class="bottom-nav" aria-label="Main">
    <button class="hit" class:active={path === '/'} onclick={() => go('/')}>
      <UiIcon name="home" size={22} /> Home
    </button>
    <button class="hit" class:active={path.startsWith('/files')} onclick={() => go('/files')}>
      <UiIcon name="folder" size={22} /> Files
    </button>
    <button class="hit" class:active={path.startsWith('/services') || path === '/tasks'} onclick={() => go('/services')}>
      <UiIcon name="services" size={22} /> Services
    </button>
    <button class="hit" class:active={path.startsWith('/settings')} onclick={() => go('/settings')}>
      <UiIcon name="settings" size={22} /> Settings
    </button>
  </nav>
</div>

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
