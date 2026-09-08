<script lang="ts">
  import { onMount } from 'svelte';
  import { api, toggleTheme, isLight } from '../lib/api';
  import { bytes, bps, pct, uptime, shortOs, prettyGpu } from '../lib/format';
  import { appIcons, appIcon, iconRev } from '../lib/icons';
  import Icon from './Icon.svelte';

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
  };
  type Proc = { pid: number; name: string; cpu_percent: number; mem_bytes: number };
  type Summary = {
    hostname: string;
    os: string;
    uptime_secs: number;
    cpu_percent: number;
    cpu_cores: number;
    cpu_temp_c?: number | null;
    mem_used: number;
    mem_total: number;
    disks: { name: string; mount: string; total: number; used: number }[];
    networks: Net[];
    gpus: Gpu[];
    processes: Proc[];
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
      } catch {
        /* ignore */
      }
    };
    api<Summary>('/api/system/summary')
      .then((s) => (summary = s))
      .catch(() => {});
    api<App[]>('/api/apps')
      .then((a) => (apps = a))
      .catch(() => {});
    const c = setInterval(() => (now = new Date()), 30000);
    return () => {
      es.close();
      clearInterval(c);
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
            <div>{summary.cpu_cores} cores{#if summary.cpu_temp_c != null} · {summary.cpu_temp_c.toFixed(0)}°C{/if}</div>
            <div class="meta clip">{shortOs(summary.os)}</div>
          </div>
        </button>
        {#if gpu}
          <button class="widget hit" onclick={() => tap('/tasks')}>
            <div class="gauge" style="--p:{gpu.util_percent ?? 0}"><span>{gpu.util_percent != null ? gpu.util_percent.toFixed(0) + '%' : 'GPU'}</span></div>
            <div>
              <h3>GPU</h3>
              <div class="clip">{prettyGpu(gpu.name)}</div>
              <div class="meta">{#if gpu.temp_c != null}{gpu.temp_c.toFixed(0)}°C{/if}{#if gpu.mem_used != null && gpu.mem_total} · {bytes(gpu.mem_used)} / {bytes(gpu.mem_total)}{/if}</div>
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
        {#if primaryNet}
          <button class="widget hit" onclick={() => tap('/network')}>
            <div class="gauge plain"><span>↓↑</span></div>
            <div>
              <h3><Icon name={appIcons.network} size={16} alt="" /> Network</h3>
              <div>↓ {bps(primaryNet.rx_bps)} ↑ {bps(primaryNet.tx_bps)}</div>
              <div class="meta clip">{primaryNet.ipv4 || primaryNet.name}</div>
            </div>
          </button>
        {/if}
        {#if worstDisk}
          <button class="widget hit" onclick={() => tap('/storage')}>
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
      {#key $iconRev}
      <div class="desk-apps">
        <button class="desk-app hit" class:active={appActive('/files')} onclick={() => go('/files')}>
          <Icon name={appIcons.files} size={56} class="tile-img" alt="" />
          <div class="label">Files</div>
        </button>
        <button class="desk-app hit" class:active={appActive('/apps')} onclick={() => go('/apps')}>
          <Icon name={appIcons.apps} size={56} class="tile-img" alt="" />
          <div class="label">Apps</div>
        </button>
        <button class="desk-app hit" class:active={appActive('/storage')} onclick={() => go('/storage')}>
          <Icon name={appIcons.storage} size={56} class="tile-img" alt="" />
          <div class="label">Storage</div>
        </button>
        <button class="desk-app hit" class:active={appActive('/tasks')} onclick={() => go('/tasks')}>
          <Icon name={appIcons.tasks} size={56} class="tile-img" alt="" />
          <div class="label">Tasks</div>
        </button>
        <button class="desk-app hit" class:active={appActive('/vpn')} onclick={() => go('/vpn')}>
          <Icon name={appIcons.vpn} size={56} class="tile-img" alt="" />
          <div class="label">VPN</div>
        </button>
        <button class="desk-app hit" class:active={appActive('/proxy')} onclick={() => go('/proxy')}>
          <Icon name={appIcons.proxy} size={56} class="tile-img" alt="" />
          <div class="label">Proxy</div>
        </button>
        <button class="desk-app hit" class:active={appActive('/services')} onclick={() => go('/services')}>
          <Icon name={appIcons.services} size={56} class="tile-img" alt="" />
          <div class="label">Services</div>
        </button>
        <button class="desk-app hit" class:active={appActive('/settings')} onclick={() => go('/settings')}>
          <Icon name={appIcons.settings} size={56} class="tile-img" alt="" />
          <div class="label">Settings</div>
        </button>
        {#each apps as app}
          <button class="desk-app hit" class:active={appActive('/apps/' + app.id)} onclick={() => openApp(app)}>
            <Icon name={appIcon(app)} size={56} class="tile-img" alt="" />
            <div class="label">{app.name}</div>
            <div class="state"><span class="dot" class:on={app.status.running}></span> {app.status.running ? 'Running' : 'Stopped'}</div>
          </button>
        {/each}
        <button class="desk-app hit" class:active={path === '/apps/new'} onclick={() => go('/apps/new')}>
          <Icon name={appIcons.install} size={56} class="tile-img" alt="" />
          <div class="label">Install</div>
        </button>
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
    <button class="hit" class:active={path.startsWith('/storage')} onclick={() => go('/storage')}>
      <Icon name={appIcons.storage} size={22} alt="" /> Storage
    </button>
    <button class="hit" class:active={path.startsWith('/settings')} onclick={() => go('/settings')}>
      <Icon name={appIcons.settings} size={22} alt="" /> Settings
    </button>
  </nav>
</div>
