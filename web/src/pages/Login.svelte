<script lang="ts">
  import { api } from '../lib/api';

  let { onDone } = $props<{ onDone: () => void }>();
  let username = $state('');
  let password = $state('');
  let error = $state('');
  let busy = $state(false);

  async function submit(e: Event) {
    e.preventDefault();
    busy = true;
    error = '';
    try {
      await api('/api/login', { method: 'POST', body: JSON.stringify({ username, password }) });
      onDone();
    } catch (err: any) {
      error = err.message === 'unauthorized' ? 'Wrong username or password' : err.message;
    } finally {
      busy = false;
    }
  }
</script>

<div class="auth-wrap">
  <form class="auth-card" onsubmit={submit}>
    <h1>CoduOS</h1>
    <p>Sign in to manage this server.</p>
    <label class="field"><span>Username</span><input bind:value={username} autocomplete="username" required /></label>
    <label class="field"><span>Password</span><input type="password" bind:value={password} autocomplete="current-password" required /></label>
    {#if error}<div class="err">{error}</div>{/if}
    <button class="btn" disabled={busy}>{busy ? 'Signing in…' : 'Sign in'}</button>
  </form>
</div>
