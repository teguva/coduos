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
    extraDoc: {}
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
    extraDoc: {}
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
  const doc: Record<string, unknown> = {
    ...stack.extraDoc,
    services,
    'x-coduos': {
      title: stack.title || first,
      icon: stack.iconUrl,
      scheme: stack.scheme,
      hostname: stack.webHost,
      port_map: stack.webPort,
      index: stack.webPath || '/'
    }
  };
  return stringify(doc, { lineWidth: 0 }).trim() + '\n';
}

export function yamlToStack(raw: string): StackForm {
  const stack = emptyStack();
  if (!raw.trim()) return stack;
  const doc = parse(raw) as Record<string, unknown> | null;
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
    env: parseEnv(svc.environment),
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
  if (!env) return [];
  if (Array.isArray(env)) {
    return env.map((item) => {
      if (typeof item === 'string') {
        const i = item.indexOf('=');
        return i < 0 ? { key: item, value: '' } : { key: item.slice(0, i), value: item.slice(i + 1) };
      }
      const [k, v] = Array.isArray(item) ? item : [String(item), ''];
      return { key: String(k), value: String(v ?? '') };
    });
  }
  if (typeof env === 'object') {
    return Object.entries(env as Record<string, string>).map(([key, value]) => ({
      key,
      value: String(value ?? '')
    }));
  }
  return [];
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
