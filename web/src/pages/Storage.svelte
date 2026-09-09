<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { bytes, pct } from '../lib/format';
  import UiIcon from '../components/UiIcon.svelte';
  import InfoTip from '../components/InfoTip.svelte';
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
    files_label?: string | null;
    auto_mount: boolean;
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
  let formatDlg = $state<{ device: string; title: string } | null>(null);
  let fmtFs = $state('ext4');
  let fmtLabel = $state('');
  let mountDlg = $state<{
    device: string;
    title: string;
    fstype: string;
    uuid: string;
    size: number;
  } | null>(null);
  let mntFolder = $state('');
  let mntFilesName = $state('');
  let mntAddFiles = $state(true);
  let mntAuto = $state(true);
  let mntRo = $state(false);

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

  function canManage(d: Disk) {
    return !d.system;
  }

  function canAutoMount(p: Partition, d: Disk) {
    if (!canManage(d) || !p.uuid) return false;
    const kind = role(p, d);
    return kind !== 'swap' && kind !== 'efi' && kind !== 'boot' && kind !== 'system';
  }

  function folderSlug(s: string) {
    const out = s
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 48);
    return out || 'disk';
  }

  function mountPath() {
    return '/media/coduos/' + folderSlug(mntFolder || 'disk');
  }

  function openMount(p: Partition) {
    mntFolder = folderSlug(p.label || p.name || 'disk');
    mntFilesName = p.label || p.name || 'Disk';
    mntAddFiles = true;
    mntAuto = p.auto_mount !== false;
    mntRo = false;
    mountDlg = {
      device: p.path,
      title: p.label || p.name,
      fstype: p.fstype,
      uuid: p.uuid,
      size: p.size
    };
  }

  function displayWhere(p: Partition, kind: Role) {
    if (kind === 'swap') return 'Swap';
    const bits: string[] = [];
    if (p.mountpoint) bits.push(`Mounted at ${p.mountpoint}`);
    if (p.files_label) bits.push(`in Files as ${p.files_label}`);
    else if (p.in_files) bits.push('in Files');
    return bits.join(' · ');
  }

  async function setAuto(device: string, enabled: boolean) {
    error = '';
    busy = device;
    try {
      await api('/api/storage/auto-mount', {
        method: 'POST',
        body: JSON.stringify({ device, enabled })
      });
      await load();
    } catch (e: any) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  async function doMount() {
    if (!mountDlg) return;
    const device = mountDlg.device;
    error = '';
    busy = device;
    try {
      await api('/api/storage/mount', {
        method: 'POST',
        body: JSON.stringify({
          device,
          folder: folderSlug(mntFolder || 'disk'),
          files_name: mntFilesName.trim() || undefined,
          add_to_files: mntAddFiles,
          auto_mount: mntAuto,
          read_only: mntRo
        })
      });
      mountDlg = null;
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

  async function doFormat() {
    if (!formatDlg) return;
    const device = formatDlg.device;
    error = '';
    busy = device;
    try {
      await api('/api/storage/format', {
        method: 'POST',
        body: JSON.stringify({ device, fstype: fmtFs, label: fmtLabel })
      });
      formatDlg = null;
      await load();
    } catch (e: any) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  function openFormat(device: string, title: string, label: string) {
    fmtFs = 'ext4';
    fmtLabel = label || 'data';
    formatDlg = { device, title };
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
      in_files: false,
      files_label: null,
      auto_mount: false
    };
  }
</script>

{#if !inv?.privileged}
  <div class="banner">Mount, format, and eject need the installed daemon running as root. Inventory is still shown.</div>
{/if}
{#if error}<div class="err">{error}</div>{/if}

<h3 class="group-title">This device</h3>
{#if inv && internal.length === 0}
  <p class="hint">No internal disks reported.</p>
{/if}
{#each internal as d}
  <div class="storage-card">
    <div class="storage-head">
      <UiIcon name="storage" size={28} />
      <div class="storage-ident">
        <strong>{title(d)}</strong>
        <div class="meta">{bytes(d.size)} · {d.path}{#if d.transport} · {d.transport}{/if}</div>
      </div>
      {#if d.system}
        <span class="chip muted">System disk</span>
      {/if}
      {#if d.health}
        <span class="chip" class:ok={d.health === 'passed'} class:warn={d.health !== 'passed'}>SMART {d.health}</span>
      {/if}
    </div>
    {#if d.system}
      <p class="hint">Left alone because it holds OS partitions.</p>
    {/if}
    {#each d.partitions.length ? d.partitions : canManage(d) ? [fallbackPart(d)] : [] as p}
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
            <div class="meta">{displayWhere(p, kind)} · {bytes(used)} / {bytes(total)}</div>
          {:else if p.mountpoint}
            <div class="meta">{displayWhere(p, kind)}</div>
          {/if}
          {#if canAutoMount(p, d)}
            <label class="toggle-row storage-auto">
              <span>Mount at startup</span>
              <input
                type="checkbox"
                checked={p.auto_mount}
                disabled={!inv?.privileged || busy === p.path}
                onchange={(e) => setAuto(p.path, e.currentTarget.checked)}
              />
            </label>
          {/if}
        </div>
        {#if canManage(d)}
          <div class="part-actions">
            {#if p.mountpoint}
              <button class="btn compact secondary" disabled={!inv?.privileged || busy === p.path} onclick={() => eject(p.path)}>Unmount</button>
              {#if action === 'open'}
                <button class="btn compact secondary" onclick={() => go('/files')}>Open in Files</button>
              {:else if action === 'add'}
                <button class="btn compact secondary" onclick={() => useInFiles(p.path)}>Use in Files</button>
              {/if}
            {:else if kind !== 'swap'}
              <button class="btn compact" disabled={!inv?.privileged || busy === p.path} onclick={() => openMount(p)}>Mount</button>
            {/if}
            <button class="btn compact danger" disabled={!inv?.privileged || busy === p.path || busy === d.path} onclick={() => openFormat(p.path === d.path ? d.path : p.path, p.label || p.name, p.label)}>Format</button>
          </div>
        {:else if action}
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
    {#if canManage(d) && d.partitions.length > 0}
      <div class="part-actions" style="margin-top:8px">
        <button class="btn compact danger" disabled={!inv?.privileged || busy === d.path} onclick={() => openFormat(d.path, title(d), title(d))}>Format whole disk</button>
      </div>
    {/if}
  </div>
{/each}

<h3 class="group-title">External</h3>
{#if inv && external.length === 0}
  <p class="hint">Plug in a drive.</p>
{/if}
{#each external as d}
  <div class="storage-card">
    <div class="storage-head">
      <UiIcon name="storage" size={28} />
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
            <div class="meta">{displayWhere(p, kind)} · {bytes(used)} / {bytes(total)}</div>
          {:else if p.mountpoint}
            <div class="meta">{displayWhere(p, kind)}</div>
          {/if}
          {#if canAutoMount(p, d)}
            <label class="toggle-row storage-auto">
              <span>Mount at startup</span>
              <input
                type="checkbox"
                checked={p.auto_mount}
                disabled={!inv?.privileged || busy === p.path}
                onchange={(e) => setAuto(p.path, e.currentTarget.checked)}
              />
            </label>
          {/if}
        </div>
        <div class="part-actions">
          {#if canManage(d)}
            {#if p.mountpoint}
              <button class="btn compact secondary" disabled={!inv?.privileged || busy === p.path} onclick={() => eject(p.path)}>Eject</button>
              {#if action === 'open'}
                <button class="btn compact" onclick={() => go('/files')}>Open in Files</button>
              {:else if action === 'add'}
                <button class="btn compact" onclick={() => useInFiles(p.path)}>Use in Files</button>
              {/if}
            {:else if kind !== 'swap'}
              <button class="btn compact" disabled={!inv?.privileged || busy === p.path} onclick={() => openMount(p)}>Mount</button>
            {/if}
            <button class="btn compact danger" disabled={!inv?.privileged || busy === p.path || busy === d.path} onclick={() => openFormat(p.path === d.path ? d.path : p.path, p.label || p.name, p.label)}>Format</button>
          {:else if p.mountpoint}
            {#if action === 'open'}
              <button class="btn compact" onclick={() => go('/files')}>Open in Files</button>
            {:else if action === 'add'}
              <button class="btn compact" onclick={() => useInFiles(p.path)}>Use in Files</button>
            {/if}
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

{#if mountDlg}
  <div class="confirm-bg" role="presentation" onclick={(e) => { if (e.currentTarget === e.target) mountDlg = null; }}>
    <div class="confirm-card" role="dialog" aria-label="Mount drive">
      <h3>Mount {mountDlg.title}</h3>
      <p>
        {mountDlg.device}{#if mountDlg.fstype} · {mountDlg.fstype}{/if} · {bytes(mountDlg.size)}
        {#if mountDlg.uuid}<br />UUID {mountDlg.uuid}{/if}
      </p>
      <label class="field">
        <span class="field-head">
          Folder name
          <InfoTip
            label="Where this mounts"
            text="CoduOS mounts by UUID under /media/coduos so the same disk keeps this folder after reboot, even if the /dev name changes."
          />
        </span>
        <input bind:value={mntFolder} placeholder="backup" />
      </label>
      <div class="mount-path">{mountPath()}</div>
      <label class="field">
        <span>Name in Files</span>
        <input bind:value={mntFilesName} placeholder="Backup" disabled={!mntAddFiles} />
      </label>
      <label class="toggle-row">
        <span>Show in Files</span>
        <input type="checkbox" bind:checked={mntAddFiles} />
      </label>
      <label class="toggle-row">
        <span>Mount at startup</span>
        <input type="checkbox" bind:checked={mntAuto} />
      </label>
      <label class="toggle-row">
        <span>Read only</span>
        <input type="checkbox" bind:checked={mntRo} />
      </label>
      <p>Linux filesystems keep their own permissions. FAT, exFAT, and NTFS are mounted so the CoduOS user can write.</p>
      <div class="row">
        <button class="btn secondary" onclick={() => (mountDlg = null)}>Cancel</button>
        <button class="btn" disabled={!!busy} onclick={doMount}>Mount</button>
      </div>
    </div>
  </div>
{/if}

{#if formatDlg}
  <div class="confirm-bg" role="presentation" onclick={(e) => { if (e.currentTarget === e.target) formatDlg = null; }}>
    <div class="confirm-card" role="dialog" aria-label="Format drive">
      <h3>Format {formatDlg.title}?</h3>
      <p class="danger-text">This erases everything on {formatDlg.device}. CoduOS then mounts it at /media/coduos/{folderSlug(fmtLabel || 'data')}.</p>
      <label class="field"><span>Filesystem</span>
        <select bind:value={fmtFs}>
          <option value="ext4">ext4 (Linux)</option>
          <option value="xfs">XFS</option>
          <option value="btrfs">Btrfs</option>
          <option value="exfat">exFAT (USB / Windows)</option>
        </select>
      </label>
      <label class="field"><span>Label</span>
        <input bind:value={fmtLabel} maxlength="16" placeholder="data" />
      </label>
      <div class="row">
        <button class="btn secondary" onclick={() => (formatDlg = null)}>Cancel</button>
        <button class="btn danger" disabled={!!busy} onclick={doFormat}>Format</button>
      </div>
    </div>
  </div>
{/if}
