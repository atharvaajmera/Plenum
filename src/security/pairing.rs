//! Authenticated pairing tokens.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::security::{SecurityError, SessionId};

type HmacSha256 = Hmac<Sha256>;
const NONCE_LEN: usize = 16;

/// Authenticated token used to prove pairing intent over an out-of-band or
/// signaling channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticatedPairingToken {
    pub session_id: SessionId,
    pub peer_id: String,
    pub issued_at_unix_secs: u64,
    pub expires_at_unix_secs: u64,
    pub nonce: String,
    pub signature: String,
}

impl AuthenticatedPairingToken {
    pub fn issue(
        secret: &[u8],
        session_id: SessionId,
        peer_id: impl Into<String>,
        ttl: Duration,
    ) -> Result<Self, SecurityError> {
        let peer_id = peer_id.into();
        if peer_id.trim().is_empty() {
            return Err(SecurityError::EmptyPeerId);
        }
        if session_id.as_str().trim().is_empty() {
            return Err(SecurityError::EmptySessionId);
        }

        let issued_at_unix_secs = unix_now();
        let expires_at_unix_secs = issued_at_unix_secs.saturating_add(ttl.as_secs());
        let nonce = random_nonce();
        let nonce_b64 = URL_SAFE_NO_PAD.encode(nonce);

        let mut token = Self {
            session_id,
            peer_id,
            issued_at_unix_secs,
            expires_at_unix_secs,
            nonce: nonce_b64,
            signature: String::new(),
        };
        token.signature = token.compute_signature(secret)?;
        Ok(token)
    }

    pub fn verify(&self, secret: &[u8]) -> Result<(), SecurityError> {
        if self.peer_id.trim().is_empty() {
            return Err(SecurityError::EmptyPeerId);
        }
        if self.session_id.as_str().trim().is_empty() {
            return Err(SecurityError::EmptySessionId);
        }
        if unix_now() > self.expires_at_unix_secs {
            return Err(SecurityError::TokenExpired);
        }

        let signature = URL_SAFE_NO_PAD
            .decode(self.signature.as_bytes())
            .map_err(|_| SecurityError::InvalidSignature)?;

        let mut mac =
            HmacSha256::new_from_slice(secret).map_err(|_| SecurityError::InvalidSignature)?;
        mac.update(self.session_id.as_str().as_bytes());
        mac.update(b"|");
        mac.update(self.peer_id.as_bytes());
        mac.update(b"|");
        mac.update(&self.issued_at_unix_secs.to_be_bytes());
        mac.update(&self.expires_at_unix_secs.to_be_bytes());
        mac.update(b"|");
        mac.update(self.nonce.as_bytes());
        mac.verify_slice(&signature)
            .map_err(|_| SecurityError::InvalidSignature)
    }

    pub fn to_json(&self) -> Result<String, SecurityError> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self, SecurityError> {
        Ok(serde_json::from_str(json)?)
    }

    fn compute_signature(&self, secret: &[u8]) -> Result<String, SecurityError> {
        let mut mac =
            HmacSha256::new_from_slice(secret).map_err(|_| SecurityError::InvalidSignature)?;
        mac.update(self.session_id.as_str().as_bytes());
        mac.update(b"|");
        mac.update(self.peer_id.as_bytes());
        mac.update(b"|");
        mac.update(&self.issued_at_unix_secs.to_be_bytes());
        mac.update(&self.expires_at_unix_secs.to_be_bytes());
        mac.update(b"|");
        mac.update(self.nonce.as_bytes());
        Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
    }
}

fn random_nonce() -> [u8; NONCE_LEN] {
    let mut nonce = [0_u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
