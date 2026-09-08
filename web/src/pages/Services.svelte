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
  let more = $state<string | null>(null);

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
    more = null;
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
    more = null;
    const res = await api<{ journal: string }>(`/api/units/${id}/journal`);
    journal = res.journal;
  }
</script>

<div class="top">
  <p class="hint" style="margin:0">Allowlisted systemd units.</p>
  <button class="btn secondary" onclick={load}>Refresh</button>
</div>
{#if error}<div class="err">{error}</div>{/if}

<table class="table">
  <thead><tr><th>Service</th><th>State</th><th></th></tr></thead>
  <tbody>
    {#each units as u}
      <tr>
        <td>
          <strong>{u.label}</strong>
          <div class="sub">{u.unit} · {u.enabled}</div>
        </td>
        <td><span class="dot" class:on={u.active === 'active'} class:off={u.active === 'failed'}></span> {u.active}</td>
        <td class="row" style="justify-content:flex-end; position:relative">
          {#if u.active === 'active'}
            <button class="btn" disabled={!!busy} onclick={() => act(u.id, 'stop')}>Stop</button>
          {:else}
            <button class="btn" disabled={!!busy} onclick={() => act(u.id, 'start')}>Start</button>
          {/if}
          <button class="btn secondary icon-only" disabled={!!busy} onclick={() => (more = more === u.id ? null : u.id)} aria-label="More">···</button>
          {#if more === u.id}
            <div class="ctx" style="position:absolute; right:0; top:100%; z-index:5">
              <button onclick={() => act(u.id, 'restart')}>Restart</button>
              <button onclick={() => act(u.id, u.enabled === 'enabled' ? 'disable' : 'enable')}>
                {u.enabled === 'enabled' ? 'Disable' : 'Enable'}
              </button>
              <button onclick={() => showJournal(u.id)}>Logs</button>
            </div>
          {/if}
        </td>
      </tr>
    {/each}
  </tbody>
</table>

{#if selected}
  <div class="top" style="margin-top:24px"><h3>Journal · {selected}</h3></div>
  <pre class="logs">{journal || '(empty)'}</pre>
{/if}
