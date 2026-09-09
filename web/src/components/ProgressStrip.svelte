<script lang="ts">
  let { percent = null, pulse = false } = $props<{ percent?: number | null; pulse?: boolean }>();
  let indeterminate = $derived(pulse || percent == null);
</script>

<div
  class="progress-strip"
  class:pulse={indeterminate}
  role="progressbar"
  aria-valuemin={0}
  aria-valuemax={100}
  aria-valuenow={indeterminate ? undefined : percent ?? 0}
  aria-label="Progress"
>
  <i style={indeterminate ? undefined : `--p:${percent ?? 0}%`}></i>
</div>
