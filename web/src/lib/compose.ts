import { parse, stringify } from 'yaml';

export type PortRow = { host: string; container: string; protocol: 'tcp' | 'udp' | '' };
export type PairRow = { host: string; container: string };
export type EnvRow = { key: string; value: string };
export type CpuShares = 'low' | 'medium' | 'high' | '';

export type ServiceForm = {
  serviceName: string;
  image: string;
  network: string;
  ports: PortRow[];
  volumes: PairRow[];
  env: EnvRow[];
  envFiles: string[];
  devices: PairRow[];
  command: string;
  privileged: boolean;
  memoryMb: number;
  cpuShares: CpuShares;
  restart: string;
  capAdd: string;
  hostname: string;
  dependsOn: string;
  extra: Record<string, unknown>;
};

export type StackForm = {
  title: string;
  iconUrl: string;
  scheme: 'http' | 'https';
  webHost: string;
  webPort: string;
  webPath: string;
  services: ServiceForm[];
  /** Shared .env keys: image tags, volume hosts, and uploaded env files. */
  dotEnv: EnvRow[];
  extraDoc: Record<string, unknown>;
};

const CPU: Record<Exclude<CpuShares, ''>, number> = {
  low: 10,
  medium: 50,
  high: 90
};

const OWNED_KEYS = new Set([
  'image',
  'network_mode',
  'restart',
  'privileged',
  'hostname',
  'ports',
  'volumes',
  'environment',
  'env',
  'env_file',
  'devices',
  'command',
  'mem_limit',
  'cpu_shares',
  'cap_add'
]);

export function emptyService(name = 'app'): ServiceForm {
  return {
    serviceName: name,
    image: '',
    network: 'stack',
    ports: [],
    volumes: [],
    env: [],
    envFiles: [],
    devices: [],
    command: '',
    privileged: false,
    memoryMb: 0,
    cpuShares: 'medium',
    restart: 'unless-stopped',
    capAdd: '',
    hostname: '',
    dependsOn: '',
    extra: {}
  };
}

export function emptyStack(): StackForm {
  return {
    title: '',
    iconUrl: '',
    scheme: 'http',
    webHost: '',
    webPort: '',
    webPath: '/',
    services: [emptyService('app')],
    extraDoc: {},
    dotEnv: []
  };
}

export function exampleStack(): StackForm {
  const s = emptyService('whoami');
  s.image = 'traefik/whoami:v1.11';
  s.network = 'bridge';
  s.ports = [{ host: '8088', container: '80', protocol: 'tcp' }];
  return {
    title: 'whoami',
    iconUrl: '',
    scheme: 'http',
    webHost: '',
    webPort: '8088',
    webPath: '/',
    services: [s],
    extraDoc: {},
    dotEnv: []
  };
}

export function addService(stack: StackForm): StackForm {
  const taken = new Set(stack.services.map((s) => slug(s.serviceName)));
  let n = stack.services.length + 1;
  let name = `service-${n}`;
  while (taken.has(name)) {
    n += 1;
    name = `service-${n}`;
  }
  return { ...stack, services: [...stack.services, emptyService(name)] };
}

export function stackToYaml(stack: StackForm): string {
  const services: Record<string, unknown> = {};
  for (const s of stack.services) {
    const name = slug(s.serviceName || 'app');
    services[uniqueKey(services, name)] = serviceToObj(s);
  }
  const first = slug(stack.services[0]?.serviceName || 'app');
  const composeEnv = Object.fromEntries(
    mergeEnv(interpolationsFromStack(stack), stack.dotEnv)
      .filter((e) => e.key.trim())
      .map((e) => [e.key.trim(), e.value])
  );
  const meta: Record<string, unknown> = {
    title: stack.title || first,
    icon: stack.iconUrl,
    scheme: stack.scheme,
    hostname: stack.webHost,
    port_map: stack.webPort,
    index: stack.webPath || '/'
  };
  if (Object.keys(composeEnv).length) meta.env = composeEnv;
  const doc: Record<string, unknown> = {
    ...stack.extraDoc,
    services,
    'x-coduos': meta
  };
  return stringify(doc, { lineWidth: 0 }).trim() + '\n';
}

export function yamlToStack(raw: string): StackForm {
  const stack = emptyStack();
  if (!raw.trim()) return stack;
  const doc = parse(raw, { merge: true }) as Record<string, unknown> | null;
  if (!doc || typeof doc !== 'object') return stack;
  const services = (doc.services || {}) as Record<string, unknown>;
  const keys = Object.keys(services);
  stack.services = keys.length
    ? keys.map((key) => yamlToService(key, (services[key] || {}) as Record<string, unknown>))
    : [emptyService('app')];
  const extraDoc = { ...doc };
  delete extraDoc.services;
  delete extraDoc['x-coduos'];
  delete extraDoc['x-casaos'];
  stack.extraDoc = extraDoc;

  const meta = (doc['x-coduos'] || doc['x-casaos'] || {}) as Record<string, unknown>;
  const title = meta.title;
  const first = stack.services[0];
  const parsedTitle =
    typeof title === 'object' && title
      ? String((title as { custom?: string; en_us?: string }).custom || (title as { en_us?: string }).en_us || '')
      : String(title || '');
  const docName = typeof extraDoc.name === 'string' ? extraDoc.name : '';
  stack.title = parsedTitle || docName || first?.serviceName || '';
  stack.iconUrl = String(meta.icon || '');
  stack.scheme = meta.scheme === 'https' ? 'https' : 'http';
  stack.webHost = String(meta.hostname || '');
  stack.webPort = String(meta.port_map || first?.ports[0]?.host || '');
  stack.webPath = String(meta.index || '/');
  stack.dotEnv = mergeEnv(collectInterpolations(raw), parseEnv(meta.env));
  return stack;
}

/** @deprecated use exampleStack */
export function exampleForm(): StackForm {
  return exampleStack();
}

/** @deprecated use stackToYaml */
export function formToYaml(stack: StackForm): string {
  return stackToYaml(stack);
}

/** @deprecated use yamlToStack */
export function yamlToForm(raw: string): StackForm {
  return yamlToStack(raw);
}

function yamlToService(name: string, svc: Record<string, unknown>): ServiceForm {
  const extra: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(svc)) {
    if (OWNED_KEYS.has(k)) continue;
    if (k === 'depends_on' && (Array.isArray(v) || typeof v === 'string')) continue;
    extra[k] = v;
  }
  const simpleDepends = Array.isArray(svc.depends_on) || typeof svc.depends_on === 'string';
  const networkMode = String(svc.network_mode || '');
  return {
    serviceName: name,
    image: String(svc.image || ''),
    network: networkMode || 'stack',
    ports: asArray(svc.ports).map(parsePort),
    volumes: asArray(svc.volumes).map(parsePair),
    env: parseEnv(svc.environment ?? svc.env),
    envFiles: parseEnvFiles(svc.env_file),
    devices: asArray(svc.devices).map((item) =>
      typeof item === 'string'
        ? splitPair(item)
        : {
            host: String((item as { source?: string; host?: string }).source || (item as { host?: string }).host || ''),
            container: String(
              (item as { target?: string; container?: string }).target ||
                (item as { container?: string }).container ||
                ''
            )
          }
    ),
    command: Array.isArray(svc.command) ? svc.command.join(' ') : String(svc.command || ''),
    privileged: svc.privileged === true,
    memoryMb: parseMemoryMb(svc.mem_limit || nestedMemory(svc.deploy)),
    cpuShares: sharesToLabel(svc.cpu_shares),
    restart: String(svc.restart || 'unless-stopped'),
    capAdd: Array.isArray(svc.cap_add) ? svc.cap_add.join(', ') : String(svc.cap_add || ''),
    hostname: String(svc.hostname || ''),
    dependsOn: simpleDepends ? parseDepends(svc.depends_on) : '',
    extra
  };
}

function serviceToObj(s: ServiceForm): Record<string, unknown> {
  const svc: Record<string, unknown> = { ...s.extra };
  svc.image = s.image.trim();
  if (s.network && s.network !== 'stack') svc.network_mode = s.network;
  else delete svc.network_mode;
  if (s.restart) svc.restart = s.restart;
  if (s.privileged) svc.privileged = true;
  else delete svc.privileged;
  if (s.hostname.trim()) svc.hostname = s.hostname.trim();
  else delete svc.hostname;
  const ports = s.ports
    .filter((p) => p.host || p.container)
    .map((p) => {
      const map = `${p.host || p.container}:${p.container || p.host}`;
      return p.protocol ? `${map}/${p.protocol}` : map;
    });
  if (ports.length) svc.ports = ports;
  else delete svc.ports;
  const volumes = s.volumes.filter((v) => v.host || v.container).map((v) => `${v.host}:${v.container}`);
  if (volumes.length) svc.volumes = volumes;
  else delete svc.volumes;
  const env = Object.fromEntries(s.env.filter((e) => e.key).map((e) => [e.key, e.value]));
  if (Object.keys(env).length) svc.environment = env;
  else delete svc.environment;
  if (s.envFiles.length) svc.env_file = s.envFiles;
  else delete svc.env_file;
  const devices = s.devices.filter((d) => d.host || d.container).map((d) => `${d.host}:${d.container}`);
  if (devices.length) svc.devices = devices;
  else delete svc.devices;
  if (s.command.trim()) svc.command = s.command.trim();
  else delete svc.command;
  if (s.memoryMb > 0) svc.mem_limit = `${s.memoryMb}m`;
  else delete svc.mem_limit;
  if (s.cpuShares) svc.cpu_shares = CPU[s.cpuShares];
  else delete svc.cpu_shares;
  const caps = s.capAdd
    .split(/[,\s]+/)
    .map((c) => c.trim())
    .filter(Boolean);
  if (caps.length) svc.cap_add = caps;
  else delete svc.cap_add;
  const deps = s.dependsOn
    .split(/[,\s]+/)
    .map((d) => d.trim())
    .filter(Boolean);
  if (deps.length) svc.depends_on = deps;
  else if (!('depends_on' in s.extra)) delete svc.depends_on;
  return svc;
}

const INTERP_BRACED = /\$\{([A-Za-z_][A-Za-z0-9_]*)(?::-([^}]*))?\}/g;
const INTERP_BARE = /(?<!\$)\$([A-Za-z_][A-Za-z0-9_]*)/g;

/** `${VAR}` / `${VAR:-default}` / `$VAR` used in images, volume hosts, and env values. */
export function collectInterpolations(text: string): EnvRow[] {
  const seen = new Map<string, string>();
  INTERP_BRACED.lastIndex = 0;
  for (const m of text.matchAll(INTERP_BRACED)) {
    const key = m[1];
    const def = m[2] ?? '';
    if (!seen.has(key)) seen.set(key, def);
    else if (!seen.get(key) && def) seen.set(key, def);
  }
  INTERP_BARE.lastIndex = 0;
  for (const m of text.matchAll(INTERP_BARE)) {
    const key = m[1];
    if (!seen.has(key)) seen.set(key, '');
  }
  return [...seen.entries()].map(([key, value]) => ({ key, value }));
}

export function interpolationsFromStack(stack: StackForm): EnvRow[] {
  const parts: string[] = [];
  for (const s of stack.services) {
    parts.push(s.image, s.command, s.hostname, s.dependsOn, s.capAdd, ...s.envFiles);
    for (const v of s.volumes) parts.push(v.host, v.container);
    for (const p of s.ports) parts.push(p.host, p.container);
    for (const d of s.devices) parts.push(d.host, d.container);
    for (const e of s.env) parts.push(e.value);
    if (Object.keys(s.extra).length) parts.push(JSON.stringify(s.extra));
  }
  if (Object.keys(stack.extraDoc).length) parts.push(JSON.stringify(stack.extraDoc));
  return collectInterpolations(parts.join('\n'));
}

/** Immich-style `.env`: comments, blanks, optional `export`, quoted values. */
export function parseDotEnv(text: string): EnvRow[] {
  const out: EnvRow[] = [];
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith('#')) continue;
    const body = line.replace(/^export\s+/, '');
    const i = body.indexOf('=');
    if (i < 0) continue;
    const key = body.slice(0, i).trim();
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) continue;
    let value = body.slice(i + 1).trim();
    if (
      (value.startsWith('"') && value.endsWith('"') && value.length >= 2) ||
      (value.startsWith("'") && value.endsWith("'") && value.length >= 2)
    ) {
      value = value.slice(1, -1);
    }
    out.push({ key, value });
  }
  return out;
}

/** Later lists win. First-seen key order is kept. Empty keys are skipped. */
export function mergeEnv(...lists: EnvRow[][]): EnvRow[] {
  const map = new Map<string, string>();
  const order: string[] = [];
  for (const list of lists) {
    for (const { key, value } of list) {
      const k = key.trim();
      if (!k) continue;
      if (!map.has(k)) order.push(k);
      map.set(k, value);
    }
  }
  return order.map((key) => ({ key, value: map.get(key) ?? '' }));
}

/** Whole-value pointer like `${DB_PASSWORD}` — hide it when that key is already shared. */
export function interpolationPointer(value: string): string | null {
  const s = value.trim();
  const braced = s.match(/^\$\{([A-Za-z_][A-Za-z0-9_]*)(?::-([^}]*)?)?\}$/);
  if (braced) return braced[1];
  const bare = s.match(/^\$([A-Za-z_][A-Za-z0-9_]*)$/);
  return bare ? bare[1] : null;
}

/** Container env that is compose plumbing, not a .env knob (`--data-checksums`). */
export function isComposeLiteralEnv(value: string): boolean {
  const s = value.trim().replace(/^['"]|['"]$/g, '');
  return s.startsWith('-');
}

export function showServiceEnv(e: EnvRow, sharedKeys: Set<string>): boolean {
  if (!e.key.trim()) return true;
  const p = interpolationPointer(e.value);
  if (p && sharedKeys.has(p)) return false;
  if (isComposeLiteralEnv(e.value)) return false;
  return true;
}

export function stackEnvCount(stack: StackForm): number {
  const shared = mergeEnv(interpolationsFromStack(stack), stack.dotEnv).filter((e) => e.key.trim());
  const keys = new Set(shared.map((e) => e.key));
  let n = shared.length;
  for (const s of stack.services) {
    for (const e of s.env) {
      if (!e.key.trim()) continue;
      if (!showServiceEnv(e, keys)) continue;
      n += 1;
    }
  }
  return n;
}

function uniqueKey(map: Record<string, unknown>, name: string): string {
  if (!map[name]) return name;
  let n = 2;
  while (map[`${name}-${n}`]) n += 1;
  return `${name}-${n}`;
}

export function slug(name: string): string {
  const s = name
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 48);
  return s || 'app';
}

function asArray(v: unknown): unknown[] {
  if (v == null) return [];
  return Array.isArray(v) ? v : [v];
}

function parseDepends(raw: unknown): string {
  if (!raw) return '';
  if (Array.isArray(raw)) return raw.map(String).join(', ');
  if (typeof raw === 'object') return Object.keys(raw as object).join(', ');
  return String(raw);
}

function parsePort(item: unknown): PortRow {
  if (typeof item === 'object' && item) {
    const o = item as { published?: unknown; host?: unknown; target?: unknown; container?: unknown; protocol?: unknown };
    return {
      host: String(o.published ?? o.host ?? ''),
      container: String(o.target ?? o.container ?? ''),
      protocol: o.protocol === 'udp' ? 'udp' : o.protocol === '' ? '' : 'tcp'
    };
  }
  const s = String(item);
  const proto = s.includes('/udp') ? 'udp' : s.includes('/tcp') ? 'tcp' : 'tcp';
  const core = s.replace(/\/(tcp|udp)$/i, '');
  const parts = core.split(':');
  if (parts.length === 1) return { host: parts[0], container: parts[0], protocol: proto };
  if (parts.length === 2) return { host: parts[0], container: parts[1], protocol: proto };
  return { host: parts[parts.length - 2], container: parts[parts.length - 1], protocol: proto };
}

function parsePair(item: unknown): PairRow {
  if (typeof item === 'object' && item) {
    const o = item as { source?: string; host?: string; target?: string; container?: string };
    return { host: String(o.source || o.host || ''), container: String(o.target || o.container || '') };
  }
  return splitPair(String(item));
}

function splitPair(s: string): PairRow {
  const i = s.indexOf(':');
  if (i < 0) return { host: s, container: s };
  return { host: s.slice(0, i), container: s.slice(i + 1) };
}

function parseEnv(env: unknown): EnvRow[] {
  if (env == null || env === '') return [];
  if (typeof env === 'string') return [parseEnvLine(env)];
  if (env instanceof Map) {
    return [...env.entries()].flatMap(([key, value]) => parseEnvEntry(String(key), value));
  }
  if (Array.isArray(env)) {
    return env.flatMap((item) => parseEnvItem(item));
  }
  if (typeof env === 'object') {
    return Object.entries(env as Record<string, unknown>).flatMap(([key, value]) => parseEnvEntry(key, value));
  }
  return [];
}

function parseEnvItem(item: unknown): EnvRow[] {
  if (item == null) return [];
  if (typeof item === 'string') return [parseEnvLine(item)];
  if (Array.isArray(item) && item.length) {
    return [{ key: String(item[0] ?? ''), value: envScalar(item[1]) }];
  }
  if (typeof item === 'object') {
    const o = item as Record<string, unknown>;
    const key = String(o.key ?? o.name ?? o.KEY ?? o.Name ?? '');
    if (key && key !== '[object Object]') {
      return [{ key, value: envScalar(o.value ?? o.val ?? o.VALUE ?? '') }];
    }
    return Object.entries(o).flatMap(([k, v]) => parseEnvEntry(k, v));
  }
  return [{ key: String(item), value: '' }];
}

function parseEnvLine(s: string): EnvRow {
  const i = s.indexOf('=');
  return i < 0 ? { key: s.trim(), value: '' } : { key: s.slice(0, i).trim(), value: s.slice(i + 1) };
}

function parseEnvEntry(key: string, value: unknown): EnvRow[] {
  if (key === '<<') {
    if (value && typeof value === 'object' && !Array.isArray(value)) {
      return parseEnv(value);
    }
    return [];
  }
  return [{ key, value: envScalar(value) }];
}

function envScalar(v: unknown): string {
  if (v == null) return '';
  if (typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean') return String(v);
  if (typeof v === 'object') {
    const o = v as Record<string, unknown>;
    if ('value' in o) return envScalar(o.value);
    if ('val' in o) return envScalar(o.val);
  }
  return '';
}

function parseEnvFiles(v: unknown): string[] {
  return asArray(v)
    .map((item) => {
      if (typeof item === 'string') return item;
      if (item && typeof item === 'object') {
        const o = item as { path?: unknown; source?: unknown };
        return String(o.path ?? o.source ?? '');
      }
      return '';
    })
    .filter(Boolean);
}

function nestedMemory(deploy: unknown): unknown {
  if (!deploy || typeof deploy !== 'object') return undefined;
  const limits = (deploy as { resources?: { limits?: { memory?: unknown } } }).resources?.limits;
  return limits?.memory;
}

function parseMemoryMb(mem: unknown): number {
  if (mem == null || mem === '') return 0;
  if (typeof mem === 'number') return mem > 4096 ? Math.round(mem / 1024 / 1024) : mem;
  const s = String(mem);
  const n = parseFloat(s);
  if (!Number.isFinite(n)) return 0;
  if (/g/i.test(s)) return Math.round(n * 1024);
  return Math.round(n);
}

function sharesToLabel(v: unknown): CpuShares {
  if (v == null || v === '') return '';
  const n = Number(v);
  if (!Number.isFinite(n)) return '';
  if (n <= 20) return 'low';
  if (n <= 60) return 'medium';
  return 'high';
}
