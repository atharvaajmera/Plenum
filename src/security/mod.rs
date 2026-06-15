//! Security hardening primitives.
//!
//! This module provides reusable security components that remain independent of
//! discovery, signaling, and transport layers:
//! - secure session identifiers
//! - authenticated pairing tokens
//! - replay protection
//! - authenticated end-to-end encryption envelopes

pub mod cipher;
pub mod error;
pub mod pairing;
pub mod replay;
pub mod session;

pub use cipher::{EncryptedFrame, SessionCipher, SessionKey};
pub use error::SecurityError;
pub use pairing::AuthenticatedPairingToken;
pub use replay::ReplayProtector;
pub use session::SessionId;
