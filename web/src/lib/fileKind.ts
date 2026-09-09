export type PreviewKind = 'image' | 'video' | 'audio' | 'text' | 'pdf' | 'none';

const IMAGE = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'ico', 'svg', 'avif', 'jxl']);
const VIDEO = new Set(['mp4', 'webm', 'ogv', 'mov', 'm4v', 'mkv']);
const AUDIO = new Set(['mp3', 'wav', 'flac', 'ogg', 'oga', 'aac', 'm4a', 'opus']);
const PDF = new Set(['pdf']);
const TEXT = new Set([
  'txt',
  'log',
  'md',
  'markdown',
  'json',
  'yml',
  'yaml',
  'toml',
  'xml',
  'html',
  'htm',
  'css',
  'scss',
  'less',
  'js',
  'mjs',
  'cjs',
  'jsx',
  'ts',
  'tsx',
  'svelte',
  'vue',
  'rs',
  'py',
  'go',
  'java',
  'c',
  'h',
  'cpp',
  'hpp',
  'cc',
  'hh',
  'cs',
  'php',
  'rb',
  'sh',
  'bash',
  'zsh',
  'fish',
  'sql',
  'conf',
  'cfg',
  'ini',
  'env',
  'csv',
  'tsv',
  'diff',
  'patch',
  'kt',
  'swift',
  'lua',
  'r',
  'pl',
  'ps1',
  'dockerfile',
  'makefile',
  'gitignore',
  'editorconfig'
]);

export const TEXT_PREVIEW_MAX = 2 * 1024 * 1024;

export function fileExt(name: string): string {
  const base = (name.split('/').pop() || name).toLowerCase();
  if (base === 'dockerfile' || base.startsWith('dockerfile.')) return 'dockerfile';
  if (base === 'makefile' || base === 'gnumakefile') return 'makefile';
  if (base === 'cmakelists.txt') return 'txt';
  if (base === '.gitignore' || base === 'gitignore') return 'gitignore';
  const dot = base.lastIndexOf('.');
  return dot > 0 ? base.slice(dot + 1) : '';
}

export function previewKind(name: string): PreviewKind {
  const ext = fileExt(name);
  if (IMAGE.has(ext)) return 'image';
  if (VIDEO.has(ext)) return 'video';
  if (AUDIO.has(ext)) return 'audio';
  if (PDF.has(ext)) return 'pdf';
  if (TEXT.has(ext)) return 'text';
  const base = (name.split('/').pop() || name).toLowerCase();
  if (base.startsWith('.') && base.length > 1) return 'text';
  return 'none';
}

export function canPreview(name: string): boolean {
  return previewKind(name) !== 'none';
}

export function fileUrl(root: string, path: string, inline = false): string {
  const q = new URLSearchParams({ root, path });
  if (inline) q.set('inline', 'true');
  return '/api/files/download?' + q.toString();
}
