//! `GET /turn-credentials` — mints short-lived coturn REST-API credentials.
//!
//! Implements coturn's `use-auth-secret` REST scheme:
//! <https://github.com/coturn/coturn/blob/master/docs/REST_API_docs/turn-rest-secret-server.txt>
//!
//! `username = "<unix_ts + ttl>:<peer_id>"`, `credential = base64(HMAC-SHA1(secret, username))`.
//! Per the coturn spec this authentication scheme is HMAC-**SHA1**, not SHA256.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};

type HmacSha1 = Hmac<sha1::Sha1>;

/// Credential TTL: 1 hour, generous enough to cover ICE negotiation plus a
/// full transfer without needing renewal.
const TTL_SECS: u64 = 3600;

#[derive(Debug, Deserialize)]
pub struct TurnCredentialsQuery {
    pub peer_id: String,
}

#[derive(Debug, Serialize)]
pub struct TurnCredentialsResponse {
    pub username: String,
    pub credential: String,
    pub urls: Vec<String>,
    pub ttl_secs: u64,
}

pub async fn turn_credentials_handler(
    State(state): State<Arc<crate::state::AppState>>,
    Query(query): Query<TurnCredentialsQuery>,
) -> impl IntoResponse {
    let Some(secret) = state.turn_secret.as_ref() else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "TURN is not configured on this relay server"
            })),
        )
            .into_response();
    };

    if query.peer_id.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "peer_id must not be empty" })),
        )
            .into_response();
    }

    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + TTL_SECS;

    let username = format!("{expiry}:{}", query.peer_id);

    let mut mac = match HmacSha1::new_from_slice(secret.as_bytes()) {
        Ok(mac) => mac,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("hmac init failed: {err}") })),
            )
                .into_response();
        }
    };
    mac.update(username.as_bytes());
    let credential = BASE64.encode(mac.finalize().into_bytes());

    let response = TurnCredentialsResponse {
        username,
        credential,
        urls: state.turn_urls.clone(),
        ttl_secs: TTL_SECS,
    };

    Json(response).into_response()
}
