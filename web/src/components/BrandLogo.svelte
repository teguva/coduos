<script lang="ts">
  import { onMount } from 'svelte';
  import { isLight } from '../lib/api';

  let {
    kind,
    alt = 'CoduOS',
    class: className = ''
  } = $props<{
    kind: 'square' | 'text';
    alt?: string;
    class?: string;
  }>();

  let light = $state(isLight());
  let src = $derived(
    kind === 'square'
      ? light
        ? '/logos/square-light.svg'
        : '/logos/square-dark.svg'
      : light
        ? '/logos/text-dark.svg'
        : '/logos/text-light.svg'
  );

  onMount(() => {
    const onTheme = () => (light = isLight());
    window.addEventListener('coduos-theme', onTheme);
    return () => window.removeEventListener('coduos-theme', onTheme);
  });
</script>

<img class="brand-logo brand-{kind} {className}" {src} {alt} />
