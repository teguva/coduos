<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { bps } from '../lib/format';

  type Net = {
    name: string;
    rx_bps: number;
    tx_bps: number;
    ipv4?: string | null;
    operstate: string;
    speed_mbps?: number | null;
    virtual_iface: boolean;
  };

  let networks = $state<Net[]>([]);
  let showVirtual = $state(false);
  let list = $derived(showVirtual ? networks : networks.filter((n) => !n.virtual_iface));

  onMount(() => {
    const es = new EventSource('/api/system/summary/stream', { withCredentials: true });
    es.onmessage = (ev) => {
      try {
        networks = JSON.parse(ev.data).networks || [];
      } catch {
        /* ignore */
      }
    };
    api<{ networks: Net[] }>('/api/system/summary')
      .then((s) => (networks = s.networks || []))
      .catch(() => {});
    return () => es.close();
  });
</script>

<label class="toggle-row">
  <span>Show virtual</span>
  <input type="checkbox" bind:checked={showVirtual} />
</label>

{#each list as n}
  <div class="mini-card">
    <div>
      <strong>{n.name}</strong>
      <div class="meta">
        {n.operstate || 'unknown'}{#if n.ipv4} · {n.ipv4}{/if}{#if n.speed_mbps} · {n.speed_mbps} Mb/s{/if}
      </div>
      <div class="meta">↓ {bps(n.rx_bps)} · ↑ {bps(n.tx_bps)}</div>
    </div>
  </div>
{:else}
  <p class="hint">No interfaces to show.</p>
{/each}
