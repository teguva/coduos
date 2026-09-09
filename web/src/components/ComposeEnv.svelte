<script lang="ts">
  import {
    interpolationsFromStack,
    mergeEnv,
    parseDotEnv,
    showServiceEnv,
    type StackForm
  } from '../lib/compose';
  import UiIcon from './UiIcon.svelte';

  let { stack = $bindable() } = $props<{
    stack: StackForm;
  }>();

  let query = $state('');
  let addTo = $state('shared');
  let fileErr = $state('');
  let fileEl: HTMLInputElement | undefined;

  const q = $derived(query.trim().toLowerCase());
  const envFiles = $derived(
    [...new Set(stack.services.flatMap((s) => s.envFiles))].filter(Boolean)
  );

  $effect(() => {
    const found = interpolationsFromStack(stack);
    const have = new Set(stack.dotEnv.map((e) => e.key.trim()).filter(Boolean));
    const missing = found.filter((e) => e.key && !have.has(e.key));
    if (!missing.length) return;
    stack.dotEnv = [...stack.dotEnv, ...missing];
  });

  const sharedKeys = $derived(
    new Set(stack.dotEnv.map((e) => e.key.trim()).filter(Boolean))
  );

  type SharedLoc = { kind: 'shared'; ei: number };
  type SvcLoc = { kind: 'svc'; si: number; ei: number };

  const sharedRows = $derived.by(() => {
    const out: SharedLoc[] = [];
    for (let ei = 0; ei < stack.dotEnv.length; ei++) {
      const e = stack.dotEnv[ei];
      if (!q) {
        out.push({ kind: 'shared', ei });
        continue;
      }
      if (e.key.toLowerCase().includes(q) || e.value.toLowerCase().includes(q)) {
        out.push({ kind: 'shared', ei });
      }
    }
    return out;
  });

  const svcRows = $derived.by(() => {
    const out: SvcLoc[] = [];
    for (let si = 0; si < stack.services.length; si++) {
      const svc = stack.services[si];
      for (let ei = 0; ei < svc.env.length; ei++) {
        const e = svc.env[ei];
        if (!showServiceEnv(e, sharedKeys)) continue;
        if (!q) {
          out.push({ kind: 'svc', si, ei });
          continue;
        }
        const name = (svc.serviceName || '').toLowerCase();
        if (
          name.includes(q) ||
          e.key.toLowerCase().includes(q) ||
          e.value.toLowerCase().includes(q)
        ) {
          out.push({ kind: 'svc', si, ei });
        }
      }
    }
    return out;
  });

  const visible = $derived(sharedRows.length + svcRows.length);
  const namedShared = $derived(stack.dotEnv.filter((e) => e.key.trim()).length);
  const namedSvc = $derived(
    stack.services.reduce((n, s) => {
      return (
        n +
        s.env.filter((e) => e.key.trim() && showServiceEnv(e, sharedKeys)).length
      );
    }, 0)
  );

  function add() {
    query = '';
    if (addTo === 'shared') {
      stack.dotEnv = [...stack.dotEnv, { key: '', value: '' }];
      return;
    }
    const i = Math.min(Math.max(0, Number(addTo) || 0), Math.max(0, stack.services.length - 1));
    if (!stack.services[i]) return;
    stack.services[i].env = [...stack.services[i].env, { key: '', value: '' }];
  }

  function dropShared(ei: number) {
    stack.dotEnv = stack.dotEnv.filter((_, n) => n !== ei);
  }

  function dropSvc(si: number, ei: number) {
    stack.services[si].env = stack.services[si].env.filter((_, n) => n !== ei);
  }

  function moveSvc(si: number, ei: number, dest: string) {
    const row = stack.services[si].env[ei];
    if (!row) return;
    if (dest === 'shared') {
      stack.services[si].env = stack.services[si].env.filter((_, n) => n !== ei);
      stack.dotEnv = [...stack.dotEnv, row];
      return;
    }
    const next = Number(dest);
    if (next === si || next < 0 || next >= stack.services.length) return;
    stack.services[si].env = stack.services[si].env.filter((_, n) => n !== ei);
    stack.services[next].env = [...stack.services[next].env, row];
  }

  function moveShared(ei: number, dest: string) {
    if (dest === 'shared') return;
    const next = Number(dest);
    if (next < 0 || next >= stack.services.length) return;
    const row = stack.dotEnv[ei];
    if (!row) return;
    stack.dotEnv = stack.dotEnv.filter((_, n) => n !== ei);
    stack.services[next].env = [...stack.services[next].env, row];
  }

  async function onEnvFile(e: Event) {
    fileErr = '';
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    const text = await file.text();
    const parsed = parseDotEnv(text);
    if (!parsed.length) {
      fileErr = 'That file had no KEY=value lines.';
      return;
    }
    stack.dotEnv = mergeEnv(interpolationsFromStack(stack), stack.dotEnv, parsed);
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
      <option value="shared">Shared</option>
      {#each stack.services as svc, i}
        <option value={String(i)}>{svc.serviceName || `service-${i + 1}`}</option>
      {/each}
    </select>
  </label>
  <button type="button" class="btn secondary compact" onclick={() => fileEl?.click()}>
    <UiIcon name="upload" size={18} /> Upload .env
  </button>
  <input
    bind:this={fileEl}
    type="file"
    hidden
    accept=".env,text/plain,.txt"
    onchange={onEnvFile}
  />
  <button type="button" class="btn secondary compact" onclick={add}>
    <UiIcon name="add" size={18} /> Add variable
  </button>
</div>

<p class="hint">
  {#if namedShared === 0 && namedSvc === 0}
    Shared variables fill the image version and host folders. Upload a .env (Immich includes one) or add a variable.
  {:else}
    {namedShared + namedSvc} variable{namedShared + namedSvc === 1 ? '' : 's'} — image tags, upload folders, and container settings.
  {/if}
</p>
{#if envFiles.length}
  <p class="hint">This stack also reads {envFiles.join(', ')}. Upload that file so passwords and paths show up here.</p>
{/if}
{#if fileErr}<p class="err">{fileErr}</p>{/if}

{#if sharedRows.length}
  <div class="env-group">
    <h3>Shared</h3>
    <div class="kv-head env"><span>Where</span><span>Key</span><span>Value</span><span></span></div>
    {#each sharedRows as { ei } (`shared:${ei}`)}
      <div class="kv-row env">
        <select
          value="shared"
          onchange={(e) => moveShared(ei, e.currentTarget.value)}
          aria-label="Where"
        >
          <option value="shared">Shared</option>
          {#each stack.services as svc, i}
            <option value={String(i)}>{svc.serviceName || `service-${i + 1}`}</option>
          {/each}
        </select>
        <input bind:value={stack.dotEnv[ei].key} placeholder="UPLOAD_LOCATION" spellcheck="false" />
        <input bind:value={stack.dotEnv[ei].value} placeholder="value" spellcheck="false" />
        <button type="button" class="close-x" title="Remove" onclick={() => dropShared(ei)}>
          <UiIcon name="close" size={16} />
        </button>
      </div>
    {/each}
  </div>
{/if}

{#each stack.services as svc, si}
  {@const rows = svcRows.filter((r) => r.si === si)}
  {#if rows.length}
    <div class="env-group">
      <h3>{svc.serviceName || `service-${si + 1}`}</h3>
      <div class="kv-head env"><span>Where</span><span>Key</span><span>Value</span><span></span></div>
      {#each rows as { ei } (`svc:${si}:${ei}`)}
        <div class="kv-row env">
          <select
            value={String(si)}
            onchange={(e) => moveSvc(si, ei, e.currentTarget.value)}
            aria-label="Where"
          >
            <option value="shared">Shared</option>
            {#each stack.services as other, i}
              <option value={String(i)}>{other.serviceName || `service-${i + 1}`}</option>
            {/each}
          </select>
          <input bind:value={stack.services[si].env[ei].key} placeholder="POSTGRES_INITDB_ARGS" spellcheck="false" />
          <input bind:value={stack.services[si].env[ei].value} placeholder="value" spellcheck="false" />
          <button type="button" class="close-x" title="Remove" onclick={() => dropSvc(si, ei)}>
            <UiIcon name="close" size={16} />
          </button>
        </div>
      {/each}
    </div>
  {/if}
{/each}

{#if !visible && q}
  <p class="hint">No variables match “{query.trim()}”.</p>
{/if}
