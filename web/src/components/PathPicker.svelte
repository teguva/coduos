<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../lib/api';
  import { fileIcon, locationIcon } from '../lib/icons';
  import { joinHostPath, parentRel, splitHostPath, type FileFav, type FileRoot } from '../lib/filePath';
  import type { FileEntry } from '../lib/filePane';
  import Icon from './Icon.svelte';
  import UiIcon from './UiIcon.svelte';

  type List = {
    root: string;
    path: string;
    roots: FileRoot[];
    entries: FileEntry[];
    favorites?: FileFav[];
    space?: { used: number; total: number } | null;
  };

  let {
    start = '',
    title = 'Choose path',
    onPick,
    onClose
  } = $props<{
    start?: string;
    title?: string;
    onPick: (path: string) => void;
    onClose: () => void;
  }>();

  let roots = $state<FileRoot[]>([]);
  let favorites = $state<FileFav[]>([]);
  let root = $state('');
  let path = $state('');
  let entries = $state<FileEntry[]>([]);
  let query = $state('');
  let hits = $state<FileEntry[] | null>(null);
  let searching = $state(false);
  let truncated = $state(false);
  let selected = $state<FileEntry | null>(null);
  let error = $state('');
  let mkdirName = $state('');
  let showMkdir = $state(false);
  let loading = $state(true);
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let searchSeq = 0;

  const rootMeta = $derived(roots.find((r) => r.id === root));
  const filtered = $derived.by(() => {
    if (hits) return hits;
    const q = query.trim().toLowerCase();
    if (!q) return entries;
    return entries.filter((e) => e.name.toLowerCase().includes(q));
  });
  const deepSearch = $derived(hits != null);
  const crumbs = $derived.by(() => {
    const parts = path.split('/').filter(Boolean);
    const out: { label: string; rel: string }[] = [{ label: rootMeta?.label || 'Files', rel: '' }];
    let acc = '';
    for (const p of parts) {
      acc = acc ? `${acc}/${p}` : p;
      out.push({ label: p, rel: acc });
    }
    return out;
  });
  const chosen = $derived.by(() => {
    if (!rootMeta) return '';
    if (selected) return joinHostPath(rootMeta.path, selected.path);
    return joinHostPath(rootMeta.path, path);
  });

  async function load(nextRoot: string, nextPath: string): Promise<boolean> {
    error = '';
    loading = true;
    try {
      const q = new URLSearchParams();
      if (nextRoot) q.set('root', nextRoot);
      if (nextPath) q.set('path', nextPath);
      const data = await api<List>('/api/files?' + q.toString());
      roots = data.roots ?? roots;
      favorites = data.favorites ?? favorites;
      if (!nextRoot && data.roots[0]) {
        return await load(data.roots[0].id, '');
      }
      root = data.root || nextRoot;
      path = data.path;
      entries = data.entries ?? [];
      selected = null;
      query = '';
      hits = null;
      searching = false;
      truncated = false;
      return true;
    } catch (e: any) {
      error = e.message;
      return false;
    } finally {
      loading = false;
    }
  }

  async function openStart() {
    await load('', '');
    const hit = splitHostPath(start, roots);
    if (!hit) return;
    const ok = await load(hit.root.id, hit.rel);
    if (ok) return;
    await load(hit.root.id, parentRel(hit.rel));
    const name = hit.rel.split('/').filter(Boolean).pop();
    selected = entries.find((e) => e.name === name) ?? null;
    error = '';
  }

  onMount(() => {
    openStart();
  });

  onDestroy(() => {
    if (searchTimer) clearTimeout(searchTimer);
  });

  function onQueryInput(value: string) {
    query = value;
    if (searchTimer) clearTimeout(searchTimer);
    const q = value.trim();
    if (q.length < 2) {
      hits = null;
      searching = false;
      truncated = false;
      return;
    }
    searching = true;
    searchTimer = setTimeout(() => {
      runSearch();
    }, 280);
  }

  async function runSearch() {
    const q = query.trim();
    if (q.length < 2 || !root) {
      hits = null;
      searching = false;
      truncated = false;
      return;
    }
    const seq = ++searchSeq;
    searching = true;
    try {
      const params = new URLSearchParams({ root, path, q });
      const data = await api<{ entries: FileEntry[]; truncated: boolean }>('/api/files/search?' + params.toString());
      if (seq !== searchSeq) return;
      hits = data.entries;
      truncated = data.truncated;
      error = '';
    } catch (e: any) {
      if (seq !== searchSeq) return;
      error = e.message;
      hits = [];
      truncated = false;
    } finally {
      if (seq === searchSeq) searching = false;
    }
  }

  function openEnt(ent: FileEntry) {
    if (ent.dir) {
      load(root, ent.path);
      return;
    }
    onPick(joinHostPath(rootMeta?.path || '', ent.path));
  }

  function choose() {
    if (!chosen) return;
    onPick(chosen);
  }

  async function makeDir(e: Event) {
    e.preventDefault();
    const name = mkdirName.trim();
    if (!name || !root) return;
    error = '';
    try {
      await api('/api/files/mkdir', {
        method: 'POST',
        body: JSON.stringify({ root, path, name })
      });
      mkdirName = '';
      showMkdir = false;
      await load(root, path);
    } catch (err: any) {
      error = err.message;
    }
  }
</script>

<div class="picker-bg" role="presentation" onclick={(e) => { if (e.currentTarget === e.target) onClose(); }}>
  <div class="picker-card" role="dialog" aria-label={title}>
    <header class="os-head">
      <UiIcon name="folder_open" size={22} />
      <h1>{title}</h1>
      <div class="os-actions">
        <button type="button" class="btn secondary compact" onclick={() => (showMkdir = !showMkdir)}>
          <UiIcon name="create_new_folder" size={18} /> New folder
        </button>
      </div>
      <button type="button" class="close-x" onclick={onClose} aria-label="Close">
        <UiIcon name="close" size={20} />
      </button>
    </header>
    {#if showMkdir}
      <form class="files-toolbar" onsubmit={makeDir}>
        <input placeholder="Folder name" bind:value={mkdirName} />
        <button class="btn" type="submit">Create</button>
      </form>
    {/if}
    {#if error}<div class="err" style="padding:8px 16px">{error}</div>{/if}
    <div class="picker-body">
      <aside class="files-side">
        <label class="files-search">
          <UiIcon name="search" size={18} />
          <input
            value={query}
            oninput={(e) => onQueryInput(e.currentTarget.value)}
            placeholder="Search this folder and subfolders"
            aria-label="Search this folder and subfolders"
          />
        </label>
        {#if favorites.length}
          <h3>Starred</h3>
          {#each favorites as f}
            <button
              type="button"
              class="loc"
              class:active={f.root === root && f.path === path}
              onclick={() => load(f.root, f.path)}
            >
              <Icon name="folder" size={20} alt="" />
              {f.label}
            </button>
          {/each}
        {/if}
        <h3>Storage</h3>
        {#each roots as r}
          <button
            type="button"
            class="loc"
            class:active={r.id === root && !path}
            onclick={() => load(r.id, '')}
          >
            <Icon name={locationIcon(r.id, r.label)} size={22} alt="" />
            {r.label}
          </button>
        {/each}
        {#if !loading && !roots.length}
          <p class="hint" style="margin:8px 10px">Mount a disk in Storage to browse it here.</p>
        {/if}
      </aside>
      <div class="files-main">
        <header class="files-toolbar">
          <button
            type="button"
            class="btn secondary icon-only compact"
            disabled={!path}
            onclick={() => load(root, parentRel(path))}
            aria-label="Up"
          >
            <UiIcon name="arrow_upward" size={18} />
          </button>
          <div class="addr">
            {#each crumbs as c, i}
              {#if i > 0}<span>/</span>{/if}
              <button type="button" onclick={() => load(root, c.rel)}>{c.label}</button>
            {/each}
            {#if searching || deepSearch}
              <span class="chip muted">
                {#if searching}
                  Searching…
                {:else}
                  {filtered.length} match{filtered.length === 1 ? '' : 'es'}{#if truncated}+{/if}
                {/if}
              </span>
            {/if}
          </div>
        </header>
        <div class="files-list" role="presentation" onclick={() => (selected = null)}>
          {#if loading && !entries.length}
            <div class="files-empty">Loading…</div>
          {:else if searching && !hits}
            <div class="files-empty">Searching this folder and subfolders…</div>
          {:else if filtered.length === 0}
            <div class="files-empty">
              <Icon name="folder-open" size={64} alt="" />
              <div>
                {#if query.trim() && truncated}
                  Search stopped after scanning too many items. Open a more specific folder and try again.
                {:else if query.trim()}
                  No matching files or folders.
                {:else}
                  This folder is empty.
                {/if}
              </div>
            </div>
          {:else}
            <div class="files-grid">
              {#each filtered as ent}
                <button
                  type="button"
                  class="files-tile"
                  class:selected={selected?.path === ent.path}
                  onclick={(e) => {
                    e.stopPropagation();
                    selected = ent;
                  }}
                  ondblclick={(e) => {
                    e.stopPropagation();
                    openEnt(ent);
                  }}
                >
                  <Icon name={fileIcon(ent.name, ent.dir)} size={52} class="tile-img" alt="" />
                  <div class="label">{ent.name}</div>
                  {#if deepSearch}
                    <div class="meta">{parentRel(ent.path) || 'This folder'}</div>
                  {/if}
                </button>
              {/each}
            </div>
          {/if}
        </div>
        <footer class="files-foot">
          <span class="clip">{chosen || 'Choose a location in Files'}</span>
          {#if truncated}
            <span class="meta">Showing the first {filtered.length} matches</span>
          {/if}
        </footer>
      </div>
    </div>
    <div class="picker-actions">
      <button type="button" class="btn secondary" onclick={onClose}>Cancel</button>
      <button type="button" class="btn" disabled={!chosen} onclick={choose}>Use this path</button>
    </div>
  </div>
</div>
