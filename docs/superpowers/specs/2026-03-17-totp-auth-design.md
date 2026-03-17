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

- `totp-rs` — TOTP secret generation, code verification, otpauth URI generation

### Database (SQLite)

New table via migration:

```sql
CREATE TABLE IF NOT EXISTS totp_config (
    id INTEGER PRIMARY KEY CHECK (id = 1),  -- single-user, max one row
    encrypted_secret TEXT NOT NULL,
    activated INTEGER NOT NULL DEFAULT 0,    -- 0=pending confirm, 1=active
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### Secret Storage

- TOTP secret encrypted at rest using AES-256-GCM
- Encryption key derived from `auth_token` via SHA-256
- `encrypted_secret` stores: nonce (12 bytes) + ciphertext, base64-encoded

### API Endpoints

#### `POST /api/auth/totp/setup` (requires auth)

1. Generate random 160-bit TOTP secret
2. Encrypt and store in `totp_config` with `activated=0`
3. Return `{ otpauth_uri: "otpauth://totp/AgentTracker:admin?secret=...&issuer=AgentTracker", secret_base32: "..." }`

#### `POST /api/auth/totp/confirm` (requires auth)

Request: `{ "code": "123456" }`

1. Decrypt stored secret
2. Verify code against current time window (allow +/- 1 step)
3. If valid: set `activated=1`, return `{ success: true }`
4. If invalid: return 400 `{ error: "Invalid code" }`

#### `POST /api/auth/totp/login` (public, rate-limited)

Request: `{ "code": "123456" }`

1. Check `totp_config` exists and `activated=1`
2. Decrypt secret, verify code
3. If valid: issue JWT (same 7-day expiry as passkey login), return `{ token: "eyJ..." }`
4. If invalid: return 401, increment rate limit counter

**Rate limiting**: Max 5 attempts per 30-second window. Return 429 on exceed.

#### `DELETE /api/auth/totp` (requires auth)

1. Delete row from `totp_config`
2. Return `{ success: true }`

#### `GET /api/auth/totp/status` (public)

1. Check if `totp_config` row exists with `activated=1`
2. Return `{ enabled: true/false }`

### Auth Middleware Updates

Add to public (no-auth) whitelist:
- `POST /api/auth/totp/login`
- `GET /api/auth/totp/status`

### Rate Limiting Implementation

In-memory `HashMap<IpAddr, Vec<Instant>>` (same pattern as WebAuthn state maps). No persistence needed — resets on server restart is acceptable for a single-user system.

## Frontend Changes

### Dependencies (package.json)

- `qrcode.react` — QR code rendering from otpauth URI

### LoginView Updates

Login page flow:

```
Check /api/auth/passkey/status + /api/auth/totp/status
  ├─ has_passkey=true  → Show passkey button (existing)
  ├─ totp_enabled=true → Show "TOTP Login" section with 6-digit input
  └─ fallback          → Show token paste input (existing)
```

TOTP login UI:
- 6-digit input field (auto-focus, numeric, maxlength=6)
- Auto-submit when 6 digits entered
- Error display on invalid code or rate limit
- Loading state during verification

### Settings / TOTP Setup UI

Accessible from main app when authenticated. Could be in existing settings area or a dedicated section.

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

- **Brute force**: Rate limiting on login endpoint (5 attempts / 30s)
- **Secret at rest**: AES-256-GCM encrypted, key derived from auth_token
- **Time drift**: Accept codes from current window +/- 1 step (90-second total window)
- **No recovery codes**: Passkey + static token serve as fallback
- **Setup requires auth**: Only authenticated users can bind TOTP

## Non-Goals

- Multi-user support
- Recovery codes
- SMS/email OTP
- TOTP for API/hook authentication
- HOTP (counter-based) support
