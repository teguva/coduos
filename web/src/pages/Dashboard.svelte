<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { bytes, pct, uptime } from '../lib/format';

  let { go } = $props<{ go: (to: string) => void }>();

  type Summary = {
    hostname: string;
    os: string;
    uptime_secs: number;
    cpu_percent: number;
    cpu_cores: number;
    mem_used: number;
    mem_total: number;
    disks: { name: string; mount: string; total: number; used: number }[];
    networks: { name: string; rx_bytes: number; tx_bytes: number }[];
    docker: { available: boolean; version?: string | null; error?: string | null };
    version: string;
  };
  type App = {
    id: string;
    name: string;
    web_port?: number | null;
    status: { running: boolean };
  };

  let summary = $state<Summary | null>(null);
  let apps = $state<App[]>([]);
  let error = $state('');

  async function load() {
    try {
      summary = await api<Summary>('/api/system/summary');
      apps = await api<App[]>('/api/apps');
      error = '';
    } catch (e: any) {
      error = e.message;
    }
  }

  onMount(() => {
    load();
    const t = setInterval(load, 4000);
    return () => clearInterval(t);
  });
</script>

<div class="top">
  <div>
    <h2>{summary?.hostname ?? 'CoduOS'}</h2>
    <div class="sub">{summary?.os ?? ''} · up {summary ? uptime(summary.uptime_secs) : '…'} · v{summary?.version ?? ''}</div>
  </div>
</div>

{#if error}<div class="err">{error}</div>{/if}

{#if summary}
  <div class="grid-stats">
    <div class="card">
      <h3>CPU</h3>
      <div class="metric">{summary.cpu_percent.toFixed(0)}%</div>
      <div class="sub">{summary.cpu_cores} cores</div>
      <div class="bar"><i style="width:{summary.cpu_percent}%"></i></div>
    </div>
    <div class="card">
      <h3>Memory</h3>
      <div class="metric">{pct(summary.mem_used, summary.mem_total)}%</div>
      <div class="sub">{bytes(summary.mem_used)} / {bytes(summary.mem_total)}</div>
      <div class="bar"><i style="width:{pct(summary.mem_used, summary.mem_total)}%"></i></div>
    </div>
    <div class="card">
      <h3>Storage</h3>
      {#if summary.disks[0]}
        <div class="metric">{pct(summary.disks[0].used, summary.disks[0].total)}%</div>
        <div class="sub">{summary.disks[0].mount} · {bytes(summary.disks[0].used)} / {bytes(summary.disks[0].total)}</div>
        <div class="bar"><i style="width:{pct(summary.disks[0].used, summary.disks[0].total)}%"></i></div>
      {:else}
        <div class="metric">—</div>
      {/if}
    </div>
    <div class="card">
      <h3>Docker</h3>
      <div class="metric">{summary.docker.available ? 'Ready' : 'Off'}</div>
      <div class="sub">{summary.docker.version ?? summary.docker.error ?? 'not detected'}</div>
    </div>
  </div>
{/if}

<div class="top">
  <div>
    <h2>Apps</h2>
    <div class="sub">Compose stacks installed on this machine</div>
  </div>
  <button class="btn" onclick={() => go('/apps/new')}>Install app</button>
</div>

<div class="apps">
  {#each apps as app}
    <button class="app" onclick={() => go('/apps/' + app.id)}>
      <div class="icon">{app.name.slice(0, 1).toUpperCase()}</div>
      <div class="name">{app.name}</div>
      <div class="meta"><span class="dot" class:on={app.status.running}></span> {app.status.running ? 'Running' : 'Stopped'}</div>
    </button>
  {/each}
  <button class="app" onclick={() => go('/apps/new')}>
    <div class="icon">+</div>
    <div class="name">Add</div>
    <div class="meta">Compose YAML</div>
  </button>
</div>
