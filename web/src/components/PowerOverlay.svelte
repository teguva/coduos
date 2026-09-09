<script lang="ts">
  import { powerView } from '../lib/power';

  const view = $derived($powerView);

  const title = $derived.by(() => {
    if (!view.action) return '';
    if (view.action === 'reboot') {
      return view.down ? 'Waiting for this computer' : 'Restarting…';
    }
    return view.down ? 'This computer is off' : 'Shutting down…';
  });

  const body = $derived.by(() => {
    if (!view.action) return '';
    if (view.action === 'reboot') {
      return view.down
        ? 'CoduOS will reopen in this tab when the machine is back.'
        : 'The dashboard will wait here until the computer finishes restarting.';
    }
    return view.down
      ? 'Turn it on at the device when you want CoduOS again. This tab will reopen the dashboard if you do.'
      : 'Waiting for the machine to power off.';
  });
</script>

{#if view.action}
  <div class="power-veil" role="alertdialog" aria-modal="true" aria-live="polite" aria-labelledby="power-title" aria-describedby="power-body">
    <div class="power-card">
      {#if !(view.action === 'shutdown' && view.down)}
        <div class="power-spin" aria-hidden="true"></div>
      {/if}
      <h1 id="power-title">{title}</h1>
      <p id="power-body">{body}</p>
    </div>
  </div>
{/if}
