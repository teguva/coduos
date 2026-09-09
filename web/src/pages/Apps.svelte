<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { appIcon, iconRev } from '../lib/icons';
  import AppWindow from '../components/AppWindow.svelte';
  import Icon from '../components/Icon.svelte';
  import UiIcon from '../components/UiIcon.svelte';

  let { go, onClose } = $props<{ go: (to: string) => void; onClose: () => void }>();
  type App = {
    id: string;
    name: string;
    icon_url?: string | null;
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

<AppWindow title="Apps" icon="apps" size="xl" {onClose}>
  {#snippet actions()}
    <button class="btn icon-only compact" onclick={() => go('/apps/new')} aria-label="Install app">
      <UiIcon name="add" size={20} />
    </button>
  {/snippet}

{#if error}<div class="err">{error}</div>{/if}
{#key $iconRev}
<div class="apps">
  {#each apps as app}
    <button class="app" onclick={() => go('/apps/' + app.id)}>
      <Icon name={appIcon(app)} size={48} class="tile-img" alt="" />
      <div class="name">{app.name}</div>
      <div class="meta"><span class="dot" class:on={app.status.running}></span> {app.status.running ? 'Running' : 'Stopped'}</div>
    </button>
  {/each}
</div>
{/key}
{#if apps.length === 0}
  <p class="hint">No apps yet. Install from Compose YAML — try the whoami example in packaging/examples.</p>
{/if}
</AppWindow>
