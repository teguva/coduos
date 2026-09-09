<script lang="ts">
  import { phaseLabel, type AppStatus } from '../lib/apps';

  let { status } = $props<{ status: AppStatus }>();
  let label = $derived(phaseLabel(status));
</script>

<span class="status-pill {status.phase}" title={status.message || label}>
  <span class="dot" class:on={status.phase === 'running'} class:off={status.phase === 'error'}></span>
  {label}
</span>
