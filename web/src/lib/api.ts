export type ApiError = Error & { code?: string; status?: number };

export async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  const isForm = typeof FormData !== 'undefined' && init.body instanceof FormData;
  if (!isForm && init.body && !headers.has('content-type')) {
    headers.set('content-type', 'application/json');
  }
  const res = await fetch(path, { credentials: 'include', ...init, headers });
  const text = await res.text();
  let body: any = null;
  if (text) {
    try {
      body = JSON.parse(text);
    } catch {
      body = { error: text };
    }
  }
  if (!res.ok) {
    const err = new Error(body?.error || res.statusText) as ApiError;
    err.code = body?.code;
    err.status = res.status;
    throw err;
  }
  return body as T;
}

export function setTheme(theme: 'light' | 'dark') {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem('coduos-theme', theme);
  window.dispatchEvent(new Event('coduos-theme'));
}

export function toggleTheme() {
  setTheme(document.documentElement.dataset.theme === 'light' ? 'dark' : 'light');
}

export function isLight() {
  return document.documentElement.dataset.theme === 'light';
}
