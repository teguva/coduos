<script lang="ts">
  import { api } from '../lib/api';
  import BrandLogo from '../components/BrandLogo.svelte';

  let { onDone } = $props<{ onDone: () => void }>();
  let username = $state('admin');
  let password = $state('');
  let confirm = $state('');
  let error = $state('');
  let busy = $state(false);

  async function submit(e: Event) {
    e.preventDefault();
    busy = true;
    error = '';
    try {
      if (password !== confirm) {
        throw new Error('Passwords do not match');
      }
      await api('/api/setup', { method: 'POST', body: JSON.stringify({ username, password }) });
      onDone();
    } catch (err: any) {
      error = err.message;
    } finally {
      busy = false;
    }
  }
</script>

<div class="auth-wrap">
  <form class="auth-card" onsubmit={submit}>
    <div class="auth-logo">
      <BrandLogo kind="square" />
    </div>
    <h1>Create your admin account</h1>
    <p>First start on this machine. Pick a username and password — CoduOS has no default login.</p>
    <label class="field"><span>Username</span><input bind:value={username} autocomplete="username" required /></label>
    <label class="field"><span>Password (8+ characters)</span><input type="password" bind:value={password} autocomplete="new-password" required minlength="8" /></label>
    <label class="field"><span>Confirm password</span><input type="password" bind:value={confirm} autocomplete="new-password" required minlength="8" /></label>
    {#if error}<div class="err">{error}</div>{/if}
    <button class="btn" disabled={busy}>{busy ? 'Creating…' : 'Create admin'}</button>
  </form>
</div>
