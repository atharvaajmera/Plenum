//! Security-related errors.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityError {
    EmptyPeerId,
    EmptySessionId,
    InvalidSessionId,
    TokenExpired,
    InvalidSignature,
    ReplayDetected,
    InvalidCapacity,
    InvalidNonce,
    EncryptionFailed,
    DecryptionFailed,
    Json(String),
}

impl fmt::Display for SecurityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPeerId => write!(f, "peer id must not be empty"),
            Self::EmptySessionId => write!(f, "session id must not be empty"),
            Self::InvalidSessionId => write!(f, "session id is malformed"),
            Self::TokenExpired => write!(f, "pairing token has expired"),
            Self::InvalidSignature => write!(f, "pairing token signature is invalid"),
            Self::ReplayDetected => write!(f, "replayed frame or nonce detected"),
            Self::InvalidCapacity => write!(f, "capacity must be greater than zero"),
            Self::InvalidNonce => write!(f, "nonce is malformed"),
            Self::EncryptionFailed => write!(f, "failed to encrypt payload"),
            Self::DecryptionFailed => write!(f, "failed to decrypt payload"),
            Self::Json(message) => write!(f, "security json error: {message}"),
        }
    }
}

impl std::error::Error for SecurityError {}

impl From<serde_json::Error> for SecurityError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error.to_string())
    }
}
