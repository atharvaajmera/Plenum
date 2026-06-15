//! Authenticated end-to-end encryption envelope.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::security::{ReplayProtector, SecurityError};

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const DEFAULT_REPLAY_CAPACITY: usize = 4096;

pub type SessionKey = [u8; KEY_LEN];

/// Encrypted byte frame with a transport-safe nonce representation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedFrame {
    pub nonce: String,
    pub ciphertext: Vec<u8>,
}

/// Session-scoped AEAD cipher with replay protection for inbound frames.
pub struct SessionCipher {
    cipher: ChaCha20Poly1305,
    replay_protector: ReplayProtector,
}

impl SessionCipher {
    pub fn generate_key() -> SessionKey {
        let mut key = [0_u8; KEY_LEN];
        OsRng.fill_bytes(&mut key);
        key
    }

    pub fn new(key: &SessionKey) -> Result<Self, SecurityError> {
        Self::with_replay_capacity(key, DEFAULT_REPLAY_CAPACITY)
    }

    pub fn with_replay_capacity(
        key: &SessionKey,
        replay_capacity: usize,
    ) -> Result<Self, SecurityError> {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
        let replay_protector = ReplayProtector::new(replay_capacity)?;
        Ok(Self {
            cipher,
            replay_protector,
        })
    }

    pub fn encrypt(
        &self,
        plaintext: &[u8],
        associated_data: &[u8],
    ) -> Result<EncryptedFrame, SecurityError> {
        let mut nonce_bytes = [0_u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: associated_data,
                },
            )
            .map_err(|_| SecurityError::EncryptionFailed)?;

        Ok(EncryptedFrame {
            nonce: URL_SAFE_NO_PAD.encode(nonce_bytes),
            ciphertext,
        })
    }

    pub fn decrypt(
        &mut self,
        frame: &EncryptedFrame,
        associated_data: &[u8],
    ) -> Result<Vec<u8>, SecurityError> {
        let nonce_bytes = URL_SAFE_NO_PAD
            .decode(frame.nonce.as_bytes())
            .map_err(|_| SecurityError::InvalidNonce)?;
        if nonce_bytes.len() != NONCE_LEN {
            return Err(SecurityError::InvalidNonce);
        }

        let plaintext = self
            .cipher
            .decrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: &frame.ciphertext,
                    aad: associated_data,
                },
            )
            .map_err(|_| SecurityError::DecryptionFailed)?;

        self.replay_protector.check_and_insert(nonce_bytes)?;
        Ok(plaintext)
    }
}
