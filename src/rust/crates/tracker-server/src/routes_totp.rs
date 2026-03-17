//! TOTP authentication routes
//!
//! Handles TOTP setup, confirmation, login, disable, and status.

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use axum::{extract::State, response::Json};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use totp_rs::{Algorithm, Secret, TOTP};
use tracing::{error, info, warn};

use crate::AppState;

// ============================================================================
// Encryption helpers
// ============================================================================

const TOTP_KEY_FILENAME: &str = "totp-key.bin";

fn load_or_create_key(data_dir: &std::path::Path) -> Result<[u8; 32], String> {
    let key_path = data_dir.join(TOTP_KEY_FILENAME);
    if key_path.exists() {
        let bytes = std::fs::read(&key_path)
            .map_err(|e| format!("Failed to read TOTP key: {}", e))?;
        if bytes.len() != 32 {
            return Err("TOTP key file has invalid length".into());
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(key)
    } else {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        if let Some(parent) = key_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create key dir: {}", e))?;
        }
        std::fs::write(&key_path, &key)
            .map_err(|e| format!("Failed to write TOTP key: {}", e))?;
        info!("Generated new TOTP encryption key at {:?}", key_path);
        Ok(key)
    }
}

fn key_hash(key: &[u8; 32]) -> String {
    let hash = Sha256::digest(key);
    hex::encode(hash)
}

fn encrypt_secret(secret: &[u8], key: &[u8; 32]) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| format!("Cipher init failed: {}", e))?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, secret)
        .map_err(|e| format!("Encryption failed: {}", e))?;
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    Ok(B64.encode(&combined))
}

fn decrypt_secret(encoded: &str, key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let combined = B64.decode(encoded)
        .map_err(|e| format!("Base64 decode failed: {}", e))?;
    if combined.len() < 13 {
        return Err("Encrypted data too short".into());
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| format!("Cipher init failed: {}", e))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decryption failed: {}", e))
}

fn build_totp(secret_bytes: &[u8]) -> Result<TOTP, String> {
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret_bytes.to_vec(),
        Some("AgentTracker".to_string()),
        "admin".to_string(),
    )
    .map_err(|e| format!("TOTP creation failed: {}", e))
}

fn current_step() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 30
}

// ============================================================================
// Rate Limiter
// ============================================================================

pub(crate) struct TotpRateLimiter {
    attempts: std::sync::Mutex<Vec<Instant>>,
}

impl TotpRateLimiter {
    pub fn new() -> Self {
        Self {
            attempts: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn check_and_record(&self) -> bool {
        let mut attempts = self.attempts.lock().unwrap();
        let cutoff = Instant::now() - std::time::Duration::from_secs(60);
        attempts.retain(|t| *t > cutoff);
        if attempts.len() >= 10 {
            false
        } else {
            attempts.push(Instant::now());
            true
        }
    }
}

// ============================================================================
// API Handlers
// ============================================================================

#[derive(Serialize)]
pub(crate) struct TotpStatusResponse {
    enabled: bool,
}

pub(crate) async fn totp_status(
    State(state): State<Arc<AppState>>,
) -> Json<TotpStatusResponse> {
    let enabled = state.state.lock().unwrap().db.has_totp_active();
    Json(TotpStatusResponse { enabled })
}

pub(crate) async fn totp_setup(
    State(state): State<Arc<AppState>>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    {
        let server = state.state.lock().unwrap();
        if server.db.has_totp_active() {
            return (StatusCode::CONFLICT, Json(serde_json::json!({"error": "TOTP already enabled. Disable first to reconfigure."}))).into_response();
        }
    }

    let key = match load_or_create_key(&state.paths.data_dir) {
        Ok(k) => k,
        Err(e) => {
            error!("Failed to load TOTP key: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "Server error: key management failed"}))).into_response();
        }
    };

    let secret = Secret::generate_secret();
    let secret_bytes = secret.to_bytes().unwrap();

    let totp = match build_totp(&secret_bytes) {
        Ok(t) => t,
        Err(e) => {
            error!("Failed to build TOTP: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "Failed to generate TOTP"}))).into_response();
        }
    };

    let otpauth_uri = totp.get_url();
    let secret_base32 = secret.to_encoded().to_string();

    let encrypted = match encrypt_secret(&secret_bytes, &key) {
        Ok(e) => e,
        Err(e) => {
            error!("TOTP encryption failed: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "Server error: encryption failed"}))).into_response();
        }
    };

    let hash = key_hash(&key);
    let save_result = {
        let server = state.state.lock().unwrap();
        server.db.save_totp_config(&encrypted, &hash)
    };

    match save_result {
        Ok(()) => {
            info!("TOTP setup initiated (pending confirmation)");
            Json(serde_json::json!({
                "success": true,
                "otpauth_uri": otpauth_uri,
                "secret_base32": secret_base32,
            })).into_response()
        }
        Err(e) => {
            error!("Failed to save TOTP config: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "Failed to save TOTP configuration"}))).into_response()
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct TotpCodeRequest {
    code: String,
}

pub(crate) async fn totp_confirm(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TotpCodeRequest>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    let key = match load_or_create_key(&state.paths.data_dir) {
        Ok(k) => k,
        Err(e) => {
            error!("Failed to load TOTP key: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "TOTP configuration corrupted"}))).into_response();
        }
    };

    let config = {
        let server = state.state.lock().unwrap();
        server.db.get_totp_config()
    };

    let (encrypted, _key_hash, activated, _) = match config {
        Ok(Some(c)) => c,
        Ok(None) => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "No TOTP setup in progress"}))).into_response(),
        Err(e) => {
            error!("DB error reading TOTP config: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "Server error"}))).into_response();
        }
    };

    if activated {
        return (StatusCode::CONFLICT, Json(serde_json::json!({"error": "TOTP already activated"}))).into_response();
    }

    let secret_bytes = match decrypt_secret(&encrypted, &key) {
        Ok(b) => b,
        Err(e) => {
            error!("TOTP decryption failed: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "TOTP configuration corrupted"}))).into_response();
        }
    };

    let totp = match build_totp(&secret_bytes) {
        Ok(t) => t,
        Err(e) => {
            error!("TOTP build failed: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "Server error"}))).into_response();
        }
    };

    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    if totp.check(req.code.trim(), now) {
        let result = {
            let server = state.state.lock().unwrap();
            server.db.activate_totp()
        };
        match result {
            Ok(()) => {
                info!("TOTP activated successfully");
                Json(serde_json::json!({"success": true})).into_response()
            }
            Err(e) => {
                error!("Failed to activate TOTP: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "Failed to activate TOTP"}))).into_response()
            }
        }
    } else {
        warn!("TOTP confirmation failed: invalid code");
        (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Invalid code"}))).into_response()
    }
}

pub(crate) async fn totp_login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TotpCodeRequest>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    if !state.totp_rate_limiter.check_and_record() {
        warn!("TOTP login rate limited");
        return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({"error": "Too many attempts. Please wait."}))).into_response();
    }

    let key = match load_or_create_key(&state.paths.data_dir) {
        Ok(k) => k,
        Err(e) => {
            error!("Failed to load TOTP key: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "TOTP configuration corrupted"}))).into_response();
        }
    };

    let config = {
        let server = state.state.lock().unwrap();
        server.db.get_totp_config()
    };

    let (encrypted, _key_hash, activated, last_step) = match config {
        Ok(Some(c)) => c,
        Ok(None) | Err(_) => {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "TOTP not configured"}))).into_response();
        }
    };

    if !activated {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "TOTP not activated"}))).into_response();
    }

    let secret_bytes = match decrypt_secret(&encrypted, &key) {
        Ok(b) => b,
        Err(e) => {
            error!("TOTP decryption failed: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "TOTP configuration corrupted"}))).into_response();
        }
    };

    let totp = match build_totp(&secret_bytes) {
        Ok(t) => t,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "Server error"}))).into_response();
        }
    };

    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let step = current_step();

    if !totp.check(req.code.trim(), now) {
        warn!("TOTP login failed: invalid code");
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Invalid code"}))).into_response();
    }

    if let Some(last) = last_step {
        if step as i64 <= last {
            warn!("TOTP login rejected: replay detected (step={})", step);
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Code already used. Wait for next code."}))).into_response();
        }
    }

    {
        let server = state.state.lock().unwrap();
        if let Err(e) = server.db.update_totp_last_step(step as i64) {
            error!("Failed to update TOTP last step: {}", e);
        }
    }

    match crate::routes_auth::issue_jwt(&state.jwt_secret) {
        Ok(token) => {
            info!("TOTP login successful");
            Json(serde_json::json!({
                "success": true,
                "token": token,
                "expires_in": 7 * 24 * 3600,
            })).into_response()
        }
        Err(e) => {
            error!("Failed to issue JWT after TOTP login: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "Failed to issue session token"}))).into_response()
        }
    }
}

pub(crate) async fn totp_disable(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let result = {
        let server = state.state.lock().unwrap();
        server.db.delete_totp_config()
    };
    match result {
        Ok(()) => {
            info!("TOTP disabled");
            Json(serde_json::json!({"success": true}))
        }
        Err(e) => {
            error!("Failed to disable TOTP: {}", e);
            Json(serde_json::json!({"error": "Failed to disable TOTP"}))
        }
    }
}
