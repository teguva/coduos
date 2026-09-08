<script lang="ts">
  import { toggleTheme, isLight } from '../lib/api';

  let { path, username, go, onLogout, children } = $props<{
    path: string;
    username: string;
    go: (to: string) => void;
    onLogout: () => void;
    children: any;
  }>();

  let light = $state(isLight());

  const items = [
    { href: '/', label: 'Home' },
    { href: '/apps', label: 'Apps' },
    { href: '/files', label: 'Files' },
    { href: '/services', label: 'Services' },
    { href: '/settings', label: 'Settings' }
  ];

  function active(href: string) {
    if (href === '/') return path === '/';
    return path === href || path.startsWith(href + '/');
  }
</script>

<div class="shell">
  <nav class="nav">
    <div class="brand"><span class="brand-mark"></span> CoduOS</div>
    {#each items as item}
      <button class="link" class:active={active(item.href)} onclick={() => go(item.href)}>{item.label}</button>
    {/each}
    <div class="spacer"></div>
    <button class="link" onclick={() => { toggleTheme(); light = isLight(); }}>{light ? 'Dark mode' : 'Light mode'}</button>
    <button class="link" onclick={onLogout}>Sign out ({username})</button>
  </nav>
  <main class="main">
    {@render children()}
  </main>
</div>
