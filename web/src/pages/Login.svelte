<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import UiIcon from '../components/UiIcon.svelte';

  let { onDone, onNeedSetup } = $props<{ onDone: () => void; onNeedSetup: () => void }>();
  let username = $state('');
  let password = $state('');
  let error = $state('');
  let busy = $state(false);

  onMount(async () => {
    try {
      const status = await api<{ needed: boolean }>('/api/setup/status');
      if (status.needed) onNeedSetup();
    } catch {
      /* keep login; App.svelte already handled unreachable daemon */
    }
  });

  async function submit(e: Event) {
    e.preventDefault();
    busy = true;
    error = '';
    try {
      await api('/api/login', { method: 'POST', body: JSON.stringify({ username, password }) });
      onDone();
    } catch (err: any) {
      if (err.code === 'setup_required') {
        onNeedSetup();
        return;
      }
      error = err.message === 'unauthorized' ? 'Wrong username or password' : err.message;
    } finally {
      busy = false;
    }
  }
</script>

<div class="auth-wrap">
  <form class="auth-card" onsubmit={submit}>
    <div class="auth-logo"><UiIcon name="computer" size={48} /></div>
    <h1>CoduOS</h1>
    <p>Sign in with the admin account created on first start.</p>
    <label class="field"><span>Username</span><input bind:value={username} autocomplete="username" required /></label>
    <label class="field"><span>Password</span><input type="password" bind:value={password} autocomplete="current-password" required /></label>
    {#if error}<div class="err">{error}</div>{/if}
    <button class="btn" disabled={busy}>{busy ? 'Signing in…' : 'Sign in'}</button>
  </form>
</div>
