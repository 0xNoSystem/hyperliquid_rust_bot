use std::sync::Arc;

use alloy::primitives::Address;
use alloy_primitives::Signature as AlloySig;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use super::app_state::AppState;

/// JWT claims.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // pubkey
    pub exp: usize,  // expiry (unix timestamp)
    pub iat: usize,  // issued at
}

/// The authenticated user, extracted from JWT on every protected route.
pub struct AuthUser {
    pub pubkey: String,
}

impl FromRequestParts<Arc<AppState>> for AuthUser {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // Try Authorization header first, then query param `token` (for WebSocket)
        let token = extract_token(parts).ok_or(StatusCode::UNAUTHORIZED)?;

        let claims = verify_jwt(&token, &state.jwt_secret).map_err(|_| StatusCode::UNAUTHORIZED)?;

        Ok(AuthUser { pubkey: claims.sub })
    }
}

fn extract_token(parts: &Parts) -> Option<String> {
    // 1. Authorization: Bearer <token>
    if let Some(auth_header) = parts.headers.get("authorization")
        && let Ok(value) = auth_header.to_str()
        && let Some(token) = value.strip_prefix("Bearer ")
    {
        return Some(token.to_string());
    }

    // 2. Query param ?token=...
    if let Some(query) = parts.uri.query() {
        for pair in query.split('&') {
            if let Some(token) = pair.strip_prefix("token=") {
                return Some(token.to_string());
            }
        }
    }

    None
}

/// Issue a JWT for a verified user.
pub fn issue_jwt(pubkey: &str, secret: &str) -> Result<String, jsonwebtoken::errors::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize;

    let claims = Claims {
        sub: pubkey.to_string(),
        iat: now,
        exp: now + 86400, // 24 hours
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

/// Verify a JWT and return the claims.
pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(token_data.claims)
}

/// Generate a random nonce string.
pub fn generate_nonce() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// EIP-191 personal_sign message hash.
fn eip191_hash(message: &[u8]) -> alloy::primitives::B256 {
    let prefix = format!("\x19Ethereum Signed Message:\n{}", message.len());
    let mut data = prefix.into_bytes();
    data.extend_from_slice(message);
    alloy::primitives::keccak256(&data)
}

/// Verify an EIP-191 personal_sign signature.
/// Returns Ok(()) if the recovered address matches the expected address.
pub fn verify_signature(address: &str, signature_hex: &str, nonce: &str) -> Result<(), String> {
    let message = format!(
        "Sign this message to authenticate with Hyperliquid Terminal.\n\nNonce: {}",
        nonce
    );

    // EIP-191 hash
    let hash = eip191_hash(message.as_bytes());

    // Parse signature
    let sig_bytes = hex::decode(signature_hex.strip_prefix("0x").unwrap_or(signature_hex))
        .map_err(|e| format!("invalid signature hex: {}", e))?;

    if sig_bytes.len() != 65 {
        return Err("signature must be 65 bytes".to_string());
    }

    let v = sig_bytes[64];
    // Normalize v: 27/28 → 0/1
    let v_normalized = if v >= 27 { v - 27 } else { v };

    let r = alloy::primitives::B256::from_slice(&sig_bytes[..32]);
    let s = alloy::primitives::B256::from_slice(&sig_bytes[32..64]);
    let sig = AlloySig::from_scalars_and_parity(r, s, v_normalized != 0);

    let recovered = sig
        .recover_address_from_prehash(&hash)
        .map_err(|e| format!("signature recovery failed: {}", e))?;

    let expected: Address = address
        .parse()
        .map_err(|e| format!("invalid address: {}", e))?;

    if recovered != expected {
        return Err(format!(
            "address mismatch: expected {}, recovered {}",
            expected, recovered
        ));
    }

    Ok(())
}

/// Spawn a background task that prunes expired nonces every 60 seconds.
pub fn spawn_nonce_pruner(nonces: super::app_state::NonceStore) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let mut store = nonces.write().await;
            store.retain(|_, (_, created_at)| created_at.elapsed().as_secs() < 300);
        }
    });
}
