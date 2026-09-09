<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { bytes } from '../lib/format';
  import AppWindow from '../components/AppWindow.svelte';
  import Confirm from '../components/Confirm.svelte';

  type Proc = { pid: number; name: string; cpu_percent: number; mem_bytes: number };
  type Ctn = { name: string; cpu_percent: number; mem_usage: string; pids: string };

  let { onClose } = $props<{ onClose: () => void }>();
  let tab = $state<'host' | 'apps'>('host');
  let processes = $state<Proc[]>([]);
  let containers = $state<Ctn[]>([]);
  let error = $state('');
  let sort = $state<'cpu' | 'mem' | 'name'>('cpu');
  let pending = $state<Proc | null>(null);

  async function load() {
    try {
      const data = await api<{ processes: Proc[]; containers: Ctn[] }>('/api/system/tasks');
      processes = data.processes;
      containers = data.containers;
    } catch (e: any) {
      error = e.message;
    }
  }

  onMount(() => {
    load();
    const t = setInterval(load, 3000);
    return () => clearInterval(t);
  });

  let rows = $derived.by(() => {
    const list = [...processes];
    list.sort((a, b) => {
      if (sort === 'mem') return b.mem_bytes - a.mem_bytes;
      if (sort === 'name') return a.name.localeCompare(b.name);
      return b.cpu_percent - a.cpu_percent;
    });
    return list;
  });

  async function kill(pid: number) {
    error = '';
    try {
      await api(`/api/system/process/${pid}/signal`, { method: 'POST', body: '{}' });
      pending = null;
      await load();
    } catch (e: any) {
      error = e.message;
      pending = null;
    }
  }
</script>

<AppWindow title="Services" icon="services" fileIcon size="xl" {onClose}>
  {#snippet actions()}
    <div class="os-tabs" role="tablist" aria-label="Services view">
      <button class="os-tab" class:active={tab === 'host'} role="tab" aria-selected={tab === 'host'} onclick={() => (tab = 'host')}>Host</button>
      <button class="os-tab" class:active={tab === 'apps'} role="tab" aria-selected={tab === 'apps'} onclick={() => (tab = 'apps')}>Containers</button>
    </div>
  {/snippet}

{#if error}<div class="err">{error}</div>{/if}

{#if tab === 'host'}
  <table class="table">
    <thead>
      <tr>
        <th><button class="linkish" class:active={sort === 'name'} onclick={() => (sort = 'name')}>Name</button></th>
        <th>PID</th>
        <th><button class="linkish" class:active={sort === 'cpu'} onclick={() => (sort = 'cpu')}>CPU</button></th>
        <th><button class="linkish" class:active={sort === 'mem'} onclick={() => (sort = 'mem')}>Memory</button></th>
        <th></th>
      </tr>
    </thead>
    <tbody>
      {#each rows as p}
        <tr>
          <td>{p.name}</td>
          <td>{p.pid}</td>
          <td>{p.cpu_percent.toFixed(1)}%</td>
          <td>{bytes(p.mem_bytes)}</td>
          <td>
            {#if p.pid > 1 && p.name !== 'coduosd'}
              <button class="btn secondary compact" onclick={() => (pending = p)}>End</button>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
{:else if containers.length === 0}
  <p class="hint">No Docker stats. Start Docker or install an app.</p>
{:else}
  <table class="table">
    <thead><tr><th>Container</th><th>CPU</th><th>Memory</th><th>PIDs</th></tr></thead>
    <tbody>
      {#each containers as c}
        <tr>
          <td>{c.name}</td>
          <td>{c.cpu_percent.toFixed(1)}%</td>
          <td>{c.mem_usage}</td>
          <td>{c.pids}</td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}
</AppWindow>

{#if pending}
  <Confirm
    title="End process?"
    body={`Send SIGTERM to ${pending.name} (PID ${pending.pid})? PID 1, kernel threads, and CoduOS itself cannot be ended.`}
    confirmLabel="End process"
    danger
    onCancel={() => (pending = null)}
    onConfirm={() => kill(pending!.pid)}
  />
{/if}
