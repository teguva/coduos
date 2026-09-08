<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { bytes, pct } from '../lib/format';
  import Icon from '../components/Icon.svelte';
  import Confirm from '../components/Confirm.svelte';

  let { go } = $props<{ go: (to: string) => void }>();

  type Partition = {
    name: string;
    path: string;
    size: number;
    fstype: string;
    label: string;
    uuid: string;
    mountpoint: string;
    removable: boolean;
    system: boolean;
    used?: number | null;
    total?: number | null;
    health?: string | null;
    in_files: boolean;
  };
  type Disk = {
    name: string;
    path: string;
    size: number;
    model: string;
    vendor: string;
    transport: string;
    removable: boolean;
    system: boolean;
    partitions: Partition[];
    health?: string | null;
  };
  type Inventory = { privileged: boolean; disks: Disk[] };

  let inv = $state<Inventory | null>(null);
  let error = $state('');
  let busy = $state('');
  let confirm = $state<{ title: string; body: string; force?: boolean; device: string } | null>(null);

  async function load() {
    inv = await api<Inventory>('/api/storage');
  }

  onMount(() => load().catch((e) => (error = e.message)));

  let internal = $derived((inv?.disks ?? []).filter((d) => !d.removable));
  let external = $derived((inv?.disks ?? []).filter((d) => d.removable));

  function title(d: Disk) {
    return [d.vendor, d.model].filter(Boolean).join(' ').trim() || d.name;
  }

  function chip(p: Partition, d: Disk) {
    if (p.system || d.system) return 'System';
    if (p.mountpoint) return 'Mounted';
    if (d.removable || p.removable) return 'USB';
    return p.fstype || 'Disk';
  }

  async function mount(device: string) {
    error = '';
    busy = device;
    try {
      await api('/api/storage/mount', { method: 'POST', body: JSON.stringify({ device }) });
      await load();
    } catch (e: any) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  async function eject(device: string, force = false) {
    error = '';
    busy = device;
    try {
      const res = await api<{ ok: boolean; busy: boolean; pids: number[]; message: string }>(
        '/api/storage/unmount',
        { method: 'POST', body: JSON.stringify({ device, force }) }
      );
      if (!res.ok && res.busy) {
        confirm = {
          title: 'Force eject?',
          body: res.message || `In use by PIDs ${res.pids.join(', ') || 'unknown'}. Force eject may lose writes.`,
          force: true,
          device
        };
      } else {
        await load();
      }
    } catch (e: any) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  async function useInFiles(device: string) {
    error = '';
    try {
      const root = await api<{ id: string }>('/api/storage/files', {
        method: 'POST',
        body: JSON.stringify({ device })
      });
      go('/files?root=' + encodeURIComponent(root.id));
    } catch (e: any) {
      error = e.message;
    }
  }
</script>

{#if !inv?.privileged}
  <div class="banner">Mount and eject need the installed daemon running as root. Inventory is still shown.</div>
{/if}
{#if error}<div class="err">{error}</div>{/if}

<h3 class="group-title">This device</h3>
{#if internal.length === 0}
  <p class="hint">No internal disks reported.</p>
{/if}
{#each internal as d}
  <div class="storage-card">
    <div class="storage-head">
      <Icon name="disk" size={28} alt="" />
      <div>
        <strong>{title(d)}</strong>
        <div class="meta">{bytes(d.size)} · {d.path}{#if d.transport} · {d.transport}{/if}</div>
      </div>
    </div>
    {#each d.partitions as p}
      {@const used = p.used ?? 0}
      {@const total = p.total ?? p.size}
      <div class="part">
        <div class="row">
          <span class="chip">{chip(p, d)}</span>
          <strong>{p.label || p.name}</strong>
          <span class="meta">{p.fstype || 'unformatted'} · {bytes(p.size)}</span>
        </div>
        {#if p.mountpoint}
          <div class="bar"><i style="width:{pct(used, total)}%"></i></div>
          <div class="meta">{p.mountpoint} · {bytes(used)} / {bytes(total)}{#if p.health} · SMART {p.health}{/if}</div>
        {/if}
        <div class="row">
          {#if p.in_files}
            <button class="btn" onclick={() => go('/files')}>Open in Files</button>
          {:else if p.mountpoint}
            <button class="btn" onclick={() => useInFiles(p.path)}>Use in Files</button>
          {/if}
        </div>
      </div>
    {/each}
  </div>
{/each}

<h3 class="group-title">External</h3>
{#if external.length === 0}
  <p class="hint">Plug in a drive.</p>
{/if}
{#each external as d}
  <div class="storage-card">
    <div class="storage-head">
      <Icon name="disk" size={28} alt="" />
      <div>
        <strong>{title(d)}</strong>
        <div class="meta">{bytes(d.size)} · {d.path}</div>
      </div>
    </div>
    {#each d.partitions.length ? d.partitions : [{ ...d, name: d.name, path: d.path, fstype: '', label: title(d), uuid: '', mountpoint: '', removable: true, system: false, in_files: false } as any] as p}
      {@const used = p.used ?? 0}
      {@const total = p.total ?? p.size}
      <div class="part">
        <div class="row">
          <span class="chip">{p.mountpoint ? 'Mounted' : 'USB'}</span>
          <strong>{p.label || p.name}</strong>
        </div>
        {#if p.mountpoint}
          <div class="bar"><i style="width:{pct(used, total)}%"></i></div>
          <div class="meta">{p.mountpoint} · {bytes(used)} / {bytes(total)}</div>
        {/if}
        <div class="row">
          {#if p.mountpoint}
            <button class="btn secondary" disabled={!inv?.privileged || busy === p.path} onclick={() => eject(p.path)}>Eject</button>
            {#if p.in_files}
              <button class="btn" onclick={() => go('/files')}>Open in Files</button>
            {:else}
              <button class="btn" onclick={() => useInFiles(p.path)}>Use in Files</button>
            {/if}
          {:else}
            <button class="btn" disabled={!inv?.privileged || busy === p.path} onclick={() => mount(p.path)}>Mount</button>
          {/if}
        </div>
      </div>
    {/each}
  </div>
{/each}

{#if confirm}
  <Confirm
    title={confirm.title}
    body={confirm.body}
    confirmLabel="Force eject"
    danger
    onCancel={() => (confirm = null)}
    onConfirm={async () => {
      const device = confirm!.device;
      confirm = null;
      await eject(device, true);
    }}
  />
{/if}
