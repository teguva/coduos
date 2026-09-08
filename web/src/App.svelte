<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type ApiError } from './lib/api';
  import Shell from './components/Shell.svelte';
  import Setup from './pages/Setup.svelte';
  import Login from './pages/Login.svelte';
  import Dashboard from './pages/Dashboard.svelte';
  import Apps from './pages/Apps.svelte';
  import AppEditor from './pages/AppEditor.svelte';
  import Files from './pages/Files.svelte';
  import Services from './pages/Services.svelte';
  import Settings from './pages/Settings.svelte';

  let path = $state(location.pathname);
  let username = $state('');
  let ready = $state(false);
  let setupNeeded = $state(false);

  function go(to: string) {
    history.pushState({}, '', to);
    path = to;
  }

  onMount(() => {
    const onPop = () => (path = location.pathname);
    window.addEventListener('popstate', onPop);
    refresh();
    return () => window.removeEventListener('popstate', onPop);
  });

  async function refresh() {
    try {
      const me = await api<{ username: string; setup_needed: boolean }>('/api/me');
      setupNeeded = me.setup_needed;
      username = me.username;
    } catch (e) {
      const err = e as ApiError;
      if (err.code === 'setup_required') setupNeeded = true;
      else {
        setupNeeded = false;
        username = '';
      }
    } finally {
      ready = true;
    }
  }

  let page = $derived.by(() => {
    if (path.startsWith('/apps/') && path !== '/apps/new') {
      return { name: 'app', id: decodeURIComponent(path.slice('/apps/'.length)) };
    }
    if (path === '/apps/new') return { name: 'new' };
    if (path === '/apps') return { name: 'apps' };
    if (path === '/files') return { name: 'files' };
    if (path === '/services') return { name: 'services' };
    if (path === '/settings') return { name: 'settings' };
    return { name: 'home' };
  });
</script>

{#if !ready}
  <div class="auth-wrap"><p>Loading CoduOS…</p></div>
{:else if setupNeeded}
  <Setup onDone={() => { setupNeeded = false; refresh(); go('/'); }} />
{:else if !username}
  <Login onDone={() => refresh()} />
{:else}
  <Shell {path} {username} {go} onLogout={async () => { await api('/api/logout', { method: 'POST' }); username = ''; }}>
    {#if page.name === 'home'}
      <Dashboard {go} />
    {:else if page.name === 'apps'}
      <Apps {go} />
    {:else if page.name === 'new'}
      <AppEditor {go} />
    {:else if page.name === 'app'}
      <AppEditor {go} id={page.id} />
    {:else if page.name === 'files'}
      <Files />
    {:else if page.name === 'services'}
      <Services />
    {:else}
      <Settings />
    {/if}
  </Shell>
{/if}
