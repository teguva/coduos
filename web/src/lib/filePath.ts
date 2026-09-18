export type FileRoot = { id: string; label: string; path: string };
export type FileFav = { root: string; path: string; label: string };

export type BrowsePath = (start: string, pick: (path: string) => void) => void;

/** Absolute host path for a Files root + relative path. */
export function joinHostPath(rootPath: string, rel: string): string {
  const base = rootPath.replace(/\/+$/, '');
  const rest = rel.replace(/^\/+/, '').replace(/\/+$/, '');
  return rest ? `${base}/${rest}` : base;
}

/** Map an absolute path back to a Files location, preferring the longest matching root. */
export function splitHostPath(abs: string, roots: FileRoot[]): { root: FileRoot; rel: string } | null {
  const n = abs.trim().replace(/\/+$/, '');
  if (!n.startsWith('/')) return null;
  let best: FileRoot | null = null;
  for (const r of roots) {
    const rp = r.path.replace(/\/+$/, '');
    if (!rp) continue;
    if (n === rp || n.startsWith(`${rp}/`)) {
      if (!best || rp.length > best.path.replace(/\/+$/, '').length) best = r;
    }
  }
  if (!best) return null;
  const rp = best.path.replace(/\/+$/, '');
  return { root: best, rel: n === rp ? '' : n.slice(rp.length + 1) };
}

export function parentRel(rel: string): string {
  const parts = rel.split('/').filter(Boolean);
  parts.pop();
  return parts.join('/');
}
