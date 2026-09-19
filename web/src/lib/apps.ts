export type AppPhase =
  | 'not_installed'
  | 'installing'
  | 'starting'
  | 'running'
  | 'stopped'
  | 'updating'
  | 'error';

export type AppStatus = {
  running: boolean;
  installed: boolean;
  phase: AppPhase;
  percent?: number | null;
  message?: string | null;
  error?: string | null;
  containers?: { name: string; state: string; status: string }[];
};

export type AppJob = {
  id: string;
  phase: AppPhase;
  percent?: number | null;
  message?: string | null;
  error?: string | null;
};

export type AppRecord = {
  id: string;
  name: string;
  compose_yaml?: string;
  icon_url?: string | null;
  web_port?: number | null;
  created_at?: string;
  app_dir?: string;
  status: AppStatus;
  image_update?: { available: boolean; images?: string[] } | null;
};

export function overlayJob(status: AppStatus, job?: AppJob | null): AppStatus {
  if (!job) return status;
  return {
    ...status,
    phase: job.phase,
    percent: job.percent ?? null,
    message: job.message ?? status.message,
    error: job.error ?? status.error
  };
}

export function isBusy(phase: AppPhase) {
  return phase === 'installing' || phase === 'starting' || phase === 'updating';
}

/** Live job overlay. Ignore a stale Starting/Updating job once the server is idle. */
export function mergeStatus(status: AppStatus, job?: AppJob | null): AppStatus {
  if (!job) return status;
  if (!isBusy(status.phase)) return status;
  return overlayJob(status, job);
}

export function isLaunchable(status: AppStatus) {
  return status.phase === 'running';
}

export function phaseLabel(status: AppStatus): string {
  switch (status.phase) {
    case 'installing':
      return status.percent != null ? `Installing ${status.percent}%` : 'Installing';
    case 'starting': {
      const msg = (status.message || '').trim();
      if (msg.toLowerCase().startsWith('restarting')) return 'Restarting';
      return 'Starting';
    }
    case 'running':
      return 'Running';
    case 'stopped':
      return 'Stopped';
    case 'updating':
      return status.percent != null ? `Updating ${status.percent}%` : 'Updating';
    case 'error':
      return 'Error';
    default:
      return 'Not installed';
  }
}

export function errorTip(status: AppStatus): string {
  const msg = (status.error || status.message || '').trim();
  if (msg.toLowerCase().includes('restarting')) {
    return 'A container keeps restarting. Open logs.';
  }
  if (msg) return `${msg} See logs for the full output.`;
  return 'Something went wrong. See logs for the full output.';
}

function parseJobMap(raw: string): Record<string, AppJob> {
  try {
    const data = JSON.parse(raw);
    if (data && typeof data === 'object' && !Array.isArray(data)) {
      return data as Record<string, AppJob>;
    }
  } catch {
    /* ignore */
  }
  return {};
}

export function subscribeAppJobs(onJobs: (jobs: Record<string, AppJob>) => void): () => void {
  const es = new EventSource('/api/apps/stream', { withCredentials: true });
  es.onmessage = (ev) => onJobs(parseJobMap(ev.data));
  return () => es.close();
}
