<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';

  type Unit = {
    id: string;
    unit: string;
    label: string;
    active: string;
    enabled: string;
    description: string;
  };

  let units = $state<Unit[]>([]);
  let journal = $state('');
  let selected = $state('');
  let error = $state('');
  let busy = $state('');

  async function load() {
    try {
      units = await api<Unit[]>('/api/units');
      error = '';
    } catch (e: any) {
      error = e.message;
    }
  }

  onMount(load);

  async function act(id: string, action: string) {
    if (!confirm(`${action} ${id}?`)) return;
    busy = id + action;
    try {
      await api(`/api/units/${id}/${action}`, { method: 'POST' });
      await load();
    } catch (e: any) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  async function showJournal(id: string) {
    selected = id;
    const res = await api<{ journal: string }>(`/api/units/${id}/journal`);
    journal = res.journal;
  }
</script>

<div class="top">
  <div>
    <h2>Services</h2>
    <div class="sub">Allowlisted systemd units only</div>
  </div>
  <button class="btn secondary" onclick={load}>Refresh</button>
</div>
{#if error}<div class="err">{error}</div>{/if}

<table class="table">
  <thead><tr><th>Service</th><th>State</th><th>Enabled</th><th></th></tr></thead>
  <tbody>
    {#each units as u}
      <tr>
        <td>
          <strong>{u.label}</strong>
          <div class="sub">{u.unit}</div>
        </td>
        <td><span class="dot" class:on={u.active === 'active'} class:off={u.active === 'failed'}></span> {u.active}</td>
        <td>{u.enabled}</td>
        <td class="row">
          <button class="btn" disabled={!!busy} onclick={() => act(u.id, 'start')}>Start</button>
          <button class="btn secondary" disabled={!!busy} onclick={() => act(u.id, 'stop')}>Stop</button>
          <button class="btn secondary" disabled={!!busy} onclick={() => act(u.id, 'restart')}>Restart</button>
          <button class="btn secondary" disabled={!!busy} onclick={() => act(u.id, 'enable')}>Enable</button>
          <button class="btn secondary" disabled={!!busy} onclick={() => act(u.id, 'disable')}>Disable</button>
          <button class="btn secondary" onclick={() => showJournal(u.id)}>Logs</button>
        </td>
      </tr>
    {/each}
  </tbody>
</table>

{#if selected}
  <div class="top" style="margin-top:24px"><h2>Journal · {selected}</h2></div>
  <pre class="logs">{journal || '(empty)'}</pre>
{/if}
