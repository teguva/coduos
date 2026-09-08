<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { bytes } from '../lib/format';

  type Root = { id: string; label: string; path: string };
  type Entry = { name: string; path: string; dir: boolean; size: number };
  type List = { root: string; path: string; roots: Root[]; entries: Entry[] };

  let list = $state<List | null>(null);
  let error = $state('');
  let root = $state('');
  let path = $state('');
  let mkdirName = $state('');

  async function load(nextRoot = root, nextPath = path) {
    try {
      const q = new URLSearchParams();
      if (nextRoot) q.set('root', nextRoot);
      if (nextPath) q.set('path', nextPath);
      const data = await api<List>('/api/files?' + q.toString());
      list = data;
      root = data.root || data.roots[0]?.id || '';
      path = data.path;
      if (!data.root && data.roots[0]) {
        await load(data.roots[0].id, '');
      }
      error = '';
    } catch (e: any) {
      error = e.message;
    }
  }

  onMount(() => load());

  function crumbs() {
    const parts = path.split('/').filter(Boolean);
    const out: { label: string; path: string }[] = [{ label: list?.roots.find((r) => r.id === root)?.label || root, path: '' }];
    let acc = '';
    for (const p of parts) {
      acc = acc ? acc + '/' + p : p;
      out.push({ label: p, path: acc });
    }
    return out;
  }

  async function enter(ent: Entry) {
    if (ent.dir) await load(root, ent.path);
    else window.open(`/api/files/download?root=${encodeURIComponent(root)}&path=${encodeURIComponent(ent.path)}`);
  }

  async function makeDir(e: Event) {
    e.preventDefault();
    if (!mkdirName) return;
    await api('/api/files/mkdir', { method: 'POST', body: JSON.stringify({ root, path, name: mkdirName }) });
    mkdirName = '';
    await load();
  }

  async function remove(ent: Entry) {
    if (!confirm(`Delete ${ent.name}?`)) return;
    await api('/api/files/delete', { method: 'POST', body: JSON.stringify({ root, path: ent.path }) });
    await load();
  }

  async function rename(ent: Entry) {
    const next = prompt('New name', ent.name);
    if (!next || next === ent.name) return;
    const parent = ent.path.split('/').slice(0, -1).join('/');
    const to = parent ? parent + '/' + next : next;
    await api('/api/files/rename', { method: 'POST', body: JSON.stringify({ root, from: ent.path, to }) });
    await load();
  }

  async function upload(ev: Event) {
    const input = ev.target as HTMLInputElement;
    if (!input.files?.length) return;
    const fd = new FormData();
    for (const f of input.files) fd.append('file', f);
    const q = new URLSearchParams({ root, path });
    await api('/api/files/upload?' + q.toString(), { method: 'POST', body: fd });
    input.value = '';
    await load();
  }
</script>

<div class="top">
  <div>
    <h2>Files</h2>
    <div class="sub">Jailed to configured roots</div>
  </div>
  <div class="row">
    <select bind:value={root} onchange={() => load(root, '')}>
      {#each list?.roots ?? [] as r}
        <option value={r.id}>{r.label}</option>
      {/each}
    </select>
    <form class="row" onsubmit={makeDir}>
      <input placeholder="New folder" bind:value={mkdirName} />
      <button class="btn secondary">Create</button>
    </form>
    <label class="btn secondary">Upload<input type="file" multiple hidden onchange={upload} /></label>
  </div>
</div>

{#if error}<div class="err">{error}</div>{/if}

<div class="crumbs">
  {#each crumbs() as c, i}
    {#if i > 0}<span>/</span>{/if}
    <button onclick={() => load(root, c.path)}>{c.label}</button>
  {/each}
</div>

<table class="table">
  <thead><tr><th>Name</th><th>Size</th><th></th></tr></thead>
  <tbody>
    {#each list?.entries ?? [] as ent}
      <tr>
        <td onclick={() => enter(ent)}>{ent.dir ? '▸' : ''} {ent.name}</td>
        <td>{ent.dir ? '—' : bytes(ent.size)}</td>
        <td>
          <button class="btn secondary" onclick={() => rename(ent)}>Rename</button>
          <button class="btn danger" onclick={() => remove(ent)}>Delete</button>
        </td>
      </tr>
    {/each}
  </tbody>
</table>
