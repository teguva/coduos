<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { bytes, shortDate, when } from '../lib/format';
  import { fileUrl } from '../lib/fileKind';
  import { fileIcon, locationIcon, iconRev } from '../lib/icons';
  import FileViewer from '../components/FileViewer.svelte';
  import AppWindow from '../components/AppWindow.svelte';
  import Icon from '../components/Icon.svelte';
  import UiIcon from '../components/UiIcon.svelte';

  type Root = { id: string; label: string; path: string };
  type Fav = { root: string; path: string; label: string };
  type Entry = { name: string; path: string; dir: boolean; size: number; modified?: number | null };
  type List = {
    root: string;
    path: string;
    roots: Root[];
    entries: Entry[];
    favorites?: Fav[];
    space?: { used: number; total: number } | null;
  };
  type Clip = { op: 'copy' | 'cut'; root: string; items: { path: string; name: string }[] };

  const DRAG_MIME = 'application/x-coduos-files';

  let { onClose } = $props<{ onClose: () => void }>();

  let list = $state<List | null>(null);
  let error = $state('');
  let root = $state('');
  let path = $state('');
  let mkdirName = $state('');
  let showMkdir = $state(false);
  let dragging = $state(false);
  let dropOn = $state<string | null>(null);
  let menu = $state<{ x: number; y: number; ent: Entry | null } | null>(null);
  let view = $state<'list' | 'grid'>('grid');
  let query = $state('');
  let searchEl = $state<HTMLInputElement | null>(null);
  let favorites = $state<Fav[]>([]);
  let selected = $state<Set<string>>(new Set());
  let lastClicked = $state<string | null>(null);
  let clip = $state<Clip | null>(null);
  let preview = $state<Entry | null>(null);

  let filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const ents = list?.entries ?? [];
    if (!q) return ents;
    return ents.filter((e) => e.name.toLowerCase().includes(q));
  });

  let selectedEntries = $derived((list?.entries ?? []).filter((e) => selected.has(e.path)));

  async function load(nextRoot = root, nextPath = path) {
    if (nextRoot !== root || nextPath !== path) query = '';
    try {
      const q = new URLSearchParams();
      if (nextRoot) q.set('root', nextRoot);
      if (nextPath) q.set('path', nextPath);
      const data = await api<List>('/api/files?' + q.toString());
      list = data;
      root = data.root || data.roots[0]?.id || '';
      path = data.path;
      favorites = data.favorites ?? favorites;
      const keep = new Set((data.entries ?? []).map((e) => e.path));
      selected = new Set([...selected].filter((p) => keep.has(p)));
      if (!data.root && data.roots[0]) {
        await load(data.roots[0].id, '');
      }
      error = '';
    } catch (e: any) {
      error = e.message;
    }
  }

  function typingInField(ev: KeyboardEvent) {
    const el = ev.target as HTMLElement | null;
    if (!el) return false;
    const tag = el.tagName;
    return tag === 'INPUT' || tag === 'TEXTAREA' || el.isContentEditable;
  }

  onMount(() => {
    const q = new URLSearchParams(location.search);
    const r = q.get('root') || '';
    load(r, '');
    const hide = () => (menu = null);
    const onKey = (ev: KeyboardEvent) => {
      if (preview) return;
      if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === 'k') {
        ev.preventDefault();
        searchEl?.focus();
        return;
      }
      if (ev.key === 'Escape') {
        if (menu) {
          menu = null;
          return;
        }
        selected = new Set();
        return;
      }
      if (typingInField(ev)) return;
      const cmd = ev.ctrlKey || ev.metaKey;
      if (cmd && ev.key.toLowerCase() === 'a') {
        ev.preventDefault();
        selected = new Set(filtered.map((e) => e.path));
      } else if (cmd && ev.key.toLowerCase() === 'c') {
        ev.preventDefault();
        copySelection('copy');
      } else if (cmd && ev.key.toLowerCase() === 'x') {
        ev.preventDefault();
        copySelection('cut');
      } else if (cmd && ev.key.toLowerCase() === 'v') {
        ev.preventDefault();
        paste();
      } else if (ev.key === 'Enter') {
        const ents = selectedEntries;
        if (ents.length === 1) {
          ev.preventDefault();
          open(ents[0]);
        }
      } else if (ev.key === 'Delete' || ev.key === 'Backspace') {
        if (!selected.size) return;
        ev.preventDefault();
        removeMany(selectedEntries);
      }
    };
    window.addEventListener('click', hide);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('click', hide);
      window.removeEventListener('keydown', onKey);
    };
  });

  function crumbs() {
    const parts = path.split('/').filter(Boolean);
    const out: { label: string; path: string }[] = [
      { label: list?.roots.find((r) => r.id === root)?.label || root || 'Files', path: '' }
    ];
    let acc = '';
    for (const p of parts) {
      acc = acc ? acc + '/' + p : p;
      out.push({ label: p, path: acc });
    }
    return out;
  }

  function pathLabel() {
    const label = list?.roots.find((r) => r.id === root)?.label || root;
    const parts = path.split('/').filter(Boolean);
    return [label, ...parts].filter(Boolean).join(' / ');
  }

  function joinPath(dir: string, name: string) {
    return dir ? dir + '/' + name : name;
  }

  function isInside(fromPath: string, destDir: string) {
    return destDir === fromPath || destDir.startsWith(fromPath + '/');
  }

  function selectClick(ev: MouseEvent, ent: Entry) {
    menu = null;
    if (ev.ctrlKey || ev.metaKey) {
      const next = new Set(selected);
      if (next.has(ent.path)) next.delete(ent.path);
      else next.add(ent.path);
      selected = next;
      lastClicked = ent.path;
      return;
    }
    if (ev.shiftKey && lastClicked) {
      const items = filtered;
      const a = items.findIndex((e) => e.path === lastClicked);
      const b = items.findIndex((e) => e.path === ent.path);
      if (a >= 0 && b >= 0) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        selected = new Set(items.slice(lo, hi + 1).map((e) => e.path));
        return;
      }
    }
    selected = new Set([ent.path]);
    lastClicked = ent.path;
  }

  function open(ent: Entry) {
    menu = null;
    query = '';
    if (ent.dir) {
      selected = new Set();
      load(root, ent.path);
      return;
    }
    preview = ent;
  }

  function download(ent: Entry) {
    window.open(fileUrl(root, ent.path, false));
  }

  function copySelection(op: 'copy' | 'cut', ents?: Entry[]) {
    const items = ents ?? selectedEntries;
    if (!items.length) return;
    clip = { op, root, items: items.map((e) => ({ path: e.path, name: e.name })) };
    menu = null;
  }

  async function paste(destDir = path, destRoot = root) {
    menu = null;
    if (!clip?.items.length) return;
    if (clip.root !== destRoot) {
      error = 'Copy and move only work within the same storage location.';
      return;
    }
    try {
      for (const item of clip.items) {
        const name = item.path.split('/').pop() || item.name;
        const to = joinPath(destDir, name);
        if (isInside(item.path, destDir)) {
          error = 'Cannot place a folder inside itself.';
          continue;
        }
        if (clip.op === 'copy') {
          await api('/api/files/copy', {
            method: 'POST',
            body: JSON.stringify({ root: destRoot, from: item.path, to })
          });
        } else if (item.path !== to) {
          await api('/api/files/rename', {
            method: 'POST',
            body: JSON.stringify({ root: destRoot, from: item.path, to, unique: true })
          });
        }
      }
      if (clip.op === 'cut') clip = null;
      error = '';
      await load();
    } catch (e: any) {
      error = e.message;
    }
  }

  async function makeDir(e: Event) {
    e.preventDefault();
    if (!mkdirName) return;
    await api('/api/files/mkdir', { method: 'POST', body: JSON.stringify({ root, path, name: mkdirName }) });
    mkdirName = '';
    showMkdir = false;
    await load();
  }

  async function removeMany(ents: Entry[]) {
    if (!ents.length) return;
    const label = ents.length === 1 ? ents[0].name : `${ents.length} items`;
    if (!confirm(`Delete ${label}?`)) return;
    try {
      for (const ent of ents) {
        await api('/api/files/delete', { method: 'POST', body: JSON.stringify({ root, path: ent.path }) });
      }
      menu = null;
      selected = new Set();
      await load();
    } catch (e: any) {
      error = e.message;
    }
  }

  async function rename(ent: Entry) {
    const next = prompt('New name', ent.name);
    if (!next || next === ent.name) return;
    const parent = ent.path.split('/').slice(0, -1).join('/');
    const to = parent ? parent + '/' + next : next;
    try {
      await api('/api/files/rename', { method: 'POST', body: JSON.stringify({ root, from: ent.path, to }) });
      menu = null;
      await load();
    } catch (e: any) {
      error = e.message;
    }
  }

  function isStarred(r: string, p: string) {
    return favorites.some((f) => f.root === r && f.path === p);
  }

  async function toggleStar(ent?: { root?: string; path: string; name: string }) {
    const r = ent?.root ?? root;
    const p = ent?.path ?? path;
    const label = ent?.name || p.split('/').pop() || list?.roots.find((x) => x.id === r)?.label || r;
    const next = isStarred(r, p)
      ? favorites.filter((f) => !(f.root === r && f.path === p))
      : [...favorites, { root: r, path: p, label }];
    try {
      const s = await api<{ file_favorites?: Fav[] }>('/api/settings', {
        method: 'PUT',
        body: JSON.stringify({ file_favorites: next })
      });
      favorites = s.file_favorites ?? next;
    } catch (e: any) {
      error = e.message;
    }
    menu = null;
  }

  async function uploadFiles(files: FileList | File[], destRoot = root, destPath = path) {
    const fd = new FormData();
    for (const f of files) fd.append('file', f);
    const q = new URLSearchParams({ root: destRoot, path: destPath });
    await api('/api/files/upload?' + q.toString(), { method: 'POST', body: fd });
    await load();
  }

  async function onFileInput(ev: Event) {
    const input = ev.target as HTMLInputElement;
    if (!input.files?.length) return;
    await uploadFiles(input.files);
    input.value = '';
  }

  function dragPayload(ev: DragEvent): { root: string; paths: string[] } | null {
    const raw = ev.dataTransfer?.getData(DRAG_MIME) || ev.dataTransfer?.getData('text/plain') || '';
    if (!raw) return null;
    try {
      const data = JSON.parse(raw);
      if (data && Array.isArray(data.paths) && typeof data.root === 'string') return data;
    } catch {
      /* not ours */
    }
    return null;
  }

  function hasInternalDrag(ev: DragEvent) {
    const types = [...(ev.dataTransfer?.types ?? [])];
    return types.includes(DRAG_MIME);
  }

  function onDragStart(ev: DragEvent, ent: Entry) {
    if (!selected.has(ent.path)) selected = new Set([ent.path]);
    const payload = JSON.stringify({ root, paths: [...selected] });
    ev.dataTransfer?.setData(DRAG_MIME, payload);
    ev.dataTransfer?.setData('text/plain', payload);
    if (ev.dataTransfer) ev.dataTransfer.effectAllowed = 'copyMove';
  }

  function onDragOverList(ev: DragEvent) {
    ev.preventDefault();
    if (ev.dataTransfer) {
      ev.dataTransfer.dropEffect = hasInternalDrag(ev) ? 'move' : 'copy';
    }
    dragging = !hasInternalDrag(ev);
  }

  function dropKey(kind: 'crumb' | 'loc' | 'fav' | 'ent', id: string) {
    return `${kind}:${id}`;
  }

  function onDragOverTarget(ev: DragEvent, key: string) {
    ev.preventDefault();
    ev.stopPropagation();
    if (ev.dataTransfer) ev.dataTransfer.dropEffect = 'move';
    dropOn = key;
    dragging = false;
  }

  async function movePaths(paths: string[], destRoot: string, destDir: string) {
    if (destRoot !== root) {
      error = 'Move only works within the same storage location.';
      return;
    }
    for (const from of paths) {
      const name = from.split('/').pop() || from;
      if (isInside(from, destDir)) continue;
      const to = joinPath(destDir, name);
      if (from === to) continue;
      await api('/api/files/rename', {
        method: 'POST',
        body: JSON.stringify({ root: destRoot, from, to, unique: true })
      });
    }
    selected = new Set();
    await load();
  }

  async function onDropAt(ev: DragEvent, destRoot: string, destDir: string) {
    ev.preventDefault();
    ev.stopPropagation();
    dragging = false;
    dropOn = null;
    const internal = dragPayload(ev);
    try {
      if (internal) {
        await movePaths(internal.paths, destRoot, destDir);
        return;
      }
      if (ev.dataTransfer?.files?.length) {
        await uploadFiles(ev.dataTransfer.files, destRoot, destDir);
      }
    } catch (e: any) {
      error = e.message;
    }
  }

  function onDropList(ev: DragEvent) {
    onDropAt(ev, root, path);
  }

  function openContext(ev: MouseEvent, ent: Entry | null) {
    ev.preventDefault();
    ev.stopPropagation();
    if (ent && !selected.has(ent.path)) selected = new Set([ent.path]);
    menu = { x: ev.clientX, y: ev.clientY, ent };
  }

  function parentPath() {
    const parts = path.split('/').filter(Boolean);
    parts.pop();
    return parts.join('/');
  }

  function clipLabel() {
    if (!clip?.items.length) return '';
    const n = clip.items.length;
    const verb = clip.op === 'cut' ? 'Cut' : 'Copied';
    return n === 1 ? `${verb} ${clip.items[0].name}` : `${verb} ${n} items`;
  }
</script>

<AppWindow title="Files" icon="folder" size="sheet" flush {onClose}>
  {#snippet actions()}
    {#if selected.size}
      <button class="btn secondary compact" onclick={() => copySelection('copy')}>
        <UiIcon name="content_copy" size={18} /> Copy
      </button>
      <button class="btn secondary compact" onclick={() => copySelection('cut')}>
        <UiIcon name="content_cut" size={18} /> Cut
      </button>
      {#if clip}
        <button class="btn secondary compact" onclick={() => paste()}>
          <UiIcon name="content_paste" size={18} /> Paste
        </button>
      {/if}
      <button class="btn secondary compact" onclick={() => removeMany(selectedEntries)}>
        <UiIcon name="delete" size={18} /> Delete
      </button>
    {:else}
      {#if clip}
        <button class="btn secondary compact" onclick={() => paste()}>
          <UiIcon name="content_paste" size={18} /> Paste
        </button>
      {/if}
      <button class="btn secondary compact" onclick={() => (showMkdir = !showMkdir)}>
        <UiIcon name="create_new_folder" size={18} /> New folder
      </button>
      <label class="btn secondary compact">
        <UiIcon name="upload" size={18} /> Import
        <input type="file" multiple hidden onchange={onFileInput} />
      </label>
    {/if}
    <button class="btn secondary icon-only compact" onclick={() => (view = view === 'list' ? 'grid' : 'list')} aria-label={view === 'list' ? 'Grid view' : 'List view'}>
      <UiIcon name={view === 'list' ? 'grid_view' : 'view_list'} size={18} />
    </button>
  {/snippet}

{#key $iconRev}
<div class="files-card">
    <aside class="files-side">
      <label class="files-search">
        <UiIcon name="search" size={18} />
        <input
          bind:this={searchEl}
          bind:value={query}
          placeholder="Search"
          aria-label="Search this folder"
        />
        <kbd>Ctrl+K</kbd>
      </label>
      {#if favorites.length}
        <h3>Starred</h3>
        {#each favorites as f}
          <button
            class="loc"
            class:active={f.root === root && f.path === path}
            class:drop-ok={dropOn === dropKey('fav', f.root + '/' + f.path)}
            onclick={() => load(f.root, f.path)}
            ondragover={(e) => onDragOverTarget(e, dropKey('fav', f.root + '/' + f.path))}
            ondragleave={() => (dropOn = null)}
            ondrop={(e) => onDropAt(e, f.root, f.path)}
          >
            <Icon name="folder" size={20} alt="" />
            {f.label}
          </button>
        {/each}
      {/if}
      <h3>Storage</h3>
      {#each list?.roots ?? [] as r}
        <button
          class="loc"
          class:active={r.id === root && !path}
          class:drop-ok={dropOn === dropKey('loc', r.id)}
          onclick={() => load(r.id, '')}
          ondragover={(e) => onDragOverTarget(e, dropKey('loc', r.id))}
          ondragleave={() => (dropOn = null)}
          ondrop={(e) => onDropAt(e, r.id, '')}
        >
          <Icon name={locationIcon(r.id, r.label)} size={22} alt="" />
          {r.label}
        </button>
      {/each}
    </aside>
    <div class="files-main">
      <header class="files-toolbar">
        <button
          class="btn secondary icon-only compact"
          disabled={!path}
          onclick={() => load(root, parentPath())}
          aria-label="Up"
        >
          <UiIcon name="arrow_upward" size={18} />
        </button>
        <div class="addr">
          {#each crumbs() as c, i}
            {#if i > 0}<span>/</span>{/if}
            <button
              class:drop-ok={dropOn === dropKey('crumb', c.path)}
              onclick={() => load(root, c.path)}
              ondragover={(e) => onDragOverTarget(e, dropKey('crumb', c.path))}
              ondragleave={() => (dropOn = null)}
              ondrop={(e) => onDropAt(e, root, c.path)}
            >{c.label}</button>
          {/each}
          <span class="chip muted">{filtered.length} {filtered.length === 1 ? 'item' : 'items'}</span>
        </div>
      </header>
      {#if showMkdir}
        <form class="files-toolbar" onsubmit={makeDir}>
          <input placeholder="Folder name" bind:value={mkdirName} />
          <button class="btn">Create</button>
        </form>
      {/if}
      {#if error}<div class="err" style="padding:0 16px">{error}</div>{/if}
      <div
        class="files-list"
        class:drag={dragging}
        role="presentation"
        onclick={() => { selected = new Set(); menu = null; }}
        oncontextmenu={(e) => openContext(e, null)}
        ondragover={onDragOverList}
        ondragleave={() => (dragging = false)}
        ondrop={onDropList}
      >
        {#if filtered.length === 0}
          <div class="files-empty">
            <Icon name="folder-open" size={64} alt="" />
            <div>{query ? 'No matching items.' : 'This folder is empty. Drop files here or create a folder.'}</div>
          </div>
        {:else if view === 'grid'}
          <div class="files-grid">
            {#each filtered as ent}
              <button
                class="files-tile"
                class:selected={selected.has(ent.path)}
                class:drop-ok={ent.dir && dropOn === dropKey('ent', ent.path)}
                draggable="true"
                onclick={(e) => { e.stopPropagation(); selectClick(e, ent); }}
                ondblclick={(e) => { e.stopPropagation(); open(ent); }}
                oncontextmenu={(e) => openContext(e, ent)}
                ondragstart={(e) => onDragStart(e, ent)}
                ondragover={(e) => {
                  if (ent.dir) onDragOverTarget(e, dropKey('ent', ent.path));
                  else {
                    e.stopPropagation();
                    onDragOverList(e);
                  }
                }}
                ondragleave={ent.dir ? () => (dropOn = null) : undefined}
                ondrop={(e) => onDropAt(e, root, ent.dir ? ent.path : path)}
              >
                <Icon name={fileIcon(ent.name, ent.dir)} size={52} class="tile-img" alt="" />
                <div class="label">{ent.name}</div>
                <div class="meta">{shortDate(ent.modified)}</div>
              </button>
            {/each}
          </div>
        {:else}
          <table class="table">
            <thead><tr><th>Name</th><th>Size</th><th>Modified</th></tr></thead>
            <tbody>
              {#each filtered as ent}
                <tr
                  class:selected={selected.has(ent.path)}
                  class:drop-ok={ent.dir && dropOn === dropKey('ent', ent.path)}
                  draggable="true"
                  onclick={(e) => { e.stopPropagation(); selectClick(e, ent); }}
                  ondblclick={(e) => { e.stopPropagation(); open(ent); }}
                  oncontextmenu={(e) => openContext(e, ent)}
                  ondragstart={(e) => onDragStart(e, ent)}
                  ondragover={(e) => {
                    if (ent.dir) onDragOverTarget(e, dropKey('ent', ent.path));
                    else {
                      e.stopPropagation();
                      onDragOverList(e);
                    }
                  }}
                  ondragleave={ent.dir ? () => (dropOn = null) : undefined}
                  ondrop={(e) => onDropAt(e, root, ent.dir ? ent.path : path)}
                >
                  <td>
                    <span class="file-name">
                      <Icon name={fileIcon(ent.name, ent.dir)} size={22} alt="" />
                      {ent.name}
                    </span>
                  </td>
                  <td>{ent.dir ? '—' : bytes(ent.size)}</td>
                  <td>{when(ent.modified)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
      <footer class="files-foot">
        <span class="clip">{pathLabel()}</span>
        {#if selected.size}
          <span class="meta">{selected.size} selected</span>
        {:else if clip}
          <span class="meta">{clipLabel()}</span>
        {/if}
        {#if list?.space}
          <span class="meta">{bytes(list.space.total - list.space.used)} available / {bytes(list.space.total)}</span>
        {/if}
      </footer>
    </div>
  </div>
  {/key}
</AppWindow>

{#if menu}
  <div class="ctx" style="left:{menu.x}px; top:{menu.y}px" role="menu">
    {#if menu.ent}
      <button onclick={() => open(menu!.ent!)}>Open</button>
    {/if}
    {#if menu.ent && !menu.ent.dir}
      <button onclick={() => download(menu!.ent!)}>
        <UiIcon name="download" size={16} /> Download
      </button>
    {/if}
    {#if menu.ent}
      <button onclick={() => copySelection('copy', selectedEntries.length ? selectedEntries : [menu!.ent!])}>Copy</button>
      <button onclick={() => copySelection('cut', selectedEntries.length ? selectedEntries : [menu!.ent!])}>Cut</button>
    {/if}
    {#if clip}
      <button onclick={() => paste(menu?.ent?.dir ? menu.ent.path : path)}>Paste</button>
    {/if}
    {#if menu.ent}
      <button onclick={() => toggleStar({ path: menu!.ent!.path, name: menu!.ent!.name })}>
        {isStarred(root, menu.ent.path) ? 'Unstar' : 'Star'}
      </button>
      <button onclick={() => rename(menu!.ent!)}>Rename</button>
      <button onclick={() => removeMany(selectedEntries.length ? selectedEntries : [menu!.ent!])}>
        <UiIcon name="delete" size={16} /> Delete
      </button>
    {/if}
  </div>
{/if}

{#if preview}
  <FileViewer
    {root}
    entry={preview}
    siblings={list?.entries ?? []}
    onClose={() => (preview = null)}
    onChange={(ent) => (preview = ent)}
  />
{/if}
