import { writable } from 'svelte/store';
import { api } from './api';

export type PowerAction = 'reboot' | 'shutdown';

export type PowerView = {
  action: PowerAction | null;
  /** True once the dashboard could not be reached (machine is going/gone). */
  down: boolean;
};

export const powerView = writable<PowerView>({ action: null, down: false });

let watchTimer = 0;

export async function applyPower(action: PowerAction): Promise<void> {
  await api('/api/system/power', { method: 'POST', body: JSON.stringify({ action }) });
  beginWait(action);
}

export function beginWait(action: PowerAction) {
  window.clearTimeout(watchTimer);
  try {
    sessionStorage.setItem('coduos-power', action);
  } catch {
    /* private mode */
  }
  powerView.set({ action, down: false });
  watch(action, false);
}

/** Resume the waiting screen after a tab reload during reboot/shutdown. */
export async function resumeWait() {
  let action: string | null = null;
  try {
    action = sessionStorage.getItem('coduos-power');
  } catch {
    return;
  }
  if (action !== 'reboot' && action !== 'shutdown') return;
  const up = await ping();
  if (up) {
    try {
      sessionStorage.removeItem('coduos-power');
    } catch {
      /* ignore */
    }
    return;
  }
  powerView.set({ action, down: true });
  watch(action, true);
}

async function ping(): Promise<boolean> {
  const ctrl = new AbortController();
  const t = window.setTimeout(() => ctrl.abort(), 2500);
  try {
    const res = await fetch('/api/health', {
      cache: 'no-store',
      credentials: 'include',
      signal: ctrl.signal
    });
    if (!res.ok) return false;
    const body = (await res.json()) as { ok?: boolean };
    return body.ok === true;
  } catch {
    return false;
  } finally {
    window.clearTimeout(t);
  }
}

function watch(action: PowerAction, down: boolean) {
  watchTimer = window.setTimeout(async () => {
    const up = await ping();
    if (!up) {
      if (!down) powerView.set({ action, down: true });
      watch(action, true);
      return;
    }
    if (down) {
      try {
        sessionStorage.removeItem('coduos-power');
      } catch {
        /* ignore */
      }
      location.reload();
      return;
    }
    watch(action, false);
  }, down ? 2000 : 1200);
}
