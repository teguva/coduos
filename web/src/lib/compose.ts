import { parse, stringify } from 'yaml';

export type PortRow = { host: string; container: string; protocol: 'tcp' | 'udp' | '' };
export type PairRow = { host: string; container: string };
export type EnvRow = { key: string; value: string };

export type ComposeForm = {
  serviceName: string;
  image: string;
  title: string;
  iconUrl: string;
  scheme: 'http' | 'https';
  webHost: string;
  webPort: string;
  webPath: string;
  network: string;
  ports: PortRow[];
  volumes: PairRow[];
  env: EnvRow[];
  devices: PairRow[];
  command: string;
  privileged: boolean;
  memoryMb: number;
  cpuShares: 'low' | 'medium' | 'high' | '';
  restart: string;
  capAdd: string;
  hostname: string;
};

const CPU: Record<ComposeForm['cpuShares'], number | undefined> = {
  low: 10,
  medium: 50,
  high: 90,
  '': undefined
};

export function emptyForm(): ComposeForm {
  return {
    serviceName: 'app',
    image: '',
    title: '',
    iconUrl: '',
    scheme: 'http',
    webHost: '',
    webPort: '',
    webPath: '/',
    network: 'bridge',
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
    hostname: ''
  };
}

export function exampleForm(): ComposeForm {
  const f = emptyForm();
  f.serviceName = 'whoami';
  f.image = 'traefik/whoami:v1.11';
  f.title = 'whoami';
  f.webPort = '8088';
  f.ports = [{ host: '8088', container: '80', protocol: 'tcp' }];
  f.restart = 'unless-stopped';
  return f;
}

export function formToYaml(f: ComposeForm): string {
  const name = slug(f.serviceName || f.title || 'app');
  const svc: Record<string, unknown> = {
    image: f.image.trim()
  };
  if (f.network) svc.network_mode = f.network;
  if (f.restart) svc.restart = f.restart;
  if (f.privileged) svc.privileged = true;
  if (f.hostname.trim()) svc.hostname = f.hostname.trim();
  const ports = f.ports
    .filter((p) => p.host || p.container)
    .map((p) => {
      const map = `${p.host || p.container}:${p.container || p.host}`;
      return p.protocol ? `${map}/${p.protocol}` : map;
    });
  if (ports.length) svc.ports = ports;
  const volumes = f.volumes
    .filter((v) => v.host || v.container)
    .map((v) => `${v.host}:${v.container}`);
  if (volumes.length) svc.volumes = volumes;
  const env = Object.fromEntries(
    f.env.filter((e) => e.key).map((e) => [e.key, e.value])
  );
  if (Object.keys(env).length) svc.environment = env;
  const devices = f.devices
    .filter((d) => d.host || d.container)
    .map((d) => `${d.host}:${d.container}`);
  if (devices.length) svc.devices = devices;
  if (f.command.trim()) svc.command = f.command.trim();
  if (f.memoryMb > 0) svc.mem_limit = `${f.memoryMb}m`;
  const shares = CPU[f.cpuShares];
  if (shares) svc.cpu_shares = shares;
  const caps = f.capAdd
    .split(/[,\s]+/)
    .map((c) => c.trim())
    .filter(Boolean);
  if (caps.length) svc.cap_add = caps;

  const doc: Record<string, unknown> = {
    services: { [name]: svc },
    'x-coduos': {
      title: f.title || name,
      icon: f.iconUrl,
      scheme: f.scheme,
      hostname: f.webHost,
      port_map: f.webPort,
      index: f.webPath || '/'
    }
  };
  return stringify(doc, { lineWidth: 0 }).trim() + '\n';
}

export function yamlToForm(raw: string): ComposeForm {
  const f = emptyForm();
  if (!raw.trim()) return f;
  const doc = parse(raw) as any;
  if (!doc || typeof doc !== 'object') return f;
  const services = doc.services || {};
  const keys = Object.keys(services);
  if (!keys.length) return f;
  const key = keys[0];
  const svc = services[key] || {};
  f.serviceName = key;
  f.image = String(svc.image || '');
  f.network = String(svc.network_mode || 'bridge');
  f.restart = String(svc.restart || 'unless-stopped');
  f.privileged = svc.privileged === true;
  f.hostname = String(svc.hostname || '');
  f.command = Array.isArray(svc.command) ? svc.command.join(' ') : String(svc.command || '');
  f.capAdd = Array.isArray(svc.cap_add) ? svc.cap_add.join(', ') : String(svc.cap_add || '');
  f.memoryMb = parseMemoryMb(svc.mem_limit || svc.deploy?.resources?.limits?.memory);
  f.cpuShares = sharesToLabel(svc.cpu_shares);
  f.ports = asArray(svc.ports).map(parsePort);
  f.volumes = asArray(svc.volumes).map(parsePair);
  f.devices = asArray(svc.devices).map((item) =>
    typeof item === 'string' ? splitPair(item) : { host: String(item.source || item.host || ''), container: String(item.target || item.container || '') }
  );
  f.env = parseEnv(svc.environment);

  const meta = doc['x-coduos'] || doc['x-casaos'] || {};
  const title = meta.title;
  f.title =
    (typeof title === 'object' ? title.custom || title.en_us : title) || key;
  f.iconUrl = String(meta.icon || '');
  f.scheme = meta.scheme === 'https' ? 'https' : 'http';
  f.webHost = String(meta.hostname || '');
  f.webPort = String(meta.port_map || f.ports[0]?.host || '');
  f.webPath = String(meta.index || '/');
  return f;
}

function slug(name: string): string {
  const s = name
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 48);
  return s || 'app';
}

function asArray(v: unknown): any[] {
  if (v == null) return [];
  return Array.isArray(v) ? v : [v];
}

function parsePort(item: any): PortRow {
  if (typeof item === 'object' && item) {
    return {
      host: String(item.published ?? item.host ?? ''),
      container: String(item.target ?? item.container ?? ''),
      protocol: item.protocol === 'udp' ? 'udp' : item.protocol === '' ? '' : 'tcp'
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

function parsePair(item: any): PairRow {
  if (typeof item === 'object' && item) {
    return {
      host: String(item.source || item.host || ''),
      container: String(item.target || item.container || '')
    };
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

function parseMemoryMb(mem: unknown): number {
  if (mem == null || mem === '') return 0;
  if (typeof mem === 'number') return mem > 4096 ? Math.round(mem / 1024 / 1024) : mem;
  const s = String(mem);
  const n = parseFloat(s);
  if (!Number.isFinite(n)) return 0;
  if (/g/i.test(s)) return Math.round(n * 1024);
  return Math.round(n);
}

function sharesToLabel(v: unknown): ComposeForm['cpuShares'] {
  const n = Number(v);
  if (!n || n >= 99) return 'high';
  if (n <= 20) return 'low';
  if (n <= 60) return 'medium';
  return 'high';
}
