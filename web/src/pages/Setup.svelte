<script lang="ts">
  import { api } from '../lib/api';

  let { onDone } = $props<{ onDone: () => void }>();
  let username = $state('admin');
  let password = $state('');
  let error = $state('');
  let busy = $state(false);

  async function submit(e: Event) {
    e.preventDefault();
    busy = true;
    error = '';
    try {
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
    <h1>Set up CoduOS</h1>
    <p>Create the admin account for this machine. There is only one user in v1.</p>
    <label class="field"><span>Username</span><input bind:value={username} autocomplete="username" required /></label>
    <label class="field"><span>Password (8+ characters)</span><input type="password" bind:value={password} autocomplete="new-password" required /></label>
    {#if error}<div class="err">{error}</div>{/if}
    <button class="btn" disabled={busy}>{busy ? 'Creating…' : 'Create admin'}</button>
  </form>
</div>
