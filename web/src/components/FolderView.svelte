<script lang="ts">
  import { bytes, shortDate, when } from '../lib/format';
  import { fileIcon } from '../lib/icons';
  import type { FileEntry, FilePane } from '../lib/filePane';
  import Icon from './Icon.svelte';
  import UiIcon from './UiIcon.svelte';

  let {
    pane,
    view,
    active,
    split,
    canClose,
    dropOn,
    dragging,
    dropKey,
    onActivate,
    onNavigate,
    onSelect,
    onOpen,
    onContext,
    onDragStart,
    onDragOverList,
    onDragOverTarget,
    onDropAt,
    onDropList,
    onClear,
    onClosePane,
    onDragLeave,
    hint
  } = $props<{
    pane: FilePane;
    view: 'list' | 'grid';
    active: boolean;
    split: boolean;
    canClose: boolean;
    dropOn: string | null;
    dragging: boolean;
    hint?: string;
    dropKey: (kind: 'crumb' | 'loc' | 'fav' | 'ent', id: string) => string;
    onActivate: () => void;
    onNavigate: (root: string, path: string) => void;
    onSelect: (ev: MouseEvent, ent: FileEntry) => void;
    onOpen: (ent: FileEntry, ev: MouseEvent) => void;
    onContext: (ev: MouseEvent, ent: FileEntry | null) => void;
    onDragStart: (ev: DragEvent, ent: FileEntry) => void;
    onDragOverList: (ev: DragEvent) => void;
    onDragOverTarget: (ev: DragEvent, key: string) => void;
    onDropAt: (ev: DragEvent, destRoot: string, destDir: string) => void;
    onDropList: (ev: DragEvent) => void;
    onClear: () => void;
    onClosePane?: () => void;
    onDragLeave?: () => void;
  }>();

  let filtered = $derived.by(() => {
    const q = pane.query.trim().toLowerCase();
    const ents = pane.list?.entries ?? [];
    if (!q) return ents;
    return ents.filter((e) => e.name.toLowerCase().includes(q));
  });

  function crumbs() {
    const parts = pane.path.split('/').filter(Boolean);
    const out: { label: string; path: string }[] = [
      {
        label: pane.list?.roots.find((r) => r.id === pane.root)?.label || pane.root || 'Files',
        path: ''
      }
    ];
    let acc = '';
    for (const p of parts) {
      acc = acc ? acc + '/' + p : p;
      out.push({ label: p, path: acc });
    }
    return out;
  }

  function pathLabel() {
    const label = pane.list?.roots.find((r) => r.id === pane.root)?.label || pane.root;
    const parts = pane.path.split('/').filter(Boolean);
    return [label, ...parts].filter(Boolean).join(' / ');
  }

  function parentPath() {
    const parts = pane.path.split('/').filter(Boolean);
    parts.pop();
    return parts.join('/');
  }
</script>

<div
  class="files-main"
  class:is-active={active}
  class:is-split={split}
  role="region"
  aria-label={pathLabel() || 'Folder'}
>
  <header class="files-toolbar">
    <button
      class="btn secondary icon-only compact"
      disabled={!pane.path}
      onclick={(e) => {
        e.stopPropagation();
        onNavigate(pane.root, parentPath());
      }}
      aria-label="Up"
    >
      <UiIcon name="arrow_upward" size={18} />
    </button>
    <div class="addr">
      {#each crumbs() as c, i}
        {#if i > 0}<span>/</span>{/if}
        <button
          class:drop-ok={dropOn === dropKey('crumb', pane.id + ':' + c.path)}
          onclick={(e) => {
            e.stopPropagation();
            onNavigate(pane.root, c.path);
          }}
          ondragover={(e) => onDragOverTarget(e, dropKey('crumb', pane.id + ':' + c.path))}
          ondragleave={() => onDragLeave?.()}
          ondrop={(e) => onDropAt(e, pane.root, c.path)}
        >{c.label}</button>
      {/each}
      <span class="chip muted">{filtered.length} {filtered.length === 1 ? 'item' : 'items'}</span>
    </div>
    {#if canClose && onClosePane}
      <button
        class="btn secondary icon-only compact"
        onclick={(e) => {
          e.stopPropagation();
          onClosePane();
        }}
        aria-label="Close pane"
      >
        <UiIcon name="close" size={18} />
      </button>
    {/if}
  </header>
  {#if pane.error}<div class="err" style="padding:0 16px">{pane.error}</div>{/if}
  <div
    class="files-list"
    class:drag={dragging && active}
    role="presentation"
    onclick={(e) => {
      e.stopPropagation();
      onActivate();
      onClear();
    }}
    oncontextmenu={(e) => onContext(e, null)}
    ondragover={onDragOverList}
    ondrop={onDropList}
  >
    {#if filtered.length === 0}
      <div class="files-empty">
        <Icon name="folder-open" size={64} alt="" />
        <div>
          {#if !pane.list && !pane.error}
            Loading…
          {:else if pane.query}
            No matching items.
          {:else}
            This folder is empty. Drop files here or create a folder.
          {/if}
        </div>
      </div>
    {:else if view === 'grid'}
      <div class="files-grid">
        {#each filtered as ent}
          <button
            class="files-tile"
            class:selected={pane.selected.has(ent.path)}
            class:drop-ok={ent.dir && dropOn === dropKey('ent', pane.id + ':' + ent.path)}
            draggable="true"
            onclick={(e) => {
              e.stopPropagation();
              onActivate();
              onSelect(e, ent);
            }}
            ondblclick={(e) => {
              e.stopPropagation();
              onActivate();
              onOpen(ent, e);
            }}
            oncontextmenu={(e) => {
              onActivate();
              onContext(e, ent);
            }}
            ondragstart={(e) => {
              onActivate();
              onDragStart(e, ent);
            }}
            ondragover={(e) => {
              if (ent.dir) onDragOverTarget(e, dropKey('ent', pane.id + ':' + ent.path));
              else {
                e.stopPropagation();
                onDragOverList(e);
              }
            }}
            ondragleave={ent.dir ? () => onDragLeave?.() : undefined}
            ondrop={(e) => onDropAt(e, pane.root, ent.dir ? ent.path : pane.path)}
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
              class:selected={pane.selected.has(ent.path)}
              class:drop-ok={ent.dir && dropOn === dropKey('ent', pane.id + ':' + ent.path)}
              draggable="true"
              onclick={(e) => {
                e.stopPropagation();
                onActivate();
                onSelect(e, ent);
              }}
              ondblclick={(e) => {
                e.stopPropagation();
                onActivate();
                onOpen(ent, e);
              }}
              oncontextmenu={(e) => {
                onActivate();
                onContext(e, ent);
              }}
              ondragstart={(e) => {
              onActivate();
              onDragStart(e, ent);
            }}
              ondragover={(e) => {
                if (ent.dir) onDragOverTarget(e, dropKey('ent', pane.id + ':' + ent.path));
                else {
                  e.stopPropagation();
                  onDragOverList(e);
                }
              }}
              ondragleave={ent.dir ? () => onDragLeave?.() : undefined}
              ondrop={(e) => onDropAt(e, pane.root, ent.dir ? ent.path : pane.path)}
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
    {#if pane.selected.size}
      <span class="meta">{pane.selected.size} selected</span>
    {:else if hint}
      <span class="meta">{hint}</span>
    {/if}
    {#if pane.list?.space}
      <span class="meta">{bytes(pane.list.space.total - pane.list.space.used)} available / {bytes(pane.list.space.total)}</span>
    {/if}
  </footer>
</div>
