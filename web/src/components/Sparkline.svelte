<script lang="ts">
  import { bps } from '../lib/format';

  let { rx = [], tx = [], showScale = false } = $props<{
    rx?: number[];
    tx?: number[];
    showScale?: boolean;
  }>();

  const w = 80;
  const h = 40;

  function niceMax(n: number): number {
    if (!Number.isFinite(n) || n <= 1) return 1;
    const exp = Math.floor(Math.log10(n));
    const base = 10 ** exp;
    const m = n / base;
    const nice = m <= 1 ? 1 : m <= 2 ? 2 : m <= 5 ? 5 : 10;
    return nice * base;
  }

  let max = $derived(niceMax(Math.max(1, ...rx, ...tx)));

  function pts(vals: number[]) {
    if (vals.length < 2) {
      return `0,${h} ${w},${h}`;
    }
    return vals
      .map((v, i) => {
        const x = (i / (vals.length - 1)) * w;
        const y = h - (Math.min(v, max) / max) * (h - 2) - 1;
        return `${x.toFixed(2)},${y.toFixed(2)}`;
      })
      .join(' ');
  }

  let rxPts = $derived(pts(rx));
  let txPts = $derived(pts(tx));
  let rxFill = $derived(`0,${h} ${rxPts} ${w},${h}`);
</script>

<div class="spark-wrap">
  {#if showScale}
    <span class="spark-scale">{bps(max)}</span>
  {/if}
  <svg class="spark" viewBox="0 0 {w} {h}" preserveAspectRatio="none" aria-hidden="true">
    <polygon points={rxFill} class="spark-fill" />
    <polyline points={rxPts} fill="none" class="spark-rx" />
    <polyline points={txPts} fill="none" class="spark-tx" />
  </svg>
</div>
