<script lang="ts">
  import type { ComposeForm } from '../lib/compose';

  let { form = $bindable() } = $props<{ form: ComposeForm }>();

  function addPort() {
    form.ports = [...form.ports, { host: '', container: '', protocol: 'tcp' }];
  }
  function addVol() {
    form.volumes = [...form.volumes, { host: '', container: '' }];
  }
  function addEnv() {
    form.env = [...form.env, { key: '', value: '' }];
  }
  function addDev() {
    form.devices = [...form.devices, { host: '', container: '' }];
  }
  function drop<T>(list: T[], i: number): T[] {
    return list.filter((_, n) => n !== i);
  }
</script>

<section class="form-sec">
  <label class="field"><span>Docker Image</span><input bind:value={form.image} placeholder="linuxserver/jellyfin:latest" required /></label>
  <label class="field"><span>Title</span><input bind:value={form.title} placeholder="Jellyfin" required /></label>
  <label class="field"><span>Icon URL</span>
    <div class="icon-row">
      {#if form.iconUrl}
        <img class="app-icon" src={form.iconUrl} alt="" />
      {:else}
        <img class="app-icon" src="/icons/docker.svg" alt="" />
      {/if}
      <input bind:value={form.iconUrl} placeholder="https://…" />
    </div>
  </label>
  <div class="field">
    <span>Web UI</span>
    <div class="webui">
      <select bind:value={form.scheme}>
        <option value="http">http://</option>
        <option value="https">https://</option>
      </select>
      <input bind:value={form.webHost} placeholder={typeof location !== 'undefined' ? location.hostname : 'host'} />
      <input bind:value={form.webPort} placeholder="8096" />
      <input bind:value={form.webPath} placeholder="/" />
    </div>
  </div>
  <label class="field"><span>Network</span>
    <select bind:value={form.network}>
      <option value="bridge">bridge</option>
      <option value="host">host</option>
      <option value="none">none</option>
    </select>
  </label>
</section>

<section class="form-sec">
  <div class="sec-head"><h3>Ports</h3><button type="button" class="btn secondary" onclick={addPort}>Add</button></div>
  {#if form.ports.length === 0}
    <p class="hint">No ports now, click Add.</p>
  {:else}
    <div class="kv-head"><span>Host</span><span>Container</span><span>Protocol</span><span></span></div>
    {#each form.ports as p, i}
      <div class="kv-row">
        <input bind:value={p.host} placeholder="8096" />
        <input bind:value={p.container} placeholder="8096" />
        <select bind:value={p.protocol}>
          <option value="tcp">TCP</option>
          <option value="udp">UDP</option>
          <option value="">TCP + UDP</option>
        </select>
        <button type="button" class="close-x" onclick={() => (form.ports = drop(form.ports, i))}>×</button>
      </div>
    {/each}
  {/if}
</section>

<section class="form-sec">
  <div class="sec-head"><h3>Volumes</h3><button type="button" class="btn secondary" onclick={addVol}>Add</button></div>
  {#if form.volumes.length === 0}
    <p class="hint">Click Add to map a host folder into the container.</p>
  {:else}
    <div class="kv-head two"><span>Host</span><span>Container</span><span></span></div>
    {#each form.volumes as v, i}
      <div class="kv-row two">
        <input bind:value={v.host} placeholder="/DATA/Media" />
        <input bind:value={v.container} placeholder="/Media" />
        <button type="button" class="close-x" onclick={() => (form.volumes = drop(form.volumes, i))}>×</button>
      </div>
    {/each}
  {/if}
</section>

<section class="form-sec">
  <div class="sec-head"><h3>Environment Variables</h3><button type="button" class="btn secondary" onclick={addEnv}>Add</button></div>
  {#if form.env.length === 0}
    <p class="hint">Click Add to set KEY=value.</p>
  {:else}
    <div class="kv-head two"><span>Key</span><span>Value</span><span></span></div>
    {#each form.env as e, i}
      <div class="kv-row two">
        <input bind:value={e.key} placeholder="PUID" />
        <input bind:value={e.value} placeholder="1000" />
        <button type="button" class="close-x" onclick={() => (form.env = drop(form.env, i))}>×</button>
      </div>
    {/each}
  {/if}
</section>

<section class="form-sec">
  <div class="sec-head"><h3>Devices</h3><button type="button" class="btn secondary" onclick={addDev}>Add</button></div>
  {#if form.devices.length === 0}
    <p class="hint">Click Add for /dev mappings (e.g. GPU).</p>
  {:else}
    <div class="kv-head two"><span>Host</span><span>Container</span><span></span></div>
    {#each form.devices as d, i}
      <div class="kv-row two">
        <input bind:value={d.host} placeholder="/dev/dri" />
        <input bind:value={d.container} placeholder="/dev/dri" />
        <button type="button" class="close-x" onclick={() => (form.devices = drop(form.devices, i))}>×</button>
      </div>
    {/each}
  {/if}
</section>

<section class="form-sec">
  <label class="field"><span>Container command</span><input bind:value={form.command} placeholder="optional" /></label>
  <label class="toggle-row">
    <span>Privileges</span>
    <input type="checkbox" bind:checked={form.privileged} />
  </label>
  <label class="field"><span>Memory limit (MB, 0 = none)</span>
    <div class="mem">
      <input type="range" min="0" max="8192" step="128" bind:value={form.memoryMb} />
      <span>{form.memoryMb || 'none'}</span>
    </div>
  </label>
  <label class="field"><span>CPU shares</span>
    <select bind:value={form.cpuShares}>
      <option value="low">Low</option>
      <option value="medium">Medium</option>
      <option value="high">High</option>
      <option value="">Default</option>
    </select>
  </label>
  <label class="field"><span>Restart policy</span>
    <select bind:value={form.restart}>
      <option value="no">no</option>
      <option value="always">always</option>
      <option value="unless-stopped">unless-stopped</option>
      <option value="on-failure">on-failure</option>
    </select>
  </label>
  <label class="field"><span>Container capabilities (cap-add)</span><input bind:value={form.capAdd} placeholder="SYS_ADMIN, NET_ADMIN" /></label>
  <label class="field"><span>Container hostname</span><input bind:value={form.hostname} placeholder="Hostname of app container" /></label>
</section>
