import axios from 'axios';

import { TOKEN_KEY, UNAUTHORIZED_EVENT } from './auth';

export const API_BASE_KEY = 'nbot_api_base_url';

export function isTauriRuntime() {
  // Avoid importing `@tauri-apps/api` so the Web build stays clean.
  return (
    typeof window !== 'undefined' &&
    (Object.prototype.hasOwnProperty.call(window, '__TAURI__') ||
      Object.prototype.hasOwnProperty.call(window, '__TAURI_INTERNALS__'))
  );
}

function normalizeApiBaseUrl(input: string | null): string | null {
  if (!input) return null;
  const trimmed = input.trim();
  if (!trimmed) return null;

  if (trimmed.startsWith('/')) {
    const withoutTrailing = trimmed.replace(/\/+$/, '');
    if (!withoutTrailing) return '/api';
    if (withoutTrailing.endsWith('/api')) return withoutTrailing;
    return `${withoutTrailing}/api`;
  }

  let candidate = trimmed;
  if (!candidate.includes('://')) {
    candidate = `http://${candidate}`;
  }

  try {
    const url = new URL(candidate);
    url.search = '';
    url.hash = '';

    const withoutTrailing = url.pathname.replace(/\/+$/, '');
    url.pathname = withoutTrailing && withoutTrailing !== '/' ? withoutTrailing : '';

    if (!url.pathname.endsWith('/api')) {
      url.pathname = url.pathname ? `${url.pathname}/api` : '/api';
    }

    return url.toString().replace(/\/$/, '');
  } catch {
    return null;
  }
}

function resolveApiBaseUrl(): string {
  const envRaw =
    (import.meta.env.VITE_NBOT_API_BASE as string | undefined) ??
    (import.meta.env.VITE_API_BASE as string | undefined) ??
    null;
  const env = normalizeApiBaseUrl(envRaw);
  if (env) return env;

  const stored = normalizeApiBaseUrl(localStorage.getItem(API_BASE_KEY));
  if (stored) return stored;

  if (isTauriRuntime()) return 'http://127.0.0.1:32100/api';

  return '/api';
}

export const api = axios.create({
  baseURL: resolveApiBaseUrl(),
  timeout: 10_000,
  withCredentials: true,
});

export function setApiBaseUrl(input: string | null) {
  const normalized = normalizeApiBaseUrl(input);
  if (input && input.trim() && !normalized) {
    return { ok: false as const, error: '后端地址格式不正确' };
  }

  if (!normalized) {
    localStorage.removeItem(API_BASE_KEY);
  } else {
    localStorage.setItem(API_BASE_KEY, normalized);
  }

  const next = resolveApiBaseUrl();
  api.defaults.baseURL = next;
  return { ok: true as const, value: next };
}

api.interceptors.request.use((config) => {
  const token = localStorage.getItem(TOKEN_KEY);
  if (token) {
    config.headers = config.headers ?? {};
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

api.interceptors.response.use(
  (resp) => resp,
  (error) => {
    const status = error?.response?.status;
    if (status === 401 || status === 403) {
      window.dispatchEvent(new Event(UNAUTHORIZED_EVENT));
    }
    return Promise.reject(error);
  },
);
