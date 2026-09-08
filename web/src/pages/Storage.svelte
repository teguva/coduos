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
  type Role = 'swap' | 'efi' | 'boot' | 'system' | 'usb' | 'data' | 'unmounted';

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

  function role(p: Partition, d: Disk): Role {
    const fs = (p.fstype || '').toLowerCase();
    const mp = p.mountpoint || '';
    if (fs === 'swap' || mp.toUpperCase().includes('SWAP')) return 'swap';
    if (mp === '/boot/efi' || (mp.toLowerCase().includes('efi') && mp.includes('boot'))) return 'efi';
    if (mp === '/boot') return 'boot';
    if (!mp && fs === 'vfat' && p.size > 0 && p.size < 4 * 1024 ** 3 && d.partitions.some((x) => /ext4|xfs|btrfs|f2fs/.test((x.fstype || '').toLowerCase()))) {
      return 'efi';
    }
    if (mp === '/' || mp === '/usr') return 'system';
    if (d.removable || p.removable) return 'usb';
    if (mp) return 'data';
    return 'unmounted';
  }

  function chipLabel(kind: Role, p: Partition) {
    switch (kind) {
      case 'swap':
        return 'Swap';
      case 'efi':
        return 'EFI';
      case 'boot':
        return 'Boot';
      case 'system':
        return 'System';
      case 'usb':
        return p.mountpoint ? 'USB' : 'USB';
      case 'data':
        return 'Data';
      default:
        return p.fstype ? p.fstype.toUpperCase() : 'Unmounted';
    }
  }

  function filesAction(p: Partition, d: Disk): 'open' | 'add' | null {
    const kind = role(p, d);
    if (kind === 'swap' || kind === 'efi' || kind === 'boot') return null;
    if (p.in_files) return 'open';
    if (kind === 'system') return null;
    if (p.mountpoint && p.mountpoint.startsWith('/')) return 'add';
    return null;
  }

  function showUsage(p: Partition, d: Disk) {
    const kind = role(p, d);
    return Boolean(p.mountpoint) && kind !== 'swap' && kind !== 'efi' && (p.total ?? p.size) > 0;
  }

  function displayMount(p: Partition, d: Disk) {
    const kind = role(p, d);
    if (kind === 'swap') return 'Swap';
    return p.mountpoint;
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

  function fallbackPart(d: Disk): Partition {
    return {
      name: d.name,
      path: d.path,
      size: d.size,
      fstype: '',
      label: title(d),
      uuid: '',
      mountpoint: '',
      removable: true,
      system: false,
      in_files: false
    };
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
      <div class="storage-ident">
        <strong>{title(d)}</strong>
        <div class="meta">{bytes(d.size)} · {d.path}{#if d.transport} · {d.transport}{/if}</div>
      </div>
      {#if d.health}
        <span class="chip" class:ok={d.health === 'passed'} class:warn={d.health !== 'passed'}>SMART {d.health}</span>
      {/if}
    </div>
    {#each d.partitions as p}
      {@const kind = role(p, d)}
      {@const used = p.used ?? 0}
      {@const total = p.total ?? p.size}
      {@const action = filesAction(p, d)}
      <div class="part">
        <div class="part-main">
          <div class="part-title">
            <span class="chip" class:ok={kind === 'system' || kind === 'data'} class:muted={kind === 'swap' || kind === 'efi' || kind === 'boot' || kind === 'unmounted'}>{chipLabel(kind, p)}</span>
            <strong>{p.label || p.name}</strong>
            <span class="meta">{p.fstype || 'unformatted'} · {bytes(p.size)}</span>
          </div>
          {#if showUsage(p, d)}
            <div class="bar"><i style="width:{pct(used, total)}%"></i></div>
            <div class="meta">{displayMount(p, d)} · {bytes(used)} / {bytes(total)}</div>
          {:else if p.mountpoint}
            <div class="meta">{displayMount(p, d)}</div>
          {/if}
        </div>
        {#if action}
          <div class="part-actions">
            {#if action === 'open'}
              <button class="btn compact secondary" onclick={() => go('/files')}>Open in Files</button>
            {:else}
              <button class="btn compact secondary" onclick={() => useInFiles(p.path)}>Use in Files</button>
            {/if}
          </div>
        {/if}
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
      <div class="storage-ident">
        <strong>{title(d)}</strong>
        <div class="meta">{bytes(d.size)} · {d.path}</div>
      </div>
      {#if d.health}
        <span class="chip" class:ok={d.health === 'passed'} class:warn={d.health !== 'passed'}>SMART {d.health}</span>
      {/if}
    </div>
    {#each d.partitions.length ? d.partitions : [fallbackPart(d)] as p}
      {@const kind = role(p, d)}
      {@const used = p.used ?? 0}
      {@const total = p.total ?? p.size}
      {@const action = filesAction(p, d)}
      <div class="part">
        <div class="part-main">
          <div class="part-title">
            <span class="chip" class:ok={Boolean(p.mountpoint)} class:muted={!p.mountpoint}>{p.mountpoint ? 'Mounted' : 'USB'}</span>
            <strong>{p.label || p.name}</strong>
            <span class="meta">{p.fstype || 'unformatted'} · {bytes(p.size)}</span>
          </div>
          {#if showUsage(p, d)}
            <div class="bar"><i style="width:{pct(used, total)}%"></i></div>
            <div class="meta">{displayMount(p, d)} · {bytes(used)} / {bytes(total)}</div>
          {/if}
        </div>
        <div class="part-actions">
          {#if p.mountpoint}
            <button class="btn compact secondary" disabled={!inv?.privileged || busy === p.path} onclick={() => eject(p.path)}>Eject</button>
            {#if action === 'open'}
              <button class="btn compact" onclick={() => go('/files')}>Open in Files</button>
            {:else if action === 'add'}
              <button class="btn compact" onclick={() => useInFiles(p.path)}>Use in Files</button>
            {/if}
          {:else if kind !== 'swap' && kind !== 'efi'}
            <button class="btn compact" disabled={!inv?.privileged || busy === p.path} onclick={() => mount(p.path)}>Mount</button>
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
