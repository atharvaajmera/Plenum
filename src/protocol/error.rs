//! Protocol encoding and parsing errors.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    PayloadTooLarge { len: usize },
    TruncatedHeader { actual: usize, expected: usize },
    TruncatedPayload { actual: usize, expected: usize },
    TruncatedChecksum { actual: usize, expected: usize },
    TrailingBytes { actual: usize, expected: usize },
    UnknownPacketType(u8),
    InvalidChecksum,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PayloadTooLarge { len } => write!(f, "payload is too large: {len} bytes"),
            Self::TruncatedHeader { actual, expected } => {
                write!(
                    f,
                    "truncated header: got {actual} bytes, expected {expected}"
                )
            }
            Self::TruncatedPayload { actual, expected } => {
                write!(
                    f,
                    "truncated payload: got {actual} bytes, expected {expected}"
                )
            }
            Self::TruncatedChecksum { actual, expected } => {
                write!(
                    f,
                    "truncated checksum: got {actual} bytes, expected {expected}"
                )
            }
            Self::TrailingBytes { actual, expected } => {
                write!(f, "trailing bytes: got {actual} bytes, expected {expected}")
            }
            Self::UnknownPacketType(packet_type) => {
                write!(f, "unknown packet type: {packet_type:#04x}")
            }
            Self::InvalidChecksum => write!(f, "invalid packet checksum"),
        }
    }
}

impl std::error::Error for ProtocolError {}
