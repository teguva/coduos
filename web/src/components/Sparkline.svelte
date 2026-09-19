<script lang="ts">
  import { bps } from '../lib/format';

  let { rx = [], tx = [], showScale = false } = $props<{
    rx?: number[];
    tx?: number[];
    showScale?: boolean;
  }>();

  const w = 80;
  const h = 40;
  const tension = 0.3;

  function niceMax(n: number): number {
    if (!Number.isFinite(n) || n <= 1) return 1;
    const exp = Math.floor(Math.log10(n));
    const base = 10 ** exp;
    const m = n / base;
    const nice = m <= 1 ? 1 : m <= 2 ? 2 : m <= 5 ? 5 : 10;
    return nice * base;
  }

  let max = $derived(niceMax(Math.max(1, ...rx, ...tx)));

  function soften(vals: number[]): number[] {
    if (vals.length < 3) return vals;
    return vals.map((v, i) => {
      if (i === 0 || i === vals.length - 1) return v;
      return vals[i - 1] * 0.18 + v * 0.64 + vals[i + 1] * 0.18;
    });
  }

  function coords(vals: number[]): { x: number; y: number }[] {
    const series = soften(vals);
    if (series.length < 2) {
      return [
        { x: 0, y: h },
        { x: w, y: h },
      ];
    }
    return series.map((v, i) => ({
      x: (i / (series.length - 1)) * w,
      y: h - (Math.min(v, max) / max) * (h - 2) - 1,
    }));
  }

  function curve(pts: { x: number; y: number }[]): string {
    const start = `M ${pts[0].x.toFixed(2)} ${pts[0].y.toFixed(2)}`;
    if (pts.length < 2) return `${start} L ${w} ${h}`;
    let d = start;
    for (let i = 0; i < pts.length - 1; i++) {
      const p = pts[i];
      const q = pts[i + 1];
      const dx = (q.x - p.x) * tension;
      d += ` C ${(p.x + dx).toFixed(2)} ${p.y.toFixed(2)}, ${(q.x - dx).toFixed(2)} ${q.y.toFixed(2)}, ${q.x.toFixed(2)} ${q.y.toFixed(2)}`;
    }
    return d;
  }

  let rxPts = $derived(coords(rx));
  let txPts = $derived(coords(tx));
  let rxLine = $derived(curve(rxPts));
  let txLine = $derived(curve(txPts));
  let rxFill = $derived(`${rxLine} L ${w} ${h} L 0 ${h} Z`);
</script>

<div class="spark-wrap">
  {#if showScale}
    <span class="spark-scale">{bps(max)}</span>
  {/if}
  <svg class="spark" viewBox="0 0 {w} {h}" preserveAspectRatio="none" aria-hidden="true">
    <path d={rxFill} class="spark-fill" />
    <path d={rxLine} fill="none" class="spark-rx" vector-effect="non-scaling-stroke" />
    {#if tx.length > 0}
      <path d={txLine} fill="none" class="spark-tx" vector-effect="non-scaling-stroke" />
    {/if}
  </svg>
</div>
