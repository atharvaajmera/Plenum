//! Packet checksum utilities.

use sha2::{Digest, Sha256};

use crate::protocol::packet::CHECKSUM_LEN;

pub type Checksum = [u8; CHECKSUM_LEN];

/// Computes a SHA-256 checksum over encoded packet header and payload bytes.
pub fn compute_checksum(bytes: &[u8]) -> Checksum {
    let digest = Sha256::digest(bytes);
    digest.into()
}
