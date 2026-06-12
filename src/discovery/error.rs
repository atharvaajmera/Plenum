//! Discovery-level errors.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryError {
    /// The pairing token has expired.
    TokenExpired,
    /// The pairing token does not match.
    TokenMismatch,
    /// An I/O error occurred during broadcast or listening.
    Io { message: String },
    /// Failed to parse an announcement from the network.
    MalformedAnnouncement,
    /// No peers were discovered within the timeout.
    NoPeersFound,
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TokenExpired => write!(f, "pairing token has expired"),
            Self::TokenMismatch => write!(f, "pairing token does not match"),
            Self::Io { message } => write!(f, "discovery I/O error: {message}"),
            Self::MalformedAnnouncement => write!(f, "malformed discovery announcement"),
            Self::NoPeersFound => write!(f, "no peers discovered within the timeout"),
        }
    }
}

impl std::error::Error for DiscoveryError {}

impl From<std::io::Error> for DiscoveryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io {
            message: error.to_string(),
        }
    }
}
