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

// ============================================================================
// WebAuthn Passkey
// ============================================================================

/** Check if any passkeys are registered */
export async function checkPasskeyStatus(): Promise<boolean> {
  // Hide passkey button if WebAuthn API not available
  if (!navigator.credentials || !navigator.credentials.get) {
    return false;
  }
  try {
    const response = await fetch(`${API_BASE}/auth/passkey/status`);
    if (!response.ok) return false;
    const data = await response.json();
    return data.has_passkey === true;
  } catch {
    return false;
  }
}

/** Helper: Base64URL decode to Uint8Array */
function base64urlToUint8Array(base64url: string): Uint8Array {
  const base64 = base64url.replace(/-/g, '+').replace(/_/g, '/');
  const pad = base64.length % 4;
  const padded = pad ? base64 + '='.repeat(4 - pad) : base64;
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/** Helper: ArrayBuffer to Base64URL string */
function arrayBufferToBase64url(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let binary = '';
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=/g, '');
}

/** Start passkey registration (must be authenticated already) */
export async function registerPasskeyStart(): Promise<{ challenge: any; reg_id: string } | null> {
  const response = await authFetch(`${API_BASE}/auth/webauthn/register/start`, { method: 'POST' });
  if (!response.ok) return null;
  const data = await response.json();
  if (!data.success) return null;
  return { challenge: data.challenge, reg_id: data.reg_id };
}

/** Finish passkey registration */
export async function registerPasskeyFinish(regId: string, credential: PublicKeyCredential): Promise<boolean> {
  const attestation = credential.response as AuthenticatorAttestationResponse;

  const body = {
    reg_id: regId,
    credential: {
      id: credential.id,
      rawId: arrayBufferToBase64url(credential.rawId),
      type: credential.type,
      response: {
        attestationObject: arrayBufferToBase64url(attestation.attestationObject),
        clientDataJSON: arrayBufferToBase64url(attestation.clientDataJSON),
      },
      extensions: credential.getClientExtensionResults(),
    },
  };

  const response = await authFetch(`${API_BASE}/auth/webauthn/register/finish`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  const data = await response.json();
  return data.success === true;
}

/** Start passkey login */
export async function loginPasskeyStart(): Promise<{ challenge: any; auth_id: string } | null> {
  const response = await fetch(`${API_BASE}/auth/webauthn/login/start`, { method: 'POST' });
  if (!response.ok) return null;
  const data = await response.json();
  if (!data.success) return null;
  return { challenge: data.challenge, auth_id: data.auth_id };
}

/** Finish passkey login → returns JWT */
export async function loginPasskeyFinish(authId: string, credential: PublicKeyCredential): Promise<string | null> {
  const assertion = credential.response as AuthenticatorAssertionResponse;

  const body = {
    auth_id: authId,
    credential: {
      id: credential.id,
      rawId: arrayBufferToBase64url(credential.rawId),
      type: credential.type,
      response: {
        authenticatorData: arrayBufferToBase64url(assertion.authenticatorData),
        clientDataJSON: arrayBufferToBase64url(assertion.clientDataJSON),
        signature: arrayBufferToBase64url(assertion.signature),
        userHandle: assertion.userHandle ? arrayBufferToBase64url(assertion.userHandle) : null,
      },
      extensions: credential.getClientExtensionResults(),
    },
  };

  // Send finish request
  const response = await fetch(`${API_BASE}/auth/webauthn/login/finish`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });

  // Try parse response
  const text = await response.text();
  try {
    const data = JSON.parse(text);
    if (data.success && data.token) {
      return data.token;
    }
  } catch {
    // Non-JSON (502 from proxy) — fall through to polling
  }

  // If 502 or no token: poll for the result (server may have processed it)
  console.log(`login/finish got HTTP ${response.status}, polling for result...`);
  for (let i = 0; i < 10; i++) {
    await new Promise(r => setTimeout(r, 500));
    try {
      const pollRes = await fetch(`${API_BASE}/auth/passkey/poll?auth_id=${encodeURIComponent(authId)}`);
      const pollData = await pollRes.json();
      if (pollData.ready && pollData.token) {
        return pollData.token;
      }
    } catch { /* ignore */ }
  }

  throw new Error(`Passkey verification may have succeeded but response was lost (HTTP ${response.status})`);
}

/** Full passkey login flow: start → browser prompt → finish → store JWT
 *  Retries the ENTIRE flow on 502 (Synology proxy timeout) */
export async function loginWithPasskey(): Promise<boolean> {
  for (let flowAttempt = 0; flowAttempt < 2; flowAttempt++) {
    try {
      return await _loginWithPasskeyOnce();
    } catch (e: any) {
      if (flowAttempt === 0 && e?.message?.includes('502')) {
        console.warn('Passkey login got 502, retrying entire flow...');
        await new Promise(r => setTimeout(r, 1000));
        continue;
      }
      throw e;
    }
  }
  throw new Error('Passkey login failed after retries');
}

async function _loginWithPasskeyOnce(): Promise<boolean> {
  // Step 1: Get challenge from server
  const startData = await loginPasskeyStart();
  if (!startData) throw new Error('Failed to get challenge from server');

  const { challenge, auth_id } = startData;

  // Step 2: Convert challenge for browser API
  const publicKey = challenge.publicKey;
  publicKey.challenge = base64urlToUint8Array(publicKey.challenge);
  if (publicKey.allowCredentials) {
    publicKey.allowCredentials = publicKey.allowCredentials.map((c: any) => ({
      ...c,
      id: base64urlToUint8Array(c.id),
    }));
  }

  // Step 3: Browser prompt
  // Check if WebAuthn is available
  if (!navigator.credentials || !navigator.credentials.get) {
    throw new Error('WebAuthn not supported in this browser. Try using Safari directly or a desktop browser.');
  }
  // Wait for document focus (iOS Safari loses focus when Bitwarden/autofill UI appears)
  if (!document.hasFocus()) {
    await new Promise<void>((resolve) => {
      const onFocus = () => { window.removeEventListener('focus', onFocus); resolve(); };
      window.addEventListener('focus', onFocus);
      // Also resolve after 2s in case focus event doesn't fire
      setTimeout(resolve, 2000);
    });
  }
  let credential: PublicKeyCredential;
  try {
    credential = await navigator.credentials.get({ publicKey }) as PublicKeyCredential;
  } catch (e: any) {
    // Retry once if document was not focused
    if (e?.message?.includes('not focused') || e?.name === 'NotAllowedError') {
      await new Promise(r => setTimeout(r, 500));
      try {
        credential = await navigator.credentials.get({ publicKey }) as PublicKeyCredential;
      } catch (e2: any) {
        throw new Error(`Bitwarden prompt failed: ${e2?.message || 'cancelled'}`);
      }
    } else {
      throw new Error(`Bitwarden prompt failed: ${e?.message || 'cancelled'}`);
    }
  }

  // Step 4: Send to server for verification
  let token: string | null;
  try {
    token = await loginPasskeyFinish(auth_id, credential);
  } catch (e: any) {
    throw new Error(`Finish request failed: ${e?.message}`);
  }
  if (!token) throw new Error('Server returned no token (check console for details)');

  // Step 5: Store JWT
  setAuthToken(token);
  return true;
}

// ============================================================================
// TOTP
// ============================================================================

/** Check if TOTP is enabled */
export async function checkTotpStatus(): Promise<boolean> {
  try {
    const response = await fetch(`${API_BASE}/auth/totp/status`);
    if (!response.ok) return false;
    const data = await response.json();
    return data.enabled === true;
  } catch {
    return false;
  }
}

/** Login with TOTP code → returns JWT or throws */
export async function loginWithTotp(code: string): Promise<string> {
  const response = await fetch(`${API_BASE}/auth/totp/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ code }),
  });
  if (response.status === 429) {
    throw new Error('Too many attempts. Please wait.');
  }
  const data = await response.json();
  if (!response.ok || !data.success) {
    throw new Error(data.error || 'TOTP login failed');
  }
  return data.token;
}

/** Start TOTP setup → returns otpauth URI and secret */
export async function setupTotp(): Promise<{ otpauth_uri: string; secret_base32: string }> {
  const response = await authFetch(`${API_BASE}/auth/totp/setup`, { method: 'POST' });
  const data = await response.json();
  if (!data.success) {
    throw new Error(data.error || 'TOTP setup failed');
  }
  return { otpauth_uri: data.otpauth_uri, secret_base32: data.secret_base32 };
}

/** Confirm TOTP setup with a verification code */
export async function confirmTotp(code: string): Promise<boolean> {
  const response = await authFetch(`${API_BASE}/auth/totp/confirm`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ code }),
  });
  const data = await response.json();
  if (!data.success) {
    throw new Error(data.error || 'Invalid code');
  }
  return true;
}

/** Disable TOTP */
export async function disableTotp(): Promise<boolean> {
  const response = await authFetch(`${API_BASE}/auth/totp`, { method: 'DELETE' });
  const data = await response.json();
  return data.success === true;
}

/** Full passkey registration flow */
export async function registerPasskey(): Promise<boolean> {
  // Step 1: Get challenge
  const startData = await registerPasskeyStart();
  if (!startData) return false;

  const { challenge, reg_id } = startData;

  // Step 2: Convert challenge for browser API
  const publicKey = challenge.publicKey;
  publicKey.challenge = base64urlToUint8Array(publicKey.challenge);
  publicKey.user.id = base64urlToUint8Array(publicKey.user.id);
  if (publicKey.excludeCredentials) {
    publicKey.excludeCredentials = publicKey.excludeCredentials.map((c: any) => ({
      ...c,
      id: base64urlToUint8Array(c.id),
    }));
  }

  // Step 3: Browser prompt
  let credential: PublicKeyCredential;
  try {
    credential = await navigator.credentials.create({ publicKey }) as PublicKeyCredential;
  } catch (e) {
    console.error('Passkey registration cancelled or failed:', e);
    return false;
  }

  // Step 4: Send to server
  return registerPasskeyFinish(reg_id, credential);
}
