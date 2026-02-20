// Auth token management and authenticated fetch

export const API_BASE = '/api';
// Use relative WebSocket URL - works with both dev proxy (port 5173) and production (port 3099)
export const WS_BASE = `${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}/ws`;

const AUTH_TOKEN_KEY = 'agent-tracker-auth-token';

/** Check URL for ?token= param, store it, and clean URL */
export function consumeTokenFromURL(): void {
  const params = new URLSearchParams(window.location.search);
  const token = params.get('token');
  if (token) {
    localStorage.setItem(AUTH_TOKEN_KEY, token);
    // Remove token from URL without reload
    params.delete('token');
    const clean = params.toString();
    const newUrl = window.location.pathname + (clean ? `?${clean}` : '') + window.location.hash;
    window.history.replaceState({}, '', newUrl);
  }
}

export function getAuthToken(): string | null {
  return localStorage.getItem(AUTH_TOKEN_KEY);
}

export function setAuthToken(token: string): void {
  localStorage.setItem(AUTH_TOKEN_KEY, token);
}

export function clearAuthToken(): void {
  localStorage.removeItem(AUTH_TOKEN_KEY);
}

/** Authenticated fetch wrapper — injects Bearer token, handles 401 */
export async function authFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const token = getAuthToken();
  const headers = new Headers(init?.headers);
  if (token) {
    headers.set('Authorization', `Bearer ${token}`);
  }
  const response = await fetch(input, { ...init, headers });
  if (response.status === 401) {
    clearAuthToken();
    window.location.reload();
  }
  return response;
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
