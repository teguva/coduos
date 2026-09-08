<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type ApiError } from './lib/api';
  import { appIcons, ensureIconIndex } from './lib/icons';
  import Desktop from './components/Desktop.svelte';
  import AppWindow from './components/AppWindow.svelte';
  import Setup from './pages/Setup.svelte';
  import Login from './pages/Login.svelte';
  import Apps from './pages/Apps.svelte';
  import AppEditor from './pages/AppEditor.svelte';
  import Files from './pages/Files.svelte';
  import Settings from './pages/Settings.svelte';
  import Tasks from './pages/Tasks.svelte';

  let path = $state(location.pathname);
  let search = $state(location.search);
  let username = $state('');
  let ready = $state(false);
  let setupNeeded = $state(false);
  let bootError = $state('');

  function go(to: string) {
    history.pushState({}, '', to);
    path = location.pathname;
    search = location.search;
  }

  onMount(() => {
    const onPop = () => {
      path = location.pathname;
      search = location.search;
    };
    window.addEventListener('popstate', onPop);
    ensureIconIndex();
    refresh();
    return () => window.removeEventListener('popstate', onPop);
  });

  async function refresh() {
    bootError = '';
    try {
      const status = await api<{ needed: boolean }>('/api/setup/status');
      if (status.needed) {
        setupNeeded = true;
        username = '';
        return;
      }
      setupNeeded = false;
      try {
        const me = await api<{ username: string; setup_needed: boolean }>('/api/me');
        username = me.username;
        if (me.setup_needed) {
          setupNeeded = true;
          username = '';
        }
      } catch (e) {
        const err = e as ApiError;
        if (err.code === 'setup_required') {
          setupNeeded = true;
          username = '';
        } else {
          username = '';
        }
      }
    } catch (e: any) {
      bootError = e.message || 'Cannot reach CoduOS';
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
    if (path === '/tasks') return { name: 'tasks' };
    if (path === '/storage' || path === '/settings/storage') return { name: 'settings', pane: 'storage' };
    if (path === '/network' || path === '/settings/network') return { name: 'settings', pane: 'network' };
    if (path === '/vpn' || path === '/settings/vpn') return { name: 'settings', pane: 'vpn' };
    if (path === '/ddns' || path === '/settings/ddns') return { name: 'settings', pane: 'ddns' };
    if (path === '/proxy' || path === '/settings/proxy') return { name: 'settings', pane: 'proxy' };
    if (path === '/services' || path === '/settings/services') return { name: 'settings', pane: 'services' };
    if (path === '/settings' || path === '/settings/general') return { name: 'settings', pane: 'general' };
    if (path.startsWith('/settings/')) {
      return { name: 'settings', pane: decodeURIComponent(path.slice('/settings/'.length)) || 'general' };
    }
    return { name: 'home' };
  });

  let proxyPrefill = $derived.by(() => {
    const q = new URLSearchParams(search);
    const port = q.get('port');
    return {
      hostname: q.get('host') || undefined,
      port: port ? Number(port) : undefined,
      app_id: q.get('app') || undefined
    };
  });
</script>

{#if !ready}
  <div class="auth-wrap"><p>Loading CoduOS…</p></div>
{:else if bootError}
  <div class="auth-wrap">
    <div class="auth-card">
      <h1>CoduOS</h1>
      <p>Could not reach the daemon.</p>
      <div class="err">{bootError}</div>
      <button class="btn" onclick={() => { ready = false; refresh(); }}>Retry</button>
    </div>
  </div>
{:else if setupNeeded}
  <Setup onDone={() => { setupNeeded = false; refresh(); go('/'); }} />
{:else if !username}
  <Login onDone={() => refresh()} onNeedSetup={() => { setupNeeded = true; username = ''; }} />
{:else}
  <Desktop {username} {go} {path} onLogout={async () => { await api('/api/logout', { method: 'POST' }); username = ''; }} />
  {#if page.name === 'files'}
    <Files onClose={() => go('/')} />
  {:else if page.name === 'apps'}
    <AppWindow title="Apps" icon={appIcons.apps} size="wide" onClose={() => go('/')}>
      <Apps {go} />
    </AppWindow>
  {:else if page.name === 'new'}
    <AppWindow title="Install app" icon={appIcons.install} size="wide" onClose={() => go('/')}>
      <AppEditor {go} />
    </AppWindow>
  {:else if page.name === 'app'}
    <AppWindow title="App settings" icon={appIcons.docker} size="wide" onClose={() => go('/')}>
      <AppEditor {go} id={page.id} />
    </AppWindow>
  {:else if page.name === 'settings'}
    <AppWindow title="Settings" icon={appIcons.settings} size="xl" flush onClose={() => go('/')}>
      <Settings pane={page.pane} {go} prefill={proxyPrefill} />
    </AppWindow>
  {:else if page.name === 'tasks'}
    <AppWindow title="Task Manager" icon={appIcons.tasks} size="wide" onClose={() => go('/')}>
      <Tasks />
    </AppWindow>
  {/if}
{/if}
