# TOTP Authentication Design

**Date**: 2026-03-17
**Status**: Approved

## Overview

Add TOTP (Time-based One-Time Password) as a third Web UI login method alongside passkey and static token. TOTP replaces token-paste for human login scenarios while static token remains for hook/script automation.

## Authentication Matrix

| Scenario        | Method       | Notes                                |
|-----------------|-------------|--------------------------------------|
| Web UI login    | Passkey      | Primary, already implemented         |
| Web UI login    | TOTP         | New, passkey-unavailable fallback    |
| Web UI login    | Static token | Legacy, still available              |
| Hook/script     | Static token | Automated, cannot use TOTP           |

## Backend Changes

### Dependencies (Cargo.toml)

- `totp-rs = { version = "5", features = ["otpauth", "gen_secret"] }` — TOTP secret generation, code verification, otpauth URI generation
- `aes-gcm` — AES-256-GCM encryption for secret at rest

### Database (SQLite)

New table via migration:

```sql
CREATE TABLE IF NOT EXISTS totp_config (
    id INTEGER PRIMARY KEY CHECK (id = 1),  -- single-user, max one row
    encrypted_secret TEXT NOT NULL,
    encryption_key_hash TEXT NOT NULL,       -- SHA-256 of encryption key, for change detection
    activated INTEGER NOT NULL DEFAULT 0,    -- 0=pending confirm, 1=active
    last_used_step INTEGER,                  -- replay protection: last successful TOTP time step
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### Secret Storage

- TOTP secret encrypted at rest using AES-256-GCM
- Encryption key: auto-generated 256-bit random key, stored at `{data_dir}/totp-key.bin`
  - Separate from `auth_token` — changing the static token does NOT invalidate TOTP
  - Generated once on first TOTP setup, persists until manually deleted
- `encrypted_secret` stores: nonce (12 bytes) + ciphertext, base64-encoded
- `encryption_key_hash`: SHA-256 of the key, stored in DB for corruption detection

### API Endpoints

#### `POST /api/auth/totp/setup` (requires auth)

1. If TOTP already activated, return 409 `{ error: "TOTP already enabled. Disable first to reconfigure." }`
2. Generate or load encryption key from `totp-key.bin`
3. Generate random 160-bit TOTP secret
4. Encrypt and store in `totp_config` with `activated=0`
5. Return `{ otpauth_uri: "otpauth://totp/AgentTracker:admin?secret=...&issuer=AgentTracker", secret_base32: "..." }`

#### `POST /api/auth/totp/confirm` (requires auth)

Request: `{ "code": "123456" }`

1. Decrypt stored secret (if decryption fails, return 500 `{ error: "TOTP configuration corrupted" }` and log details)
2. Verify code against current time window (allow +/- 1 step)
3. If valid: set `activated=1`, return `{ success: true }`
4. If invalid: return 400 `{ error: "Invalid code" }`

#### `POST /api/auth/totp/login` (public, rate-limited)

Request: `{ "code": "123456" }`

1. Check `totp_config` exists and `activated=1`
2. Decrypt secret (if decryption fails, return 500 and log)
3. Verify code against current time window (+/- 1 step)
4. **Replay protection**: reject if code's time step <= `last_used_step`
5. If valid: update `last_used_step`, issue JWT via existing `issue_jwt()` from `routes_auth.rs`, return `{ token: "eyJ..." }`
6. If invalid: return 401, increment rate limit counter

**Rate limiting**: Global (not per-IP, single-user system). Max 10 attempts per 60-second sliding window. Return 429 on exceed. Counter resets after window expires.

#### `DELETE /api/auth/totp` (requires auth)

1. Delete row from `totp_config`
2. Return `{ success: true }`

Note: Since the user is already authenticated, no additional TOTP code confirmation is required for disable.

#### `GET /api/auth/totp/status` (public)

1. Check if `totp_config` row exists with `activated=1`
2. Return `{ enabled: true/false }`

Note: Public to allow login page UI decisions. Same pattern as existing `/api/auth/passkey/status`. Acceptable trade-off for UX — only reveals whether TOTP is configured, not secrets.

### Auth Middleware Updates

Add to public (no-auth) whitelist:
- `POST /api/auth/totp/login`
- `GET /api/auth/totp/status`

New endpoints use existing CORS configuration from `allowed_origins` in AppState.

### Rate Limiting Implementation

In-memory global counter: `(Vec<Instant>, Mutex)`. Sliding window — on each attempt, prune entries older than 60s, then check count. No persistence needed — resets on server restart is acceptable for a single-user system.

## Frontend Changes

### Dependencies (package.json)

- `qrcode.react` — QR code rendering from otpauth URI

### LoginView Updates

Login page flow with visual hierarchy:

```
Check /api/auth/passkey/status + /api/auth/totp/status

  1. Passkey button (primary, if has_passkey)
  2. TOTP 6-digit input (secondary, if totp_enabled)
  3. "Or use token" expandable link → token paste input (tertiary)
```

TOTP login UI:
- 6-digit input field (auto-focus, numeric, maxlength=6)
- Auto-submit when 6 digits entered
- Error display on invalid code or rate limit (429 → "Too many attempts, wait a moment")
- Loading state during verification

### Settings / TOTP Setup UI

Add a gear/settings icon in the app header (next to existing passkey management area). Opens a settings section containing TOTP setup.

Setup flow:
1. Click "Enable TOTP"
2. Call `POST /api/auth/totp/setup`
3. Display QR code (from `otpauth_uri`) + manual secret (base32)
4. Input field for confirmation code
5. Call `POST /api/auth/totp/confirm` with entered code
6. Success: show "TOTP enabled" status
7. Fail: show error, allow retry

Disable flow:
1. Click "Disable TOTP"
2. Confirmation dialog
3. Call `DELETE /api/auth/totp`
4. Update UI to show disabled state

## Security

- **Brute force**: Global rate limiting on login endpoint (10 attempts / 60s sliding window)
- **Replay protection**: Track `last_used_step` — each TOTP code accepted only once
- **Secret at rest**: AES-256-GCM encrypted with auto-generated key file (independent of auth_token)
- **Decryption failure**: Return 500, log details server-side. Does not auto-delete TOTP config.
- **Time drift**: Accept codes from current window +/- 1 step (90-second total window)
- **No recovery codes**: Passkey + static token serve as fallback
- **Setup requires auth**: Only authenticated users can bind TOTP
- **Public status endpoint**: Only reveals enabled/disabled, consistent with passkey status pattern

## Non-Goals

- Multi-user support
- Recovery codes
- SMS/email OTP
- TOTP for API/hook authentication
- HOTP (counter-based) support
