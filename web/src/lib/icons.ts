import { writable } from 'svelte/store';

/** Bundled + overlay icons, served from /icons/ */
export const ICON_ROOT = '/icons';

/** Bumps when the on-disk catalog is reloaded so views re-pick names. */
export const iconRev = writable(0);

const files = new Map<string, string>();

export const appIcons = {
  files: 'files',
  apps: 'apps',
  services: 'services',
  settings: 'settings',
  install: 'install',
  docker: 'docker',
  computer: 'computer',
  storage: 'disk',
  tasks: 'cpu',
  vpn: 'folder-locked',
  proxy: 'server',
  network: 'laptop',
  battery: 'battery'
} as const;

const FOLDER_ALIAS: Record<string, string> = {
  documents: 'folder-documents',
  docs: 'folder-documents',
  document: 'folder-documents',
  downloads: 'folder-download',
  download: 'folder-download',
  pictures: 'folder-images',
  photos: 'folder-images',
  images: 'folder-images',
  img: 'folder-images',
  music: 'folder-music',
  audio: 'folder-music',
  videos: 'folder-videos',
  video: 'folder-videos',
  movies: 'folder-videos',
  code: 'folder-code',
  src: 'folder-code',
  source: 'folder-code',
  git: 'folder-git',
  docker: 'folder-docker',
  public: 'folder-public',
  templates: 'folder-templates',
  template: 'folder-templates',
  tmp: 'folder-temp',
  temp: 'folder-temp',
  projects: 'folder-projects',
  project: 'folder-projects',
  development: 'folder-development',
  dev: 'folder-development',
  cloud: 'folder-cloud',
  notes: 'folder-notes',
  log: 'folder-log',
  logs: 'folder-log',
  trash: 'folder-trash',
  desktop: 'folder-desktop',
  home: 'folder-home'
};

const KIND: Record<string, string> = {
  txt: 'text',
  log: 'text',
  conf: 'text',
  cfg: 'text',
  ini: 'text',
  md: 'markdown',
  markdown: 'markdown',
  html: 'html',
  htm: 'html',
  xml: 'xml',
  css: 'css',
  scss: 'css',
  less: 'css',
  json: 'json',
  yml: 'yaml',
  yaml: 'yaml',
  toml: 'yaml',
  pdf: 'pdf',
  zip: 'zip',
  rar: 'archive',
  '7z': 'archive',
  tar: 'tar',
  tgz: 'gzip',
  gz: 'gzip',
  bz2: 'archive',
  xz: 'archive',
  bin: 'executable',
  exe: 'executable',
  so: 'executable',
  sh: 'script',
  bash: 'script',
  zsh: 'script',
  png: 'image',
  jpg: 'image',
  jpeg: 'image',
  gif: 'image',
  webp: 'image',
  bmp: 'image',
  ico: 'image',
  svg: 'image-svg',
  mp3: 'audio',
  wav: 'audio',
  flac: 'audio',
  ogg: 'audio',
  aac: 'audio',
  m4a: 'audio',
  mp4: 'video',
  mkv: 'video',
  webm: 'video',
  avi: 'video',
  mov: 'video',
  ttf: 'font',
  otf: 'font',
  woff: 'font',
  woff2: 'font',
  deb: 'package',
  rpm: 'package',
  apk: 'package',
  doc: 'document',
  docx: 'document',
  odt: 'document',
  xls: 'spreadsheet',
  xlsx: 'spreadsheet',
  ods: 'spreadsheet',
  ppt: 'presentation',
  pptx: 'presentation',
  odp: 'presentation',
  py: 'python',
  rs: 'rust',
  js: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  jsx: 'javascript',
  ts: 'typescript',
  tsx: 'typescript',
  go: 'go',
  java: 'java',
  php: 'php',
  rb: 'ruby',
  sql: 'sql',
  patch: 'patch',
  diff: 'patch'
};

const BUNDLED = new Set([
  'add',
  'apps',
  'archive',
  'audio',
  'battery',
  'close',
  'cmake',
  'computer',
  'cpu',
  'css',
  'delete',
  'disk',
  'disk-root',
  'dockerfile',
  'docker',
  'document',
  'download',
  'executable',
  'files',
  'folder',
  'folder-cloud',
  'folder-code',
  'folder-desktop',
  'folder-development',
  'folder-docker',
  'folder-documents',
  'folder-download',
  'folder-git',
  'folder-home',
  'folder-images',
  'folder-locked',
  'folder-log',
  'folder-music',
  'folder-new',
  'folder-notes',
  'folder-open',
  'folder-projects',
  'folder-public',
  'folder-templates',
  'folder-temp',
  'folder-trash',
  'folder-videos',
  'font',
  'go',
  'go-home',
  'go-up',
  'gzip',
  'html',
  'image',
  'image-png',
  'image-svg',
  'install',
  'java',
  'javascript',
  'json',
  'laptop',
  'makefile',
  'markdown',
  'memory',
  'open',
  'package',
  'patch',
  'pdf',
  'php',
  'presentation',
  'python',
  'ruby',
  'rust',
  'script',
  'server',
  'services',
  'settings',
  'spreadsheet',
  'sql',
  'symlink',
  'tar',
  'terminal',
  'text',
  'typescript',
  'unknown',
  'upload',
  'video',
  'view-grid',
  'view-list',
  'xml',
  'yaml',
  'zip'
]);

export function hasIcon(name: string): boolean {
  const key = name.toLowerCase();
  if (files.size > 0) return files.has(key);
  return BUNDLED.has(key);
}

export function iconSrc(rel: string): string {
  const key = rel.toLowerCase();
  const file = files.get(key) || `${rel}.svg`;
  return `${ICON_ROOT}/${file}`;
}

export function resolveIcon(nameOrUrl?: string | null): string {
  if (!nameOrUrl) return iconSrc('unknown');
  if (/^https?:\/\//i.test(nameOrUrl) || nameOrUrl.startsWith('data:')) return nameOrUrl;
  if (nameOrUrl.startsWith('/')) return nameOrUrl;
  return iconSrc(nameOrUrl);
}

export function pickIcon(...candidates: (string | null | undefined)[]): string {
  for (const c of candidates) {
    if (!c) continue;
    if (/^https?:\/\//i.test(c) || c.startsWith('data:') || c.startsWith('/')) return c;
    if (hasIcon(c)) return c;
  }
  return 'unknown';
}

export function appIcon(app: { id: string; icon_url?: string | null }): string {
  return pickIcon(app.icon_url, app.id, appIcons.docker);
}

function stem(name: string): string {
  const base = name.split('/').pop() || name;
  return base.toLowerCase();
}

export function folderIcon(name: string): string {
  const key = stem(name).replace(/^\./, '');
  return pickIcon(`folder-${key}`, FOLDER_ALIAS[key], 'folder');
}

export function locationIcon(id: string, label: string): string {
  const keys = [id, label, stem(label)].map((s) => s.toLowerCase());
  for (const key of keys) {
    const hit = pickIcon(`folder-${key}`, FOLDER_ALIAS[key]);
    if (hit !== 'unknown') return hit;
    if (key.includes('home')) return pickIcon('folder-home', 'folder');
    if (key.includes('media') || key.includes('disk') || key.includes('drive')) {
      return pickIcon('disk', 'folder');
    }
    if (key.includes('server')) return pickIcon('server', 'folder');
    if (key.includes('root')) return pickIcon('disk-root', 'disk', 'folder');
  }
  return pickIcon('disk', 'folder');
}

export function fileIcon(name: string, dir: boolean): string {
  if (dir) return folderIcon(name);
  const base = stem(name);
  if (base === 'dockerfile' || base.startsWith('dockerfile.')) {
    return pickIcon('dockerfile', 'script');
  }
  if (base === 'makefile' || base === 'gnumakefile') return pickIcon('makefile', 'text');
  if (base === 'cmakelists.txt') return pickIcon('cmake', 'text');
  if (
    base.endsWith('.compose.yml') ||
    base.endsWith('.compose.yaml') ||
    base === 'compose.yml' ||
    base === 'compose.yaml'
  ) {
    return pickIcon('folder-docker', 'docker', 'yaml');
  }
  const dot = base.lastIndexOf('.');
  const ext = dot > 0 ? base.slice(dot + 1) : '';
  if (!ext) return pickIcon('unknown');
  return pickIcon(ext, KIND[ext], 'unknown');
}

let indexPromise: Promise<void> | null = null;

export function refreshIconIndex(): Promise<void> {
  indexPromise = (async () => {
    try {
      const res = await fetch('/api/icons');
      if (!res.ok) return;
      const data = (await res.json()) as { files?: string[] };
      files.clear();
      for (const file of data.files || []) {
        const base = file.split('/').pop() || file;
        const dot = base.lastIndexOf('.');
        if (dot <= 0) continue;
        const stemName = base.slice(0, dot).toLowerCase();
        const ext = base.slice(dot + 1).toLowerCase();
        if (!['svg', 'png', 'webp', 'jpg', 'jpeg', 'gif', 'ico'].includes(ext)) continue;
        const prev = files.get(stemName);
        if (prev?.endsWith('.svg') && ext !== 'svg') continue;
        files.set(stemName, base);
      }
    } catch {
      /* bundled /icons/{name}.svg still works */
    } finally {
      iconRev.update((n) => n + 1);
    }
  })();
  return indexPromise;
}

export function ensureIconIndex(): Promise<void> {
  return indexPromise ?? refreshIconIndex();
}
