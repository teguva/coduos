<script lang="ts">
  import type { ServiceForm } from '../lib/compose';

  let { service = $bindable() } = $props<{ service: ServiceForm }>();

  const extraKeys = $derived(Object.keys(service.extra).sort());

  function addPort() {
    service.ports = [...service.ports, { host: '', container: '', protocol: 'tcp' }];
  }
  function addVol() {
    service.volumes = [...service.volumes, { host: '', container: '' }];
  }
  function addEnv() {
    service.env = [...service.env, { key: '', value: '' }];
  }
  function addDev() {
    service.devices = [...service.devices, { host: '', container: '' }];
  }
  function drop<T>(list: T[], i: number): T[] {
    return list.filter((_, n) => n !== i);
  }
</script>

<section class="form-sec">
  <label class="field"><span>Service name</span><input bind:value={service.serviceName} placeholder="immich-server" required /></label>
  <label class="field"><span>Docker Image</span><input bind:value={service.image} placeholder="ghcr.io/immich-app/immich-server:release" required /></label>
  <label class="field"><span>Depends on</span><input bind:value={service.dependsOn} placeholder="database, redis" /></label>
  <label class="field"><span>Network</span>
    <select bind:value={service.network}>
      <option value="stack">stack (compose network)</option>
      <option value="bridge">bridge</option>
      <option value="host">host</option>
      <option value="none">none</option>
    </select>
  </label>
  {#if extraKeys.length}
    <p class="hint">Kept from YAML: {extraKeys.join(', ')}</p>
  {/if}
</section>

<section class="form-sec">
  <div class="sec-head"><h3>Ports</h3><button type="button" class="btn secondary" onclick={addPort}>Add</button></div>
  {#if service.ports.length === 0}
    <p class="hint">No ports now, click Add.</p>
  {:else}
    <div class="kv-head"><span>Host</span><span>Container</span><span>Protocol</span><span></span></div>
    {#each service.ports as p, i}
      <div class="kv-row">
        <input bind:value={p.host} placeholder="8096" />
        <input bind:value={p.container} placeholder="8096" />
        <select bind:value={p.protocol}>
          <option value="tcp">TCP</option>
          <option value="udp">UDP</option>
          <option value="">TCP + UDP</option>
        </select>
        <button type="button" class="close-x" onclick={() => (service.ports = drop(service.ports, i))}>×</button>
      </div>
    {/each}
  {/if}
</section>

<section class="form-sec">
  <div class="sec-head"><h3>Volumes</h3><button type="button" class="btn secondary" onclick={addVol}>Add</button></div>
  {#if service.volumes.length === 0}
    <p class="hint">Click Add to map a host folder into the container.</p>
  {:else}
    <div class="kv-head two"><span>Host</span><span>Container</span><span></span></div>
    {#each service.volumes as v, i}
      <div class="kv-row two">
        <input bind:value={v.host} placeholder="/DATA/Media" />
        <input bind:value={v.container} placeholder="/Media" />
        <button type="button" class="close-x" onclick={() => (service.volumes = drop(service.volumes, i))}>×</button>
      </div>
    {/each}
  {/if}
</section>

<section class="form-sec">
  <div class="sec-head"><h3>Environment Variables</h3><button type="button" class="btn secondary" onclick={addEnv}>Add</button></div>
  {#if service.env.length === 0}
    <p class="hint">Click Add to set KEY=value.</p>
  {:else}
    <div class="kv-head two"><span>Key</span><span>Value</span><span></span></div>
    {#each service.env as e, i}
      <div class="kv-row two">
        <input bind:value={e.key} placeholder="PUID" />
        <input bind:value={e.value} placeholder="1000" />
        <button type="button" class="close-x" onclick={() => (service.env = drop(service.env, i))}>×</button>
      </div>
    {/each}
  {/if}
</section>

<section class="form-sec">
  <div class="sec-head"><h3>Devices</h3><button type="button" class="btn secondary" onclick={addDev}>Add</button></div>
  {#if service.devices.length === 0}
    <p class="hint">Click Add for /dev mappings (e.g. GPU).</p>
  {:else}
    <div class="kv-head two"><span>Host</span><span>Container</span><span></span></div>
    {#each service.devices as d, i}
      <div class="kv-row two">
        <input bind:value={d.host} placeholder="/dev/dri" />
        <input bind:value={d.container} placeholder="/dev/dri" />
        <button type="button" class="close-x" onclick={() => (service.devices = drop(service.devices, i))}>×</button>
      </div>
    {/each}
  {/if}
</section>

<section class="form-sec">
  <label class="field"><span>Container command</span><input bind:value={service.command} placeholder="optional" /></label>
  <label class="toggle-row">
    <span>Privileges</span>
    <input type="checkbox" bind:checked={service.privileged} />
  </label>
  <label class="field"><span>Memory limit (MB, 0 = none)</span>
    <div class="mem">
      <input type="range" min="0" max="8192" step="128" bind:value={service.memoryMb} />
      <span>{service.memoryMb || 'none'}</span>
    </div>
  </label>
  <label class="field"><span>CPU shares</span>
    <select bind:value={service.cpuShares}>
      <option value="low">Low</option>
      <option value="medium">Medium</option>
      <option value="high">High</option>
      <option value="">Default</option>
    </select>
  </label>
  <label class="field"><span>Restart policy</span>
    <select bind:value={service.restart}>
      <option value="no">no</option>
      <option value="always">always</option>
      <option value="unless-stopped">unless-stopped</option>
      <option value="on-failure">on-failure</option>
    </select>
  </label>
  <label class="field"><span>Container capabilities (cap-add)</span><input bind:value={service.capAdd} placeholder="SYS_ADMIN, NET_ADMIN" /></label>
  <label class="field"><span>Container hostname</span><input bind:value={service.hostname} placeholder="Hostname of this container" /></label>
</section>
