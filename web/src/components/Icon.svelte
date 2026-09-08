<script lang="ts">
  import { resolveIcon } from '../lib/icons';

  let {
    name,
    alt = '',
    size = 24,
    fallback = 'unknown',
    class: className = ''
  } = $props<{
    name: string;
    alt?: string;
    size?: number;
    fallback?: string;
    class?: string;
  }>();

  let failed = $state(false);

  $effect(() => {
    name;
    failed = false;
  });

  let src = $derived(resolveIcon(failed ? fallback : name));
</script>

<img
  class="rev-icon {className}"
  {src}
  {alt}
  width={size}
  height={size}
  draggable="false"
  onerror={() => {
    if (!failed) failed = true;
  }}
/>
