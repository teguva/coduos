<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { bytes, shortDate, when } from '../lib/format';
  import { fileIcon, locationIcon, iconRev } from '../lib/icons';
  import Icon from '../components/Icon.svelte';

  let { onClose } = $props<{ onClose: () => void }>();

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

  let list = $state<List | null>(null);
  let error = $state('');
  let root = $state('');
  let path = $state('');
  let mkdirName = $state('');
  let showMkdir = $state(false);
  let dragging = $state(false);
  let menu = $state<{ x: number; y: number; ent: Entry } | null>(null);
  let view = $state<'list' | 'grid'>('grid');
  let query = $state('');
  let searchEl = $state<HTMLInputElement | null>(null);
  let favorites = $state<Fav[]>([]);

  let filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const ents = list?.entries ?? [];
    if (!q) return ents;
    return ents.filter((e) => e.name.toLowerCase().includes(q));
  });

  async function load(nextRoot = root, nextPath = path) {
    try {
      const q = new URLSearchParams();
      if (nextRoot) q.set('root', nextRoot);
      if (nextPath) q.set('path', nextPath);
      const data = await api<List>('/api/files?' + q.toString());
      list = data;
      root = data.root || data.roots[0]?.id || '';
      path = data.path;
      favorites = data.favorites ?? favorites;
      if (!data.root && data.roots[0]) {
        await load(data.roots[0].id, '');
      }
      error = '';
    } catch (e: any) {
      error = e.message;
    }
  }

  onMount(() => {
    const q = new URLSearchParams(location.search);
    const r = q.get('root') || '';
    load(r, '');
    const hide = () => (menu = null);
    const onKey = (ev: KeyboardEvent) => {
      if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === 'k') {
        ev.preventDefault();
        searchEl?.focus();
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

  async function enter(ent: Entry) {
    menu = null;
    query = '';
    if (ent.dir) await load(root, ent.path);
    else download(ent);
  }

  function download(ent: Entry) {
    window.open(
      `/api/files/download?root=${encodeURIComponent(root)}&path=${encodeURIComponent(ent.path)}`
    );
  }

  async function makeDir(e: Event) {
    e.preventDefault();
    if (!mkdirName) return;
    await api('/api/files/mkdir', { method: 'POST', body: JSON.stringify({ root, path, name: mkdirName }) });
    mkdirName = '';
    showMkdir = false;
    await load();
  }

  async function remove(ent: Entry) {
    if (!confirm(`Delete ${ent.name}?`)) return;
    await api('/api/files/delete', { method: 'POST', body: JSON.stringify({ root, path: ent.path }) });
    menu = null;
    await load();
  }

  async function rename(ent: Entry) {
    const next = prompt('New name', ent.name);
    if (!next || next === ent.name) return;
    const parent = ent.path.split('/').slice(0, -1).join('/');
    const to = parent ? parent + '/' + next : next;
    await api('/api/files/rename', { method: 'POST', body: JSON.stringify({ root, from: ent.path, to }) });
    menu = null;
    await load();
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

  async function uploadFiles(files: FileList | File[]) {
    const fd = new FormData();
    for (const f of files) fd.append('file', f);
    const q = new URLSearchParams({ root, path });
    await api('/api/files/upload?' + q.toString(), { method: 'POST', body: fd });
    await load();
  }

  async function onFileInput(ev: Event) {
    const input = ev.target as HTMLInputElement;
    if (!input.files?.length) return;
    await uploadFiles(input.files);
    input.value = '';
  }

  function onDrop(ev: DragEvent) {
    ev.preventDefault();
    dragging = false;
    if (ev.dataTransfer?.files?.length) uploadFiles(ev.dataTransfer.files);
  }

  function parentPath() {
    const parts = path.split('/').filter(Boolean);
    parts.pop();
    return parts.join('/');
  }
</script>

<div
  class="os-window sheet-files"
  role="presentation"
  onclick={(e) => { if (e.currentTarget === e.target) onClose(); }}
>
  {#key $iconRev}
  <div class="files-card">
    <aside class="files-side">
      <label class="files-search">
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
          <button class="loc" class:active={f.root === root && f.path === path} onclick={() => load(f.root, f.path)}>
            <Icon name="folder" size={20} alt="" />
            {f.label}
          </button>
        {/each}
      {/if}
      <h3>Storage</h3>
      {#each list?.roots ?? [] as r}
        <button class="loc" class:active={r.id === root && !path} onclick={() => load(r.id, '')}>
          <Icon name={locationIcon(r.id, r.label)} size={22} alt="" />
          {r.label}
        </button>
      {/each}
    </aside>
    <div class="files-main">
      <header class="files-toolbar">
        <button
          class="btn secondary icon-only"
          disabled={!path}
          onclick={() => load(root, parentPath())}
          aria-label="Up"
        >
          <Icon name="go-up" size={18} alt="" />
        </button>
        <div class="addr">
          {#each crumbs() as c, i}
            {#if i > 0}<span>/</span>{/if}
            <button onclick={() => load(root, c.path)}>{c.label}</button>
          {/each}
          <span class="chip muted">{filtered.length} items</span>
        </div>
        <div class="files-tools">
          <button class="btn secondary" onclick={() => (showMkdir = !showMkdir)}>
            <Icon name="folder-new" size={18} alt="" /> New folder
          </button>
          <label class="btn">
            <Icon name="upload" size={18} alt="" /> Import
            <input type="file" multiple hidden onchange={onFileInput} />
          </label>
          <button class="btn secondary icon-only" onclick={() => (view = view === 'list' ? 'grid' : 'list')} aria-label={view === 'list' ? 'Grid view' : 'List view'}>
            <Icon name={view === 'list' ? 'view-grid' : 'view-list'} size={18} alt="" />
          </button>
          <button class="close-x" onclick={onClose} aria-label="Close">
            <Icon name="close" size={18} alt="" />
          </button>
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
        ondragover={(e) => { e.preventDefault(); dragging = true; }}
        ondragleave={() => (dragging = false)}
        ondrop={onDrop}
      >
        {#if filtered.length === 0}
          <div class="files-empty">
            <Icon name="folder-open" size={64} alt="" />
            <div>{query ? 'No matching items.' : 'This folder is empty. Drop files here or create a folder.'}</div>
          </div>
        {:else if view === 'grid'}
          <div class="files-grid">
            {#each filtered as ent}
              <button class="files-tile" onclick={() => enter(ent)} oncontextmenu={(e) => { e.preventDefault(); menu = { x: e.clientX, y: e.clientY, ent }; }}>
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
                  onclick={() => enter(ent)}
                  oncontextmenu={(e) => { e.preventDefault(); menu = { x: e.clientX, y: e.clientY, ent }; }}
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
        {#if list?.space}
          <span class="meta">{bytes(list.space.total - list.space.used)} available / {bytes(list.space.total)}</span>
        {/if}
      </footer>
    </div>
  </div>
  {/key}
</div>

{#if menu}
  <div class="ctx" style="left:{menu.x}px; top:{menu.y}px" role="menu">
    {#if !menu.ent.dir}
      <button onclick={() => download(menu!.ent)}>
        <Icon name="download" size={16} alt="" /> Download
      </button>
    {/if}
    <button onclick={() => toggleStar({ path: menu!.ent.path, name: menu!.ent.name })}>
      {isStarred(root, menu.ent.path) ? 'Unstar' : 'Star'}
    </button>
    <button onclick={() => rename(menu!.ent)}>Rename</button>
    <button onclick={() => remove(menu!.ent)}>
      <Icon name="delete" size={16} alt="" /> Delete
    </button>
  </div>
{/if}
