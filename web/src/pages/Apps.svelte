<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';

  let { go } = $props<{ go: (to: string) => void }>();
  type App = {
    id: string;
    name: string;
    web_port?: number | null;
    status: { running: boolean; error?: string | null };
  };
  let apps = $state<App[]>([]);
  let error = $state('');

  async function load() {
    try {
      apps = await api<App[]>('/api/apps');
    } catch (e: any) {
      error = e.message;
    }
  }
  onMount(load);
</script>

<div class="top">
  <div>
    <h2>Apps</h2>
    <div class="sub">Docker Compose applications</div>
  </div>
  <button class="btn" onclick={() => go('/apps/new')}>Install app</button>
</div>
{#if error}<div class="err">{error}</div>{/if}
<div class="apps">
  {#each apps as app}
    <button class="app" onclick={() => go('/apps/' + app.id)}>
      <div class="icon">{app.name.slice(0, 1).toUpperCase()}</div>
      <div class="name">{app.name}</div>
      <div class="meta"><span class="dot" class:on={app.status.running}></span> {app.status.running ? 'Running' : 'Stopped'}</div>
    </button>
  {/each}
</div>
{#if apps.length === 0}
  <p class="sub">No apps yet. Install from Compose YAML — try the whoami example in packaging/examples.</p>
{/if}
