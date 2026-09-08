<script lang="ts">
  let { rx = [], tx = [] } = $props<{ rx?: number[]; tx?: number[] }>();

  const w = 80;
  const h = 40;

  let max = $derived(Math.max(1, ...rx, ...tx));

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

<svg class="spark" viewBox="0 0 {w} {h}" preserveAspectRatio="none" aria-hidden="true">
  <polygon points={rxFill} class="spark-fill" />
  <polyline points={rxPts} fill="none" class="spark-rx" />
  <polyline points={txPts} fill="none" class="spark-tx" />
</svg>
