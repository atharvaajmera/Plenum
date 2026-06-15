//! Secure session identifier generation.

use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::security::SecurityError;

const SESSION_ID_BYTES: usize = 16;
const SESSION_ID_HEX_LEN: usize = SESSION_ID_BYTES * 2;

/// Random session identifier suitable for pairing, signaling, and logging.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    pub fn generate() -> Self {
        let mut bytes = [0_u8; SESSION_ID_BYTES];
        OsRng.fill_bytes(&mut bytes);
        Self(hex_encode(&bytes))
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, SecurityError> {
        let value = value.into();
        if value.len() != SESSION_ID_HEX_LEN || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(SecurityError::InvalidSessionId);
        }

        Ok(Self(value.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }

    output
}
