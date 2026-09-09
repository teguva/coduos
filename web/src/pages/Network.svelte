<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { bps } from '../lib/format';
  import Sparkline from '../components/Sparkline.svelte';
  import UiIcon from '../components/UiIcon.svelte';

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
  let hist = $state<Record<string, { rx: number; tx: number }[]>>({});
  let showVirtual = $state(false);
  let list = $derived(showVirtual ? networks : networks.filter((n) => !n.virtual_iface));

  function record(nets: Net[]) {
    networks = nets;
    const next = { ...hist };
    for (const n of nets) {
      const row = [...(next[n.name] ?? []), { rx: n.rx_bps, tx: n.tx_bps }].slice(-90);
      next[n.name] = row;
    }
    hist = next;
  }

  onMount(() => {
    const es = new EventSource('/api/system/summary/stream', { withCredentials: true });
    es.onmessage = (ev) => {
      try {
        record(JSON.parse(ev.data).networks || []);
      } catch {
        /* ignore */
      }
    };
    api<{ networks: Net[] }>('/api/system/summary')
      .then((s) => record(s.networks || []))
      .catch(() => {});
    return () => es.close();
  });
</script>

<label class="toggle-row">
  <span>Show virtual</span>
  <input type="checkbox" bind:checked={showVirtual} />
</label>

{#each list as n}
  <div class="mini-card net-card">
    <div>
      <strong>{n.name}</strong>
      <div class="meta">
        {n.operstate || 'unknown'}{#if n.ipv4} · {n.ipv4}{/if}{#if n.speed_mbps} · {n.speed_mbps >= 1000 ? n.speed_mbps / 1000 + ' Gb/s' : n.speed_mbps + ' Mb/s'}{/if}
      </div>
      <div class="net-rates">
        <span class="net-rx"><UiIcon name="arrow_downward" size={16} /> {bps(n.rx_bps)}</span>
        <span class="net-tx"><UiIcon name="arrow_upward" size={16} /> {bps(n.tx_bps)}</span>
      </div>
    </div>
    {#if (hist[n.name] ?? []).length > 1}
      <Sparkline rx={(hist[n.name] ?? []).map((s) => s.rx)} tx={(hist[n.name] ?? []).map((s) => s.tx)} showScale />
    {/if}
  </div>
{:else}
  <p class="hint">No interfaces to show.</p>
{/each}
