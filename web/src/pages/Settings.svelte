<script lang="ts">
  import { onMount } from 'svelte';
  import { api, isLight, setTheme } from '../lib/api';
  import Icon from '../components/Icon.svelte';
  import Confirm from '../components/Confirm.svelte';

  type Root = { id: string; label: string; path: string };
  type Unit = { id: string; unit: string; label: string };
  type Settings = {
    bind: string;
    data_dir: string;
    github_owner: string;
    github_repo: string;
    file_roots: Root[];
    units: Unit[];
    version: string;
    hostname: string;
    privileged: boolean;
  };
  type Update = {
    current: string;
    latest?: string | null;
    html_url?: string | null;
    up_to_date: boolean;
    can_apply?: boolean;
    error?: string | null;
  };
  type Discovered = { unit: string; state: string };

  let settings = $state<Settings | null>(null);
  let update = $state<Update | null>(null);
  let error = $state('');
  let notice = $state('');
  let currentPw = $state('');
  let newPw = $state('');
  let confirmPw = $state('');
  let showPw = $state(false);
  let light = $state(isLight());
  let newRoot = $state({ id: '', label: '', path: '' });
  let addingRoot = $state(false);
  let addingUnit = $state(false);
  let unitQuery = $state('');
  let newUnit = $state({ id: '', label: '', unit: '' });
  let applying = $state(false);
  let confirmUpdate = $state(false);
  let open = $state<Record<string, boolean>>({
    appearance: true,
    account: true,
    access: true,
    files: true,
    services: true,
    updates: true,
    about: true
  });

  async function load() {
    settings = await api<Settings>('/api/settings');
    update = await api<Update>('/api/update');
  }

  onMount(() => load().catch((e) => (error = e.message)));

  async function save(file_roots: Root[], units: Unit[]) {
    error = '';
    notice = '';
    settings = await api<Settings>('/api/settings', {
      method: 'PUT',
      body: JSON.stringify({ file_roots, units })
    });
    notice = 'Saved.';
  }

  async function addRoot(e: Event) {
    e.preventDefault();
    if (!settings) return;
    const id = (newRoot.id || newRoot.label).toLowerCase().replace(/[^a-z0-9-]+/g, '-').replace(/^-|-$/g, '');
    const root = { id, label: newRoot.label.trim(), path: newRoot.path.trim() };
    if (!root.id || !root.label || !root.path.startsWith('/')) {
      error = 'Need a label and an absolute path.';
      return;
    }
    try {
      await save([...settings.file_roots, root], settings.units);
      newRoot = { id: '', label: '', path: '' };
      addingRoot = false;
    } catch (err: any) {
      error = err.message;
    }
  }

  async function removeRoot(id: string) {
    if (!settings) return;
    try {
      await save(
        settings.file_roots.filter((r) => r.id !== id),
        settings.units
      );
    } catch (err: any) {
      error = err.message;
    }
  }

  async function loadDiscover() {
    try {
      discovered = await api<Discovered[]>('/api/units/discover');
    } catch {
      discovered = [];
    }
  }

  async function addUnit(e: Event) {
    e.preventDefault();
    if (!settings) return;
    const unitName = newUnit.unit.trim();
    const label = newUnit.label.trim() || unitName.replace(/\.service$/, '');
    const id = (newUnit.id || label).toLowerCase().replace(/[^a-z0-9-]+/g, '-').replace(/^-|-$/g, '');
    if (!id || !unitName) {
      error = 'Pick a unit or type a unit name.';
      return;
    }
    try {
      await save(settings.file_roots, [...settings.units, { id, label, unit: unitName }]);
      newUnit = { id: '', label: '', unit: '' };
      unitQuery = '';
      addingUnit = false;
    } catch (err: any) {
      error = err.message;
    }
  }

  async function removeUnit(id: string) {
    if (!settings) return;
    try {
      await save(
        settings.file_roots,
        settings.units.filter((u) => u.id !== id)
      );
    } catch (err: any) {
      error = err.message;
    }
  }

  async function changePw(e: Event) {
    e.preventDefault();
    error = '';
    notice = '';
    if (newPw !== confirmPw) {
      error = 'New passwords do not match.';
      return;
    }
    try {
      await api('/api/settings/password', {
        method: 'PUT',
        body: JSON.stringify({ current: currentPw, new_password: newPw })
      });
      currentPw = '';
      newPw = '';
      confirmPw = '';
      notice = 'Password updated.';
    } catch (err: any) {
      error = err.message;
    }
  }

  let filteredUnits = $derived(
    discovered
      .filter((u) => {
        const q = unitQuery.toLowerCase();
        return !q || u.unit.toLowerCase().includes(q);
      })
      .slice(0, 12)
  );

  function theme(next: 'light' | 'dark') {
    setTheme(next);
    light = isLight();
  }

  async function waitForRestart() {
    notice = 'Restarting CoduOS…';
    for (let i = 0; i < 40; i++) {
      await new Promise((r) => setTimeout(r, 1000));
      try {
        const res = await fetch('/api/setup/status', { credentials: 'include' });
        if (res.ok) {
          location.reload();
          return;
        }
      } catch {
        /* still down */
      }
    }
    location.reload();
  }

  async function applyUpdate() {
    error = '';
    notice = '';
    applying = true;
    confirmUpdate = false;
    try {
      const res = await api<{ ok: boolean; version: string; restarting: boolean }>('/api/update', {
        method: 'POST'
      });
      notice = `Installed ${res.version}. Restarting…`;
      await waitForRestart();
    } catch (err: any) {
      error = err.message;
      applying = false;
    }
  }
</script>

{#if error}<div class="err">{error}</div>{/if}
{#if notice}<p>{notice}</p>{/if}

<section class="set-sec">
  <button class="set-head" onclick={() => (open.appearance = !open.appearance)}>
    <Icon name="settings" size={20} alt="" />
    <div>
      <h3>Appearance</h3>
      <p>Light or dark. Also available in the top bar.</p>
    </div>
  </button>
  {#if open.appearance}
    <div class="segment">
      <button class="btn secondary" class:active={!light} onclick={() => theme('dark')}>Dark</button>
      <button class="btn secondary" class:active={light} onclick={() => theme('light')}>Light</button>
    </div>
  {/if}
</section>

<section class="set-sec">
  <button class="set-head" onclick={() => (open.account = !open.account)}>
    <Icon name="computer" size={20} alt="" />
    <div>
      <h3>Account</h3>
      <p>Change the admin password. Use 8 or more characters.</p>
    </div>
  </button>
  {#if open.account}
    <form onsubmit={changePw}>
      <label class="field"><span>Current password</span>
        <input type={showPw ? 'text' : 'password'} bind:value={currentPw} required />
      </label>
      <label class="field"><span>New password</span>
        <input type={showPw ? 'text' : 'password'} bind:value={newPw} minlength="8" required />
      </label>
      <label class="field"><span>Confirm new password</span>
        <input type={showPw ? 'text' : 'password'} bind:value={confirmPw} minlength="8" required />
      </label>
      <label class="toggle-row">
        <span>Show passwords</span>
        <input type="checkbox" bind:checked={showPw} />
      </label>
      <button class="btn">Update password</button>
    </form>
  {/if}
</section>

<section class="set-sec">
  <button class="set-head" onclick={() => (open.access = !open.access)}>
    <Icon name="server" size={20} alt="" />
    <div>
      <h3>Access</h3>
      <p>When nginx is in front, the dashboard is on ports 80 and 443.</p>
    </div>
  </button>
  {#if open.access}
    <p class="hint">Daemon bind <code>{settings?.bind ?? '—'}</code>. Public HTTP/HTTPS is owned by nginx on an installed NAS. Use the Proxy app to publish extra hosts.</p>
  {/if}
</section>

<section class="set-sec">
  <button class="set-head" onclick={() => (open.files = !open.files)}>
    <Icon name="folder" size={20} alt="" />
    <div>
      <h3>Files locations</h3>
      <p>Folders shown in the Files app.</p>
    </div>
  </button>
  {#if open.files}
    <div class="card-list">
      {#each settings?.file_roots ?? [] as r}
        <div class="mini-card">
          <div>
            <strong>{r.label}</strong>
            <div class="meta">{r.path}</div>
          </div>
          <button class="btn secondary" onclick={() => removeRoot(r.id)}>Remove</button>
        </div>
      {/each}
    </div>
    {#if addingRoot}
      <form onsubmit={addRoot}>
        <label class="field"><span>Label</span><input bind:value={newRoot.label} placeholder="Media" required /></label>
        <label class="field"><span>Absolute path</span><input bind:value={newRoot.path} placeholder="/mnt/media" required /></label>
        <div class="row">
          <button class="btn">Add location</button>
          <button type="button" class="btn secondary" onclick={() => (addingRoot = false)}>Cancel</button>
        </div>
      </form>
    {:else}
      <button class="btn" onclick={() => (addingRoot = true)}>Add location</button>
    {/if}
  {/if}
</section>

<section class="set-sec">
  <button class="set-head" onclick={() => (open.services = !open.services)}>
    <Icon name="services" size={20} alt="" />
    <div>
      <h3>Host services</h3>
      <p>Allowlisted systemd units the Services app can start and stop.</p>
    </div>
  </button>
  {#if open.services}
    <div class="card-list">
      {#each settings?.units ?? [] as u}
        <div class="mini-card">
          <div>
            <strong>{u.label}</strong>
            <div class="meta">{u.unit}</div>
          </div>
          <button class="btn secondary" onclick={() => removeUnit(u.id)}>Remove</button>
        </div>
      {/each}
    </div>
    {#if addingUnit}
      <form onsubmit={addUnit}>
        <label class="field"><span>Search units</span>
          <input
            bind:value={unitQuery}
            placeholder="nginx.service"
            onfocus={loadDiscover}
          />
        </label>
        {#if filteredUnits.length}
          <div class="picker">
            {#each filteredUnits as u}
              <button
                type="button"
                class="picker-item"
                onclick={() => {
                  newUnit.unit = u.unit;
                  newUnit.label = u.unit.replace(/\.service$/, '');
                  unitQuery = u.unit;
                }}
              >
                {u.unit} <span class="meta">{u.state}</span>
              </button>
            {/each}
          </div>
        {/if}
        <label class="field"><span>Unit name (advanced)</span><input bind:value={newUnit.unit} placeholder="sshd.service" required /></label>
        <label class="field"><span>Label</span><input bind:value={newUnit.label} placeholder="SSH" /></label>
        <div class="row">
          <button class="btn">Add service</button>
          <button type="button" class="btn secondary" onclick={() => (addingUnit = false)}>Cancel</button>
        </div>
      </form>
    {:else}
      <button class="btn" onclick={() => { addingUnit = true; loadDiscover(); }}>Add service</button>
    {/if}
  {/if}
</section>

<section class="set-sec">
  <button class="set-head" onclick={() => (open.updates = !open.updates)}>
    <Icon name="download" size={20} alt="" />
    <div>
      <h3>Updates</h3>
      <p>Install the latest CoduOS release from GitHub.</p>
    </div>
  </button>
  {#if open.updates && update}
    <div class="row">
      {#if update.error}
        <span class="chip warn">Check failed</span>
      {:else if update.up_to_date}
        <span class="chip ok">Up to date</span>
      {:else}
        <span class="chip warn">Update available</span>
      {/if}
      <span class="meta">Current {update.current}{#if update.latest} · latest {update.latest}{/if}</span>
    </div>
    {#if update.error}<div class="err">{update.error}</div>{/if}
    {#if !update.up_to_date && !update.error}
      {#if !settings?.privileged}
        <div class="banner">Updating from here needs the installed daemon running as root.</div>
      {:else if update.can_apply === false}
        <p class="hint">This copy is not the installed service. On the NAS, run <code>scripts/update.sh</code> as root.</p>
      {/if}
      <div class="row">
        <button
          class="btn"
          disabled={!update.can_apply || applying}
          onclick={() => (confirmUpdate = true)}
        >
          {applying ? 'Updating…' : `Update to ${update.latest}`}
        </button>
        {#if update.html_url}
          <a class="btn secondary" href={update.html_url} target="_blank" rel="noreferrer">Release notes</a>
        {/if}
      </div>
    {:else if update.html_url}
      <a class="btn secondary" href={update.html_url} target="_blank" rel="noreferrer">Open GitHub release</a>
    {/if}
    <p class="hint">github.com/{settings?.github_owner}/{settings?.github_repo}</p>
    <p class="hint">CLI: <code>curl -fsSL https://raw.githubusercontent.com/teguva/coduos/main/scripts/update.sh | sudo bash</code></p>
  {/if}
</section>

<section class="set-sec">
  <button class="set-head" onclick={() => (open.about = !open.about)}>
    <Icon name="computer" size={20} alt="" />
    <div>
      <h3>About</h3>
      <p>Version and paths. Read-only.</p>
    </div>
  </button>
  {#if open.about && settings}
    <dl class="about">
      <dt>Version</dt><dd>{settings.version}</dd>
      <dt>Hostname</dt><dd>{settings.hostname}</dd>
      <dt>Bind</dt><dd>{settings.bind}</dd>
      <dt>Data dir</dt><dd>{settings.data_dir}</dd>
    </dl>
  {/if}
</section>

{#if confirmUpdate && update?.latest}
  <Confirm
    title="Install update?"
    body={`CoduOS ${update.latest} will be downloaded and installed. The dashboard restarts; you stay signed in.`}
    confirmLabel={`Update to ${update.latest}`}
    onCancel={() => (confirmUpdate = false)}
    onConfirm={applyUpdate}
  />
{/if}
