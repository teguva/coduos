export function bytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v < 10 && i > 0 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

export function uptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

export function bps(n: number): string {
  return `${bytes(n)}/s`;
}

export function pct(used: number, total: number): number {
  if (!total) return 0;
  return Math.min(100, Math.round((used / total) * 1000) / 10);
}

export function when(ts?: number | null): string {
  if (!ts) return '—';
  return new Date(ts * 1000).toLocaleString();
}

export function shortOs(os: string): string {
  let s = os.replace(/^Linux\s*\(/i, '').replace(/\)$/, '');
  s = s.replace(/\bGNU\/Linux\s+/i, '');
  return s.trim() || os;
}

export function prettyGpu(name: string): string {
  if (/intel/i.test(name) && /0x[0-9a-f]{4}/i.test(name)) return 'Intel graphics';
  if (/amd/i.test(name) && /0x[0-9a-f]{4}/i.test(name)) return 'AMD graphics';
  return name;
}

export function watts(n: number): string {
  if (!Number.isFinite(n) || n < 0) return '—';
  return n < 10 ? `${n.toFixed(1)} W` : `${Math.round(n)} W`;
}

export function joinMeta(parts: Array<string | null | undefined | false>): string {
  return parts.filter((p): p is string => Boolean(p)).join(' · ');
}
