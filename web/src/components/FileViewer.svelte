<script lang="ts">
  import { onMount } from 'svelte';
  import { bytes } from '../lib/format';
  import { fileUrl, previewKind, TEXT_PREVIEW_MAX, type PreviewKind } from '../lib/fileKind';
  import { fileIcon } from '../lib/icons';
  import Icon from './Icon.svelte';
  import UiIcon from './UiIcon.svelte';

  type Entry = { name: string; path: string; dir: boolean; size: number; modified?: number | null };

  let {
    root,
    entry,
    siblings,
    onClose,
    onChange
  } = $props<{
    root: string;
    entry: Entry;
    siblings: Entry[];
    onClose: () => void;
    onChange: (ent: Entry) => void;
  }>();

  let text = $state('');
  let textErr = $state('');
  let loadingText = $state(false);

  let files = $derived(siblings.filter((e) => !e.dir));
  let index = $derived(Math.max(0, files.findIndex((e) => e.path === entry.path)));
  let kind = $derived(previewKind(entry.name) as PreviewKind);
  let url = $derived(fileUrl(root, entry.path, true));
  let hasPrev = $derived(index > 0);
  let hasNext = $derived(index >= 0 && index < files.length - 1);

  async function loadText(ent: Entry) {
    text = '';
    textErr = '';
    if (previewKind(ent.name) !== 'text') return;
    if (ent.size > TEXT_PREVIEW_MAX) {
      textErr = `This file is ${bytes(ent.size)}. Open a download instead of previewing large text.`;
      return;
    }
    loadingText = true;
    try {
      const res = await fetch(fileUrl(root, ent.path, true), { credentials: 'include' });
      if (!res.ok) throw new Error(res.statusText);
      text = await res.text();
    } catch (e: any) {
      textErr = e.message || 'Could not load file';
    } finally {
      loadingText = false;
    }
  }

  $effect(() => {
    loadText(entry);
  });

  function prev() {
    if (!hasPrev) return;
    onChange(files[index - 1]);
  }

  function next() {
    if (!hasNext) return;
    onChange(files[index + 1]);
  }

  function download() {
    window.open(fileUrl(root, entry.path, false));
  }

  onMount(() => {
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === 'Escape') {
        ev.preventDefault();
        onClose();
      } else if (ev.key === 'ArrowLeft') {
        ev.preventDefault();
        prev();
      } else if (ev.key === 'ArrowRight') {
        ev.preventDefault();
        next();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
</script>

<div
  class="viewer"
  role="dialog"
  tabindex="-1"
  aria-label={entry.name}
  onclick={(e) => { if (e.currentTarget === e.target) onClose(); }}
  onkeydown={(e) => { if (e.key === 'Escape') onClose(); }}
>
  <header class="viewer-bar">
    <button class="btn secondary icon-only viewer-nav prev" disabled={!hasPrev} onclick={prev} aria-label="Previous">
      <UiIcon name="chevron_left" size={20} />
    </button>
    <div class="viewer-title">
      <Icon name={fileIcon(entry.name, false)} size={20} alt="" />
      <span class="clip">{entry.name}</span>
      <span class="meta">{bytes(entry.size)}</span>
    </div>
    <button class="btn secondary icon-only viewer-nav next" disabled={!hasNext} onclick={next} aria-label="Next">
      <UiIcon name="chevron_right" size={20} />
    </button>
    <button class="btn secondary" onclick={download}>
      <UiIcon name="download" size={16} /> Download
    </button>
    <button class="close-x" onclick={onClose} aria-label="Close">
      <UiIcon name="close" size={18} />
    </button>
  </header>
  <div class="viewer-stage">
    {#key entry.path}
    {#if kind === 'image'}
      <img src={url} alt={entry.name} />
    {:else if kind === 'video'}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video src={url} controls autoplay playsinline></video>
    {:else if kind === 'audio'}
      <div class="viewer-audio">
        <Icon name={fileIcon(entry.name, false)} size={72} alt="" />
        <audio src={url} controls autoplay></audio>
      </div>
    {:else if kind === 'pdf'}
      <iframe class="viewer-pdf" src={url} title={entry.name}></iframe>
    {:else if kind === 'text'}
      {#if loadingText}
        <div class="meta">Loading…</div>
      {:else if textErr}
        <div class="viewer-unknown">
          <p>{textErr}</p>
          <button class="btn" onclick={download}>Download</button>
        </div>
      {:else}
        <pre class="viewer-text">{text}</pre>
      {/if}
    {:else}
      <div class="viewer-unknown">
        <Icon name={fileIcon(entry.name, false)} size={72} alt="" />
        <p>{entry.name}</p>
        <p class="meta">{bytes(entry.size)} · no built-in preview</p>
        <button class="btn" onclick={download}>Download</button>
      </div>
    {/if}
    {/key}
  </div>
</div>
