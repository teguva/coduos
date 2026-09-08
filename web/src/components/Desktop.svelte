<script lang="ts">
  import { onMount } from 'svelte';
  import { api, toggleTheme, isLight } from '../lib/api';
  import { bytes, bps, pct, uptime, shortOs, prettyGpu, watts, joinMeta, batteryLabel } from '../lib/format';
  import { appIcons, appIcon, iconRev } from '../lib/icons';
  import Icon from './Icon.svelte';
  import Sparkline from './Sparkline.svelte';

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
  };
  type App = {
    id: string;
    name: string;
    icon_url?: string | null;
    web_port?: number | null;
    status: { running: boolean };
  };

  let summary = $state<Summary | null>(null);
  let apps = $state<App[]>([]);
  let light = $state(isLight());
  let now = $state(new Date());
  let menu = $state(false);
  let netHist = $state<{ rx: number; tx: number }[]>([]);
  let query = $state('');
  let searchEl = $state<HTMLInputElement | null>(null);

  const systemTiles = [
    { id: 'files', name: 'Files', icon: appIcons.files, to: '/files' },
    { id: 'apps', name: 'Apps', icon: appIcons.apps, to: '/apps' },
    { id: 'settings', name: 'Settings', icon: appIcons.settings, to: '/settings' },
    { id: 'tasks', name: 'Tasks', icon: appIcons.tasks, to: '/tasks' }
  ];

  let q = $derived(query.trim().toLowerCase());
  let shownSystem = $derived(systemTiles.filter((t) => !q || t.name.toLowerCase().includes(q)));
  let shownApps = $derived(apps.filter((a) => !q || a.name.toLowerCase().includes(q)));

  let primaryNet = $derived(
    (summary?.networks ?? []).find((n) => !n.virtual_iface && n.operstate === 'up') ||
      (summary?.networks ?? []).find((n) => !n.virtual_iface)
  );
  let worstDisk = $derived.by(() => {
    const disks = summary?.disks ?? [];
    if (!disks.length) return null;
    return disks.slice().sort((a, b) => pct(b.used, b.total) - pct(a.used, a.total))[0];
  });
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

  function appActive(to: string) {
    if (to === '/apps') {
      return path === '/apps' || (path.startsWith('/apps/') && path !== '/apps/new');
    }
    return path === to || path.startsWith(to + '/');
  }

  onMount(() => {
    const es = new EventSource('/api/system/summary/stream', { withCredentials: true });
    es.onmessage = (ev) => {
      try {
        summary = JSON.parse(ev.data);
        const nets: Net[] = summary?.networks ?? [];
        const n =
          nets.find((x) => !x.virtual_iface && x.operstate === 'up') ||
          nets.find((x) => !x.virtual_iface);
        if (n) {
          netHist = [...netHist, { rx: n.rx_bps, tx: n.tx_bps }].slice(-90);
        }
      } catch {
        /* ignore */
      }
    };
    api<Summary>('/api/system/summary')
      .then((s) => {
        summary = s;
        const n =
          (s.networks ?? []).find((x) => !x.virtual_iface && x.operstate === 'up') ||
          (s.networks ?? []).find((x) => !x.virtual_iface);
        if (n) {
          netHist = [{ rx: n.rx_bps, tx: n.tx_bps }];
        }
      })
      .catch(() => {});
    api<App[]>('/api/apps')
      .then((a) => (apps = a))
      .catch(() => {});
    const c = setInterval(() => (now = new Date()), 30000);
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key !== '/' || ev.ctrlKey || ev.metaKey || ev.altKey) return;
      const t = ev.target as HTMLElement | null;
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
      ev.preventDefault();
      searchEl?.focus();
    };
    window.addEventListener('keydown', onKey);
    return () => {
      es.close();
      clearInterval(c);
      window.removeEventListener('keydown', onKey);
    };
  });

  function openApp(app: App) {
    if (app.web_port) {
      window.open(`${location.protocol}//${location.hostname}:${app.web_port}`, '_blank');
    } else {
      go('/apps/' + app.id);
    }
  }

  function tap(to: string) {
    go(to);
  }
</script>

<div class="desktop" class:has-bottom={true}>
  <header class="topbar">
    <div>
      <div class="host">{summary?.hostname ?? 'CoduOS'}</div>
      <div class="clock">{now.toLocaleString()}{#if summary} · up {uptime(summary.uptime_secs)}{/if}</div>
    </div>
    <div class="topbar-actions">
      <button class="icon-btn hit" onclick={() => { toggleTheme(); light = isLight(); }}>{light ? 'Dark' : 'Light'}</button>
      <button class="icon-btn hit" onclick={() => (menu = !menu)}>{username}</button>
      <button class="icon-btn hit hide-phone" onclick={onLogout}>Sign out</button>
    </div>
  </header>
  {#if menu}
    <div class="avatar-sheet">
      <button class="hit" onclick={() => { menu = false; go('/settings'); }}>Settings</button>
      <button class="hit" onclick={() => { menu = false; onLogout(); }}>Sign out</button>
    </div>
  {/if}

  <div class="desk">
    <aside class="widget-col">
      {#if summary}
        <button class="widget hit" onclick={() => tap('/tasks')}>
          <div class="gauge" style="--p:{summary.cpu_percent}"><span>{summary.cpu_percent.toFixed(0)}%</span></div>
          <div>
            <h3><Icon name="cpu" size={16} alt="" /> CPU</h3>
            <div>
              {joinMeta([
                summary.cpu_power_w != null ? watts(summary.cpu_power_w) : null,
                `${summary.cpu_cores} cores`,
                summary.cpu_temp_c != null ? `${summary.cpu_temp_c.toFixed(0)}°C` : null
              ])}
            </div>
            <div class="meta clip">{shortOs(summary.os)}</div>
          </div>
        </button>
        {#if gpu}
          <button class="widget hit" onclick={() => tap('/tasks')}>
            <div class="gauge" style="--p:{gpu.util_percent ?? 0}">
              <span>{gpu.util_percent != null ? gpu.util_percent.toFixed(0) + '%' : 'GPU'}</span>
            </div>
            <div>
              <h3>GPU</h3>
              <div class="clip">{prettyGpu(gpu.name)}</div>
              <div class="meta">
                {joinMeta([
                  gpu.power_w != null ? watts(gpu.power_w) : null,
                  gpu.temp_c != null ? `${gpu.temp_c.toFixed(0)}°C` : null,
                  gpu.mem_used != null && gpu.mem_total ? `${bytes(gpu.mem_used)} / ${bytes(gpu.mem_total)}` : null
                ]) || 'Integrated graphics'}
              </div>
            </div>
          </button>
        {/if}
        <button class="widget hit" onclick={() => tap('/tasks')}>
          <div class="gauge" style="--p:{pct(summary.mem_used, summary.mem_total)}"><span>{pct(summary.mem_used, summary.mem_total)}%</span></div>
          <div>
            <h3><Icon name="memory" size={16} alt="" /> Memory</h3>
            <div>{bytes(summary.mem_used)} / {bytes(summary.mem_total)}</div>
          </div>
        </button>
        {#if battery && battery.capacity_pct != null}
          <button
            class="widget widget-bat hit"
            class:low={battery.capacity_pct < 20 && !battery.charging && !battery.ac_online}
            onclick={() => tap('/settings')}
          >
            <div class="gauge" style="--p:{battery.capacity_pct}"><span>{battery.capacity_pct}%</span></div>
            <div>
              <h3><Icon name={appIcons.battery} size={16} alt="" /> Battery</h3>
              <div>{batteryLabel(battery)}</div>
              <div class="meta clip">
                {joinMeta([
                  battery.charging && battery.power_w ? watts(battery.power_w) : null,
                  battery.limit_pct != null && battery.limit_pct < 100 && battery.status.toLowerCase() !== 'not charging'
                    ? `limit ${battery.limit_pct}%`
                    : null,
                  battery.name
                ])}
              </div>
            </div>
          </button>
        {/if}
        {#if primaryNet}
          <button class="widget widget-net hit" onclick={() => tap('/settings/network')}>
            <div>
              <h3><Icon name={appIcons.network} size={16} alt="" /> Network</h3>
              <div>↓ {bps(primaryNet.rx_bps)} ↑ {bps(primaryNet.tx_bps)}</div>
              <div class="meta clip">{primaryNet.ipv4 || primaryNet.name}</div>
            </div>
            <Sparkline rx={netHist.map((s) => s.rx)} tx={netHist.map((s) => s.tx)} />
          </button>
        {/if}
        {#if worstDisk}
          <button class="widget hit" onclick={() => tap('/settings/storage')}>
            <div class="gauge" style="--p:{pct(worstDisk.used, worstDisk.total)}"><span>{pct(worstDisk.used, worstDisk.total)}%</span></div>
            <div>
              <h3><Icon name="disk" size={16} alt="" /> Storage</h3>
              <div class="clip">{worstDisk.mount === '/' ? 'System disk' : worstDisk.mount}</div>
              <div class="meta">{bytes(worstDisk.used)} / {bytes(worstDisk.total)}</div>
            </div>
          </button>
        {/if}
        {#if topProc}
          <button class="widget hit" onclick={() => tap('/tasks')}>
            <div class="gauge" style="--p:{Math.min(100, topProc.cpu_percent)}"><span>{Math.min(100, topProc.cpu_percent).toFixed(0)}%</span></div>
            <div>
              <h3>Tasks</h3>
              <div class="clip">{topProc.name}</div>
              <div class="meta">{bytes(topProc.mem_bytes)}</div>
            </div>
          </button>
        {/if}
      {/if}
    </aside>

    <section class="desk-main">
      <label class="desk-search">
        <span class="meta">Search</span>
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
          <button class="desk-app hit" class:active={appActive('/apps/' + app.id)} onclick={() => openApp(app)}>
            <Icon name={appIcon(app)} size={64} class="tile-img" alt="" />
            <div class="label">{app.name}</div>
          </button>
        {/each}
        {#if !q && shownSystem.length + shownApps.length === 0}
          <p class="hint">Nothing matches.</p>
        {:else if q && shownSystem.length + shownApps.length === 0}
          <p class="hint">No apps named “{query}”.</p>
        {/if}
      </div>
      {/key}
    </section>
  </div>

  <nav class="bottom-nav" aria-label="Main">
    <button class="hit" class:active={path === '/'} onclick={() => go('/')}>
      <Icon name="go-home" size={22} alt="" /> Home
    </button>
    <button class="hit" class:active={path.startsWith('/files')} onclick={() => go('/files')}>
      <Icon name={appIcons.files} size={22} alt="" /> Files
    </button>
    <button class="hit" class:active={path.startsWith('/apps')} onclick={() => go('/apps')}>
      <Icon name={appIcons.apps} size={22} alt="" /> Apps
    </button>
    <button class="hit" class:active={path.startsWith('/settings')} onclick={() => go('/settings')}>
      <Icon name={appIcons.settings} size={22} alt="" /> Settings
    </button>
  </nav>
</div>
