<script lang="ts">
  import { onMount } from 'svelte';
  import UiIcon from './UiIcon.svelte';

  let { label, text } = $props<{ label: string; text: string }>();
  let open = $state(false);
  let root = $state<HTMLElement | null>(null);

  function toggle() {
    open = !open;
  }

  onMount(() => {
    const onDoc = (ev: MouseEvent) => {
      if (!open || !root) return;
      if (!root.contains(ev.target as Node)) open = false;
    };
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === 'Escape') open = false;
    };
    document.addEventListener('mousedown', onDoc);
    window.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDoc);
      window.removeEventListener('keydown', onKey);
    };
  });
</script>

<span class="info-tip" bind:this={root}>
  <button type="button" class="info-tip-btn" aria-label={label} aria-expanded={open} onclick={toggle}>
    <UiIcon name="info" size={16} />
  </button>
  {#if open}
    <span class="info-tip-pop" role="tooltip">{text}</span>
  {/if}
</span>
