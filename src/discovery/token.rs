//! Short-lived pairing tokens for session authentication.
//!
//! A pairing token is a random 6-character alphanumeric code that expires
//! after a configurable duration. The receiver broadcasts the token so the
//! sender can confirm they are connecting to the right peer.

use std::time::{Duration, Instant};

const TOKEN_LEN: usize = 6;
const DEFAULT_TTL: Duration = Duration::from_secs(300); // 5 minutes

/// A short-lived pairing token used during local discovery.
#[derive(Debug, Clone)]
pub struct PairingToken {
    code: String,
    created_at: Instant,
    ttl: Duration,
}

impl PairingToken {
    /// Generates a new random pairing token with the default TTL (5 minutes).
    pub fn generate() -> Self {
        Self::generate_with_ttl(DEFAULT_TTL)
    }

    /// Generates a new random pairing token with a custom TTL.
    pub fn generate_with_ttl(ttl: Duration) -> Self {
        Self::generate_with_len_and_ttl(TOKEN_LEN, ttl)
    }

    /// Generates a new random pairing token with a custom code length and the
    /// default TTL. Used for internet-transfer room codes, which are longer
    /// than the default 6-character LAN pairing PIN since they double as the
    /// actual session secret on a public relay server.
    pub fn generate_with_len(len: usize) -> Self {
        Self::generate_with_len_and_ttl(len, DEFAULT_TTL)
    }

    fn generate_with_len_and_ttl(len: usize, ttl: Duration) -> Self {
        let code = random_alphanumeric(len);
        Self {
            code,
            created_at: Instant::now(),
            ttl,
        }
    }

    /// Creates a token from a known code (used by the sender after receiving
    /// an announcement). The token is considered valid from this instant.
    pub fn from_code(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            created_at: Instant::now(),
            ttl: DEFAULT_TTL,
        }
    }

    /// Returns the token code as a string slice.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns `true` if the token has not yet expired.
    pub fn is_valid(&self) -> bool {
        self.created_at.elapsed() < self.ttl
    }

    /// Validates that this token matches another code and has not expired.
    pub fn verify(&self, other: &str) -> bool {
        self.is_valid() && self.code == other
    }
}

/// Generates a cryptographically secure random alphanumeric string of the
/// given length using the operating system's CSPRNG (OsRng).
fn random_alphanumeric(len: usize) -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

    let mut rng = rand::rngs::OsRng;
    (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}
