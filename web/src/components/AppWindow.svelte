<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import UiIcon from './UiIcon.svelte';

  let {
    title,
    icon,
    onClose,
    size = 'narrow',
    flush = false,
    actions,
    children
  } = $props<{
    title: string;
    icon?: string;
    onClose: () => void;
    size?: 'narrow' | 'wide' | 'sheet' | 'xl';
    flush?: boolean;
    actions?: Snippet;
    children: Snippet;
  }>();

  let phone = $state(false);
  let sheet = $derived(phone || size === 'sheet');

  onMount(() => {
    const mq = window.matchMedia('(max-width: 720px)');
    const apply = () => (phone = mq.matches);
    apply();
    mq.addEventListener('change', apply);
    return () => mq.removeEventListener('change', apply);
  });
</script>

<div
  class="os-window"
  class:sheet
  role="presentation"
  onclick={(e) => {
    if (!sheet && e.currentTarget === e.target) onClose();
  }}
>
  <div class="os-card {size}" class:sheet role="dialog" aria-label={title}>
    <header class="os-head">
      {#if icon}<UiIcon name={icon} size={22} />{/if}
      <h1>{title}</h1>
      {#if actions}
        <div class="os-actions">
          {@render actions()}
        </div>
      {/if}
      <button class="close-x" onclick={onClose} aria-label="Close">
        <UiIcon name="close" size={20} />
      </button>
    </header>
    <div class="os-body" class:flush>
      {@render children()}
    </div>
  </div>
</div>
