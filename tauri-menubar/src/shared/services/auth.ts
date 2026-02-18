import { Store } from '@tauri-apps/plugin-store';

// Hardcoded server URL (no proxy in Tauri)
export const API_BASE = 'http://127.0.0.1:3099/api';
export const WS_BASE = 'ws://127.0.0.1:3099/ws';

const STORE_PATH = 'settings.json';
const AUTH_TOKEN_KEY = 'auth-token';

let _store: Store | null = null;
let _cachedToken: string | null = null;

async function getStore(): Promise<Store> {
  if (!_store) {
    _store = await Store.load(STORE_PATH);
  }
  return _store;
}

export async function getAuthTokenAsync(): Promise<string | null> {
  if (_cachedToken) return _cachedToken;
  const store = await getStore();
  const token = await store.get<string>(AUTH_TOKEN_KEY);
  _cachedToken = token ?? null;
  return _cachedToken;
}

// Synchronous getter (uses cached value)
export function getAuthToken(): string | null {
  return _cachedToken;
}

export async function setAuthToken(token: string): Promise<void> {
  const store = await getStore();
  await store.set(AUTH_TOKEN_KEY, token);
  await store.save();
  _cachedToken = token;
}

export async function clearAuthToken(): Promise<void> {
  const store = await getStore();
  await store.delete(AUTH_TOKEN_KEY);
  await store.save();
  _cachedToken = null;
}

/** Authenticated fetch wrapper */
export async function authFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const token = getAuthToken();
  const headers = new Headers(init?.headers);
  if (token) {
    headers.set('Authorization', `Bearer ${token}`);
  }
  return fetch(input, { ...init, headers });
}

/** Verify a token against the server */
export async function verifyToken(token: string): Promise<boolean> {
  try {
    const response = await fetch(`${API_BASE}/auth/verify`, {
      headers: { 'Authorization': `Bearer ${token}` },
    });
    return response.ok;
  } catch {
    return false;
  }
}
