<script lang="ts">
  import type { StackForm } from '../lib/compose';
  import UiIcon from './UiIcon.svelte';

  let { stack = $bindable(), onOpenService } = $props<{
    stack: StackForm;
    onOpenService?: (index: number) => void;
  }>();

  let query = $state('');
  let addTo = $state(0);

  const q = $derived(query.trim().toLowerCase());
  const total = $derived(stack.services.reduce((n, s) => n + s.env.filter((e) => e.key.trim()).length, 0));
  const hasRows = $derived(stack.services.some((s) => s.env.length));

  function visible(si: number) {
    const svc = stack.services[si];
    if (!svc) return [];
    if (!q) return svc.env.map((_, i) => i);
    const name = (svc.serviceName || '').toLowerCase();
    if (name.includes(q)) return svc.env.map((_, i) => i);
    return svc.env
      .map((e, i) => (e.key.toLowerCase().includes(q) || e.value.toLowerCase().includes(q) ? i : -1))
      .filter((i) => i >= 0);
  }

  function add() {
    const i = Math.min(Math.max(0, Number(addTo) || 0), Math.max(0, stack.services.length - 1));
    if (!stack.services[i]) return;
    query = '';
    stack.services[i].env = [...stack.services[i].env, { key: '', value: '' }];
  }

  function addOn(i: number) {
    addTo = i;
    stack.services[i].env = [...stack.services[i].env, { key: '', value: '' }];
  }

  function drop(si: number, ei: number) {
    stack.services[si].env = stack.services[si].env.filter((_, n) => n !== ei);
  }
</script>

<div class="env-toolbar">
  <label class="field">
    <span class="sr-only">Filter variables</span>
    <input bind:value={query} placeholder="Filter by key, value, or service" />
  </label>
  <label class="field env-add-svc">
    <span>Add to</span>
    <select bind:value={addTo}>
      {#each stack.services as svc, i}
        <option value={i}>{svc.serviceName || `service-${i + 1}`}</option>
      {/each}
    </select>
  </label>
  <button type="button" class="btn secondary compact" onclick={add}>
    <UiIcon name="add" size={18} /> Add variable
  </button>
</div>

<p class="hint">
  {#if total === 0}
    All environment variables for this stack, in one list. Add a key to a service to get started.
  {:else}
    {total} variable{total === 1 ? '' : 's'} across {stack.services.length} service{stack.services.length === 1 ? '' : 's'}.
  {/if}
</p>
{#if hasRows}
  <div class="kv-head two"><span>Key</span><span>Value</span><span></span></div>
{/if}

{#each stack.services as svc, si}
  {@const shown = visible(si)}
  {@const count = q ? shown.length : svc.env.filter((e) => e.key.trim()).length}
  {#if shown.length || !q}
    <section class="env-group">
      <div class="sec-head">
        <h3>
          {#if onOpenService}
            <button type="button" class="env-jump" onclick={() => onOpenService(si)}>
              {svc.serviceName || `service-${si + 1}`}
            </button>
          {:else}
            {svc.serviceName || `service-${si + 1}`}
          {/if}
          {#if count}
            <span class="env-count">{count}</span>
          {/if}
        </h3>
        <button type="button" class="btn secondary compact" onclick={() => addOn(si)}>Add</button>
      </div>
      {#if shown.length === 0}
        <p class="hint">No variables on this service.</p>
      {:else}
        {#each shown as ei (si + ':' + ei)}
          <div class="kv-row two">
            <input bind:value={svc.env[ei].key} placeholder="POSTGRES_PASSWORD" spellcheck="false" />
            <input bind:value={svc.env[ei].value} placeholder="value" spellcheck="false" />
            <button type="button" class="close-x" title="Remove" onclick={() => drop(si, ei)}>
              <UiIcon name="close" size={16} />
            </button>
          </div>
        {/each}
      {/if}
    </section>
  {/if}
{/each}

{#if q && stack.services.every((_, i) => visible(i).length === 0)}
  <p class="hint">No variables match “{query.trim()}”.</p>
{/if}
