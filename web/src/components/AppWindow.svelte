<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';

  let {
    title,
    icon,
    onClose,
    size = 'narrow',
    children
  } = $props<{
    title: string;
    icon?: string;
    onClose: () => void;
    size?: 'narrow' | 'wide' | 'sheet';
    children: any;
  }>();

  let phone = $state(false);

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
  class:sheet={phone || size === 'sheet'}
  role="presentation"
  onclick={(e) => {
    if (e.currentTarget === e.target) onClose();
  }}
>
  <div class="os-card {size}" class:sheet={phone || size === 'sheet'} role="dialog" aria-label={title}>
    <header class="os-head">
      {#if icon}<Icon name={icon} size={22} alt="" />{/if}
      <h1>{title}</h1>
      <button class="close-x" onclick={onClose} aria-label="Close">
        <Icon name="close" size={18} alt="" />
      </button>
    </header>
    <div class="os-body">
      {@render children()}
    </div>
  </div>
</div>
