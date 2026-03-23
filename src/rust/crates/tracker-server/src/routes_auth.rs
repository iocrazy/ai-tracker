//! WebAuthn Passkey authentication routes
//!
//! Handles passkey registration and login flows with JWT session tokens.

use std::sync::Arc;

use axum::{extract::State, response::Json};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use uuid::Uuid;
use webauthn_rs::prelude::*;

use crate::AppState;

// ============================================================================
// Passkey Status
// ============================================================================

/// Check if any passkeys are registered (public endpoint)
#[derive(Serialize)]
pub(crate) struct PasskeyStatusResponse {
    has_passkey: bool,
}

pub(crate) async fn passkey_status(
    State(state): State<Arc<AppState>>,
) -> Json<PasskeyStatusResponse> {
    let has = state.state.lock().unwrap().db.has_passkeys();
    Json(PasskeyStatusResponse { has_passkey: has })
}

// ============================================================================
// Registration
// ============================================================================

/// Start passkey registration (requires existing auth - bearer token)
pub(crate) async fn register_start(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let webauthn = match &state.webauthn {
        Some(w) => w,
        None => return Json(serde_json::json!({"error": "WebAuthn not configured"})),
    };

    let user_id = Uuid::new_v4();
    let user_name = "admin";
    let user_display_name = "Admin";

    // Get existing passkeys to exclude from re-registration
    let existing_keys: Vec<Passkey> = {
        let server_state = state.state.lock().unwrap();
        server_state
            .db
            .list_passkeys()
            .unwrap_or_default()
            .iter()
            .filter_map(|(_, json)| serde_json::from_str(json).ok())
            .collect()
    };

    let exclude_credentials = if existing_keys.is_empty() {
        None
    } else {
        Some(existing_keys.iter().map(|k| k.cred_id().clone()).collect())
    };

    match webauthn.start_passkey_registration(
        user_id,
        user_name,
        user_display_name,
        exclude_credentials,
    ) {
        Ok((ccr, reg_state)) => {
            let reg_id = Uuid::new_v4().to_string();
            let mut reg_states = state.webauthn_reg_states.lock().unwrap();
            reg_states.insert(reg_id.clone(), reg_state);

            info!("Passkey registration started, reg_id={}", reg_id);
            Json(serde_json::json!({
                "success": true,
                "challenge": ccr,
                "reg_id": reg_id,
            }))
        }
        Err(e) => {
            error!("Failed to start passkey registration: {}", e);
            Json(serde_json::json!({"error": format!("Registration failed: {}", e)}))
        }
    }
}

/// Finish passkey registration
#[derive(Deserialize)]
pub(crate) struct RegisterFinishRequest {
    reg_id: String,
    credential: RegisterPublicKeyCredential,
}

pub(crate) async fn register_finish(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterFinishRequest>,
) -> Json<serde_json::Value> {
    let webauthn = match &state.webauthn {
        Some(w) => w,
        None => return Json(serde_json::json!({"error": "WebAuthn not configured"})),
    };

    // Retrieve and remove registration state
    let reg_state = {
        let mut reg_states = state.webauthn_reg_states.lock().unwrap();
        reg_states.remove(&req.reg_id)
    };

    let reg_state = match reg_state {
        Some(s) => s,
        None => {
            return Json(
                serde_json::json!({"error": "Invalid or expired registration session"}),
            )
        }
    };

    match webauthn.finish_passkey_registration(&req.credential, &reg_state) {
        Ok(passkey) => {
            let cred_id = Uuid::new_v4().to_string();
            let credential_json = serde_json::to_string(&passkey).unwrap();

            let save_result = {
                let server_state = state.state.lock().unwrap();
                server_state.db.save_passkey(&cred_id, &credential_json)
            };

            match save_result {
                Ok(()) => {
                    info!("Passkey registered successfully, id={}", cred_id);
                    Json(serde_json::json!({"success": true, "message": "Passkey registered"}))
                }
                Err(e) => {
                    error!("Failed to save passkey: {}", e);
                    Json(serde_json::json!({"error": format!("Failed to save: {}", e)}))
                }
            }
        }
        Err(e) => {
            warn!("Passkey registration verification failed: {}", e);
            Json(serde_json::json!({"error": format!("Verification failed: {}", e)}))
        }
    }
}

// ============================================================================
// Authentication (Login)
// ============================================================================

/// Start passkey authentication (public endpoint)
pub(crate) async fn login_start(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let webauthn = match &state.webauthn {
        Some(w) => w,
        None => return Json(serde_json::json!({"error": "WebAuthn not configured"})),
    };

    // Load all registered passkeys
    let passkeys: Vec<Passkey> = {
        let server_state = state.state.lock().unwrap();
        server_state
            .db
            .list_passkeys()
            .unwrap_or_default()
            .iter()
            .filter_map(|(_, json)| serde_json::from_str(json).ok())
            .collect()
    };

    if passkeys.is_empty() {
        return Json(serde_json::json!({"error": "No passkeys registered"}));
    }

    match webauthn.start_passkey_authentication(&passkeys) {
        Ok((rcr, auth_state)) => {
            let auth_id = Uuid::new_v4().to_string();
            let mut auth_states = state.webauthn_auth_states.lock().unwrap();
            auth_states.insert(auth_id.clone(), auth_state);

            info!("Passkey authentication started, auth_id={}", auth_id);
            Json(serde_json::json!({
                "success": true,
                "challenge": rcr,
                "auth_id": auth_id,
            }))
        }
        Err(e) => {
            error!("Failed to start passkey authentication: {}", e);
            Json(serde_json::json!({"error": format!("Authentication failed: {}", e)}))
        }
    }
}

/// Finish passkey authentication and return JWT
#[derive(Deserialize)]
pub(crate) struct LoginFinishRequest {
    auth_id: String,
    credential: PublicKeyCredential,
}

pub(crate) async fn login_finish(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginFinishRequest>,
) -> Json<serde_json::Value> {
    let webauthn = match &state.webauthn {
        Some(w) => w,
        None => return Json(serde_json::json!({"error": "WebAuthn not configured"})),
    };

    info!("Passkey login_finish: auth_id='{}' received", req.auth_id);

    // Check if this auth_id was already successfully verified (duplicate request from proxy)
    {
        let completed = state.webauthn_completed_auths.lock().unwrap();
        info!("Passkey login_finish: completed_auths has {} entries, checking for '{}'", completed.len(), req.auth_id);
        if let Some(cached_token) = completed.get(&req.auth_id) {
            info!("Passkey login_finish: returning cached JWT for duplicate auth_id '{}'", req.auth_id);
            return Json(serde_json::json!({
                "success": true,
                "token": cached_token,
                "expires_in": 7 * 24 * 3600,
            }));
        }
    }

    // Retrieve and remove auth state
    let auth_state = {
        let mut auth_states = state.webauthn_auth_states.lock().unwrap();
        auth_states.remove(&req.auth_id)
    };

    let auth_state = match auth_state {
        Some(s) => s,
        None => {
            warn!("Passkey login_finish: auth_id '{}' not found in auth_states", req.auth_id);
            return Json(
                serde_json::json!({"error": "Invalid or expired authentication session"}),
            )
        }
    };

    // Issue JWT BEFORE finishing authentication verification response
    // This ensures the token is cached even if Cloudflare drops the connection
    let secret = state.jwt_secret.clone();
    let token = match issue_jwt(&secret) {
        Ok(t) => t,
        Err(e) => {
            error!("Failed to issue JWT before auth check: {}", e);
            return Json(serde_json::json!({"error": "Failed to issue session token"}));
        }
    };

    match webauthn.finish_passkey_authentication(&req.credential, &auth_state) {
        Ok(auth_result) => {
            // Cache JWT immediately in a spawned task (survives connection cancellation by proxy)
            let state_for_cache = state.clone();
            let auth_id = req.auth_id.clone();
            let token_for_cache = token.clone();
            tokio::spawn(async move {
                let mut map = state_for_cache.webauthn_completed_auths.lock().unwrap();
                map.insert(auth_id.clone(), token_for_cache);
                tracing::warn!("Passkey: JWT cached for poll, auth_id={}, entries={}", auth_id, map.len());
            });

            info!(
                "Passkey authentication successful, counter={}",
                auth_result.counter()
            );

            Json(serde_json::json!({
                "success": true,
                "token": token,
                "expires_in": 7 * 24 * 3600,
            }))
        }
        Err(e) => {
            warn!("Passkey authentication verification failed: {}", e);
            Json(serde_json::json!({"error": format!("Authentication failed: {}", e)}))
        }
    }
}

// ============================================================================
// JWT
// ============================================================================

#[derive(Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
    iat: usize,
}

pub(crate) fn issue_jwt(secret: &str) -> Result<String, jsonwebtoken::errors::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize;

    let claims = Claims {
        sub: "admin".to_string(),
        iat: now,
        exp: now + 7 * 24 * 3600, // 7 days
    };

    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
}

/// Poll for passkey login result — used when login_finish gets 502 from proxy
pub(crate) async fn passkey_poll(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let auth_id = params.get("auth_id").cloned().unwrap_or_default();
    if auth_id.is_empty() {
        return Json(serde_json::json!({"ready": false}));
    }
    let completed = state.webauthn_completed_auths.lock().unwrap();
    if let Some(token) = completed.get(&auth_id) {
        Json(serde_json::json!({"ready": true, "success": true, "token": token, "expires_in": 7 * 24 * 3600}))
    } else {
        Json(serde_json::json!({"ready": false}))
    }
}

pub(crate) fn verify_jwt(token: &str, secret: &str) -> bool {
    let validation = jsonwebtoken::Validation::default();
    jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .is_ok()
}
