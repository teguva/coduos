<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';

  type Root = { id: string; label: string; path: string };
  type Unit = { id: string; unit: string; label: string };
  type Settings = {
    bind: string;
    data_dir: string;
    github_owner: string;
    github_repo: string;
    file_roots: Root[];
    units: Unit[];
    version: string;
  };
  type Update = {
    current: string;
    latest?: string | null;
    html_url?: string | null;
    up_to_date: boolean;
    error?: string | null;
  };

  let settings = $state<Settings | null>(null);
  let update = $state<Update | null>(null);
  let error = $state('');
  let notice = $state('');
  let currentPw = $state('');
  let newPw = $state('');
  let rootsText = $state('');
  let unitsText = $state('');

  function dumpRoots(r: Root[]) {
    return r.map((x) => `${x.id} | ${x.label} | ${x.path}`).join('\n');
  }
  function dumpUnits(u: Unit[]) {
    return u.map((x) => `${x.id} | ${x.label} | ${x.unit}`).join('\n');
  }
  function parseLines(text: string, kind: 'root' | 'unit') {
    return text
      .split('\n')
      .map((l) => l.trim())
      .filter(Boolean)
      .map((l) => {
        const [id, label, rest] = l.split('|').map((s) => s.trim());
        if (!id || !label || !rest) throw new Error(`bad ${kind} line: ${l}`);
        return kind === 'root'
          ? { id, label, path: rest }
          : { id, label, unit: rest };
      });
  }

  async function load() {
    settings = await api<Settings>('/api/settings');
    rootsText = dumpRoots(settings.file_roots);
    unitsText = dumpUnits(settings.units);
    update = await api<Update>('/api/update');
  }

  onMount(() => load().catch((e) => (error = e.message)));

  async function save(e: Event) {
    e.preventDefault();
    error = '';
    notice = '';
    try {
      const file_roots = parseLines(rootsText, 'root') as Root[];
      const units = parseLines(unitsText, 'unit') as Unit[];
      settings = await api<Settings>('/api/settings', {
        method: 'PUT',
        body: JSON.stringify({ file_roots, units })
      });
      notice = 'Saved.';
    } catch (err: any) {
      error = err.message;
    }
  }

  async function changePw(e: Event) {
    e.preventDefault();
    error = '';
    notice = '';
    try {
      await api('/api/settings/password', {
        method: 'PUT',
        body: JSON.stringify({ current: currentPw, new_password: newPw })
      });
      currentPw = '';
      newPw = '';
      notice = 'Password updated.';
    } catch (err: any) {
      error = err.message;
    }
  }
</script>

<div class="top">
  <div>
    <h2>Settings</h2>
    <div class="sub">CoduOS v{settings?.version ?? ''} · bind {settings?.bind ?? ''}</div>
  </div>
</div>

{#if error}<div class="err">{error}</div>{/if}
{#if notice}<p>{notice}</p>{/if}

<div class="card" style="margin-bottom:16px">
  <h3>Updates</h3>
  {#if update}
    <p>Current {update.current}{#if update.latest} · latest {update.latest}{/if}</p>
    {#if update.error}<div class="err">{update.error}</div>{/if}
    {#if update.up_to_date && !update.error}<p>Up to date.</p>{/if}
    {#if update.html_url && !update.up_to_date}
      <a class="btn" href={update.html_url} target="_blank" rel="noreferrer">Open GitHub release</a>
    {/if}
  {/if}
  <p class="sub">Checks github.com/{settings?.github_owner}/{settings?.github_repo} only.</p>
</div>

<form class="card" style="margin-bottom:16px" onsubmit={save}>
  <h3>File roots</h3>
  <p class="sub">One per line: <code>id | label | /absolute/path</code></p>
  <label class="field"><textarea bind:value={rootsText}></textarea></label>
  <h3>Host units</h3>
  <p class="sub">One per line: <code>id | label | unit.service</code>. The UI cannot start units that are not listed here.</p>
  <label class="field"><textarea bind:value={unitsText}></textarea></label>
  <button class="btn">Save</button>
</form>

<form class="card" onsubmit={changePw}>
  <h3>Admin password</h3>
  <label class="field"><span>Current</span><input type="password" bind:value={currentPw} required /></label>
  <label class="field"><span>New</span><input type="password" bind:value={newPw} required /></label>
  <button class="btn">Change password</button>
</form>
