<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { fileUrl } from '../lib/fileKind';
  import { locationIcon, iconRev } from '../lib/icons';
  import FileViewer from '../components/FileViewer.svelte';
  import FolderView from '../components/FolderView.svelte';
  import type { FileEntry, FilePane } from '../lib/filePane';
  import AppWindow from '../components/AppWindow.svelte';
  import Icon from '../components/Icon.svelte';
  import UiIcon from '../components/UiIcon.svelte';

  type Root = { id: string; label: string; path: string };
  type Fav = { root: string; path: string; label: string };
  type Entry = FileEntry;
  type List = {
    root: string;
    path: string;
    roots: Root[];
    entries: Entry[];
    favorites?: Fav[];
    space?: { used: number; total: number } | null;
  };
  type Clip = { op: 'copy' | 'cut'; root: string; items: { path: string; name: string }[] };
  type Tab = { id: string; panes: FilePane[]; activePane: number };

  const DRAG_MIME = 'application/x-coduos-files';
  const SPLIT_MIN = 1100;
  const SPLIT_WIDE = 1500;

  let { onClose } = $props<{ onClose: () => void }>();

  let seq = 0;
  function nid() {
    seq += 1;
    return 'f' + seq;
  }

  function makePane(root = '', path = ''): FilePane {
    return {
      id: nid(),
      root,
      path,
      list: null,
      query: '',
      selected: new Set(),
      lastClicked: null,
      error: ''
    };
  }

  function makeTab(root = '', path = ''): Tab {
    return { id: nid(), panes: [makePane(root, path)], activePane: 0 };
  }

  let tabs = $state<Tab[]>([makeTab()]);
  let tabI = $state(0);
  let error = $state('');
  let mkdirName = $state('');
  let showMkdir = $state(false);
  let dragging = $state(false);
  let dragPane = $state<string | null>(null);
  let dropOn = $state<string | null>(null);
  let menu = $state<{ x: number; y: number; ent: Entry | null } | null>(null);
  let view = $state<'list' | 'grid'>('grid');
  let searchEl = $state<HTMLInputElement | null>(null);
  let favorites = $state<Fav[]>([]);
  let roots = $state<Root[]>([]);
  let clip = $state<Clip | null>(null);
  let preview = $state<Entry | null>(null);
  let canSplit = $state(false);
  let maxPanes = $state(2);

  let tab = $derived(tabs[tabI] ?? tabs[0]);
  let pane = $derived(tab.panes[tab.activePane] ?? tab.panes[0]);
  let selectedEntries = $derived((pane.list?.entries ?? []).filter((e) => pane.selected.has(e.path)));

  function paneTitle(p: FilePane) {
    const parts = p.path.split('/').filter(Boolean);
    if (parts.length) return parts[parts.length - 1];
    return roots.find((r) => r.id === p.root)?.label || p.list?.roots.find((r) => r.id === p.root)?.label || p.root || 'Files';
  }

  function tabTitle(t: Tab) {
    const names = t.panes.map(paneTitle);
    return [...new Set(names)].join(' · ') || 'Files';
  }

  async function loadPane(target: FilePane, nextRoot = target.root, nextPath = target.path) {
    if (nextRoot !== target.root || nextPath !== target.path) target.query = '';
    try {
      const q = new URLSearchParams();
      if (nextRoot) q.set('root', nextRoot);
      if (nextPath) q.set('path', nextPath);
      const data = await api<List>('/api/files?' + q.toString());
      target.list = data;
      target.root = data.root || data.roots[0]?.id || '';
      target.path = data.path;
      target.error = '';
      if (data.roots) roots = data.roots;
      favorites = data.favorites ?? favorites;
      const keep = new Set((data.entries ?? []).map((e) => e.path));
      target.selected = new Set([...target.selected].filter((p) => keep.has(p)));
      if (!data.root && data.roots[0]) {
        await loadPane(target, data.roots[0].id, '');
      }
      error = '';
    } catch (e: any) {
      target.error = e.message;
      error = e.message;
    }
  }

  async function reloadAll() {
    await Promise.all(tabs.flatMap((t) => t.panes.map((p) => loadPane(p, p.root, p.path))));
  }

  function activatePane(t: Tab, i: number) {
    t.activePane = i;
  }

  function currentPane() {
    const t = tabs[tabI] ?? tabs[0];
    return t.panes[t.activePane] ?? t.panes[0];
  }

  async function openTab(root: string, path: string, after = tabI + 1) {
    const t = makeTab(root, path);
    tabs = [...tabs.slice(0, after), t, ...tabs.slice(after)];
    tabI = after;
    const pane = tabs[after].panes[0];
    await loadPane(pane, root, path);
  }

  async function newTab() {
    const src = currentPane();
    await openTab(src.root, src.path);
  }

  function closeTab(i: number) {
    if (tabs.length < 2) return;
    const nextI = tabI === i ? Math.max(0, i - 1) : tabI > i ? tabI - 1 : tabI;
    tabs = tabs.filter((_, j) => j !== i);
    tabI = Math.min(nextI, tabs.length - 1);
  }

  async function splitPane() {
    const t = tabs[tabI];
    if (!canSplit || t.panes.length >= maxPanes) return;
    const src = t.panes[t.activePane];
    t.panes = [...t.panes, makePane(src.root, src.path)];
    t.activePane = t.panes.length - 1;
    const added = t.panes[t.activePane];
    await loadPane(added, added.root, added.path);
  }

  function closePane(t: Tab, i: number) {
    if (t.panes.length < 2) return;
    t.panes = t.panes.filter((_, j) => j !== i);
    if (t.activePane >= t.panes.length) t.activePane = t.panes.length - 1;
  }

  async function openBeside(root: string, path: string) {
    const t = tabs[tabI];
    if (!canSplit) {
      await openTab(root, path);
      return;
    }
    if (t.panes.length < 2) await splitPane();
    const other = t.panes.findIndex((_, i) => i !== t.activePane);
    const dest = t.panes[other >= 0 ? other : t.panes.length - 1];
    t.activePane = t.panes.indexOf(dest);
    await loadPane(dest, root, path);
  }

  function flattenSplits() {
    const next: Tab[] = [];
    for (const t of tabs) {
      t.panes.forEach((p, i) => {
        if (i === 0) next.push({ id: t.id, panes: [p], activePane: 0 });
        else next.push({ id: p.id, panes: [p], activePane: 0 });
      });
    }
    const keep = tabs[tabI]?.panes[tabs[tabI].activePane]?.id;
    tabs = next;
    const idx = tabs.findIndex((t) => t.panes.some((p) => p.id === keep));
    tabI = idx >= 0 ? idx : 0;
  }

  function applyWidth() {
    const w = window.innerWidth;
    const wide = w >= SPLIT_MIN;
    maxPanes = w >= SPLIT_WIDE ? 3 : 2;
    if (canSplit && !wide) flattenSplits();
    canSplit = wide;
  }

  function typingInField(ev: KeyboardEvent) {
    const el = ev.target as HTMLElement | null;
    if (!el) return false;
    const tag = el.tagName;
    return tag === 'INPUT' || tag === 'TEXTAREA' || el.isContentEditable;
  }

  function locClick(ev: MouseEvent, root: string, path: string) {
    if (ev.ctrlKey || ev.metaKey) {
      ev.preventDefault();
      openTab(root, path);
      return;
    }
    loadPane(currentPane(), root, path);
  }

  onMount(() => {
    const q = new URLSearchParams(location.search);
    const r = q.get('root') || '';
    loadPane(currentPane(), r, '');
    applyWidth();
    const hide = () => (menu = null);
    const onResize = () => applyWidth();
    const onKey = (ev: KeyboardEvent) => {
      if (preview) return;
      const cmd = ev.ctrlKey || ev.metaKey;
      if (cmd && ev.key.toLowerCase() === 'k') {
        ev.preventDefault();
        searchEl?.focus();
        return;
      }
      if (cmd && ev.key.toLowerCase() === 't') {
        ev.preventDefault();
        newTab();
        return;
      }
      if (ev.key === 'Escape') {
        if (menu) {
          menu = null;
          return;
        }
        currentPane().selected = new Set();
        return;
      }
      if (typingInField(ev)) return;
      if (cmd && ev.key.toLowerCase() === 'a') {
        ev.preventDefault();
        const p = currentPane();
        const qstr = p.query.trim().toLowerCase();
        const ents = (p.list?.entries ?? []).filter((e) => !qstr || e.name.toLowerCase().includes(qstr));
        p.selected = new Set(ents.map((e) => e.path));
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
        const p = currentPane();
        if (!p.selected.size) return;
        ev.preventDefault();
        removeMany(selectedEntries);
      }
    };
    window.addEventListener('click', hide);
    window.addEventListener('keydown', onKey);
    window.addEventListener('resize', onResize);
    return () => {
      window.removeEventListener('click', hide);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('resize', onResize);
    };
  });

  function joinPath(dir: string, name: string) {
    return dir ? dir + '/' + name : name;
  }

  function isInside(fromPath: string, destDir: string) {
    return destDir === fromPath || destDir.startsWith(fromPath + '/');
  }

  function selectClick(ev: MouseEvent, ent: Entry) {
    const p = currentPane();
    menu = null;
    if (ev.ctrlKey || ev.metaKey) {
      const next = new Set(p.selected);
      if (next.has(ent.path)) next.delete(ent.path);
      else next.add(ent.path);
      p.selected = next;
      p.lastClicked = ent.path;
      return;
    }
    if (ev.shiftKey && p.lastClicked) {
      const qstr = p.query.trim().toLowerCase();
      const items = (p.list?.entries ?? []).filter((e) => !qstr || e.name.toLowerCase().includes(qstr));
      const a = items.findIndex((e) => e.path === p.lastClicked);
      const b = items.findIndex((e) => e.path === ent.path);
      if (a >= 0 && b >= 0) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        p.selected = new Set(items.slice(lo, hi + 1).map((e) => e.path));
        return;
      }
    }
    p.selected = new Set([ent.path]);
    p.lastClicked = ent.path;
  }

  function open(ent: Entry, ev?: MouseEvent) {
    menu = null;
    const p = currentPane();
    if (ent.dir) {
      if (ev && (ev.ctrlKey || ev.metaKey)) {
        openTab(p.root, ent.path);
        return;
      }
      p.selected = new Set();
      loadPane(p, p.root, ent.path);
      return;
    }
    preview = ent;
  }

  function download(ent: Entry) {
    window.open(fileUrl(currentPane().root, ent.path, false));
  }

  function copySelection(op: 'copy' | 'cut', ents?: Entry[]) {
    const p = currentPane();
    const items = ents ?? selectedEntries;
    if (!items.length) return;
    clip = { op, root: p.root, items: items.map((e) => ({ path: e.path, name: e.name })) };
    menu = null;
  }

  async function paste(destDir = currentPane().path, destRoot = currentPane().root) {
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
      await reloadAll();
    } catch (e: any) {
      error = e.message;
    }
  }

  async function makeDir(e: Event) {
    e.preventDefault();
    if (!mkdirName) return;
    const p = currentPane();
    await api('/api/files/mkdir', { method: 'POST', body: JSON.stringify({ root: p.root, path: p.path, name: mkdirName }) });
    mkdirName = '';
    showMkdir = false;
    await reloadAll();
  }

  async function removeMany(ents: Entry[]) {
    if (!ents.length) return;
    const label = ents.length === 1 ? ents[0].name : `${ents.length} items`;
    if (!confirm(`Delete ${label}?`)) return;
    try {
      const p = currentPane();
      for (const ent of ents) {
        await api('/api/files/delete', { method: 'POST', body: JSON.stringify({ root: p.root, path: ent.path }) });
      }
      menu = null;
      p.selected = new Set();
      await reloadAll();
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
      await api('/api/files/rename', { method: 'POST', body: JSON.stringify({ root: currentPane().root, from: ent.path, to }) });
      menu = null;
      await reloadAll();
    } catch (e: any) {
      error = e.message;
    }
  }

  function isStarred(r: string, p: string) {
    return favorites.some((f) => f.root === r && f.path === p);
  }

  async function toggleStar(ent?: { root?: string; path: string; name: string }) {
    const pane = currentPane();
    const r = ent?.root ?? pane.root;
    const pth = ent?.path ?? pane.path;
    const label = ent?.name || pth.split('/').pop() || roots.find((x) => x.id === r)?.label || r;
    const next = isStarred(r, pth)
      ? favorites.filter((f) => !(f.root === r && f.path === pth))
      : [...favorites, { root: r, path: pth, label }];
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

  async function uploadFiles(files: FileList | File[], destRoot = currentPane().root, destPath = currentPane().path) {
    const fd = new FormData();
    for (const f of files) fd.append('file', f);
    const q = new URLSearchParams({ root: destRoot, path: destPath });
    await api('/api/files/upload?' + q.toString(), { method: 'POST', body: fd });
    await reloadAll();
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
    const p = currentPane();
    if (!p.selected.has(ent.path)) p.selected = new Set([ent.path]);
    const payload = JSON.stringify({ root: p.root, paths: [...p.selected] });
    ev.dataTransfer?.setData(DRAG_MIME, payload);
    ev.dataTransfer?.setData('text/plain', payload);
    if (ev.dataTransfer) ev.dataTransfer.effectAllowed = 'copyMove';
  }

  function onDragOverList(ev: DragEvent, paneId: string) {
    ev.preventDefault();
    if (ev.dataTransfer) {
      ev.dataTransfer.dropEffect = hasInternalDrag(ev) ? 'move' : 'copy';
    }
    dragPane = paneId;
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

  async function movePaths(srcRoot: string, paths: string[], destRoot: string, destDir: string) {
    if (destRoot !== srcRoot) {
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
    currentPane().selected = new Set();
    await reloadAll();
  }

  async function onDropAt(ev: DragEvent, destRoot: string, destDir: string) {
    ev.preventDefault();
    ev.stopPropagation();
    dragging = false;
    dropOn = null;
    dragPane = null;
    const internal = dragPayload(ev);
    try {
      if (internal) {
        await movePaths(internal.root, internal.paths, destRoot, destDir);
        return;
      }
      if (ev.dataTransfer?.files?.length) {
        await uploadFiles(ev.dataTransfer.files, destRoot, destDir);
      }
    } catch (e: any) {
      error = e.message;
    }
  }

  function openContext(ev: MouseEvent, ent: Entry | null) {
    ev.preventDefault();
    ev.stopPropagation();
    const p = currentPane();
    if (ent && !p.selected.has(ent.path)) p.selected = new Set([ent.path]);
    menu = { x: ev.clientX, y: ev.clientY, ent };
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
    {#if selectedEntries.length}
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
          value={pane.query}
          oninput={(e) => (currentPane().query = e.currentTarget.value)}
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
            class:active={f.root === pane.root && f.path === pane.path}
            class:drop-ok={dropOn === dropKey('fav', f.root + '/' + f.path)}
            onclick={(e) => locClick(e, f.root, f.path)}
            onauxclick={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                openTab(f.root, f.path);
              }
            }}
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
      {#each (roots.length ? roots : pane.list?.roots ?? []) as r}
        <button
          class="loc"
          class:active={r.id === pane.root && !pane.path}
          class:drop-ok={dropOn === dropKey('loc', r.id)}
          onclick={(e) => locClick(e, r.id, '')}
          onauxclick={(e) => {
            if (e.button === 1) {
              e.preventDefault();
              openTab(r.id, '');
            }
          }}
          ondragover={(e) => onDragOverTarget(e, dropKey('loc', r.id))}
          ondragleave={() => (dropOn = null)}
          ondrop={(e) => onDropAt(e, r.id, '')}
        >
          <Icon name={locationIcon(r.id, r.label)} size={22} alt="" />
          {r.label}
        </button>
      {/each}
    </aside>
    <div class="files-workspace">
      <div class="files-tabs" role="tablist" aria-label="Open folders">
        {#each tabs as t, i (t.id)}
          <div
            class="files-tab"
            class:active={i === tabI}
            role="tab"
            aria-selected={i === tabI}
            onauxclick={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                closeTab(i);
              }
            }}
          >
            <button type="button" class="files-tab-name" onclick={() => (tabI = i)}>
              <span class="clip">{tabTitle(t)}</span>
            </button>
            {#if tabs.length > 1}
              <button type="button" class="files-tab-x" aria-label="Close tab" onclick={() => closeTab(i)}>
                <UiIcon name="close" size={14} />
              </button>
            {/if}
          </div>
        {/each}
        <button class="files-tab-add" onclick={() => newTab()} aria-label="New tab" title="New tab">
          <UiIcon name="add" size={18} />
        </button>
        {#if canSplit}
          <button
            class="files-tab-add"
            disabled={tab.panes.length >= maxPanes}
            onclick={() => splitPane()}
            aria-label="Split view"
            title="Split view"
          >
            <UiIcon name="view_column" size={18} />
          </button>
        {/if}
      </div>
      {#if showMkdir}
        <form class="files-toolbar" onsubmit={makeDir}>
          <input placeholder="Folder name" bind:value={mkdirName} />
          <button class="btn">Create</button>
        </form>
      {/if}
      {#if error}<div class="err" style="padding:0 16px">{error}</div>{/if}
      <div class="files-panes" class:split={tab.panes.length > 1} style="--panes:{tab.panes.length}">
        {#each tab.panes as p, pi (p.id)}
          <FolderView
            pane={p}
            {view}
            active={pi === tab.activePane}
            split={tab.panes.length > 1}
            canClose={tab.panes.length > 1}
            dropOn={dropOn}
            dragging={dragging && dragPane === p.id}
            hint={pi === tab.activePane ? clipLabel() : ''}
            {dropKey}
            onActivate={() => activatePane(tab, pi)}
            onNavigate={(root, path) => loadPane(p, root, path)}
            onSelect={selectClick}
            onOpen={open}
            onContext={openContext}
            onDragStart={onDragStart}
            onDragOverList={(e) => {
              activatePane(tab, pi);
              onDragOverList(e, p.id);
            }}
            onDragOverTarget={onDragOverTarget}
            onDragLeave={() => (dropOn = null)}
            onDropAt={onDropAt}
            onDropList={(e) => onDropAt(e, p.root, p.path)}
            onClear={() => {
              p.selected = new Set();
              menu = null;
            }}
            onClosePane={() => closePane(tab, pi)}
          />
        {/each}
      </div>
    </div>
  </div>
  {/key}
</AppWindow>

{#if menu}
  <div class="ctx" style="left:{menu.x}px; top:{menu.y}px" role="menu">
    {#if menu.ent}
      <button onclick={() => open(menu!.ent!)}>Open</button>
    {/if}
    {#if menu.ent?.dir}
      <button onclick={() => { const ent = menu!.ent!; menu = null; openTab(currentPane().root, ent.path); }}>
        Open in new tab
      </button>
      {#if canSplit}
        <button onclick={() => { const ent = menu!.ent!; menu = null; openBeside(currentPane().root, ent.path); }}>
          Open beside
        </button>
      {/if}
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
      <button onclick={() => paste(menu?.ent?.dir ? menu.ent.path : currentPane().path)}>Paste</button>
    {/if}
    {#if menu.ent}
      <button onclick={() => toggleStar({ path: menu!.ent!.path, name: menu!.ent!.name })}>
        {isStarred(currentPane().root, menu.ent.path) ? 'Unstar' : 'Star'}
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
    root={currentPane().root}
    entry={preview}
    siblings={currentPane().list?.entries ?? []}
    onClose={() => (preview = null)}
    onChange={(ent) => (preview = ent)}
  />
{/if}
