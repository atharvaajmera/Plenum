//! Stream chunking and reassembly errors.

use std::fmt;

use crate::protocol::PacketType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamError {
    InvalidChunkSize,
    TooManyChunks { chunks: usize },
    DuplicateSequence { sequence_no: u32 },
    MissingSequence { sequence_no: u32 },
    UnexpectedPacketType { actual: PacketType },
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChunkSize => write!(f, "chunk size must be greater than zero"),
            Self::TooManyChunks { chunks } => {
                write!(f, "too many chunks for u32 sequence numbers: {chunks}")
            }
            Self::DuplicateSequence { sequence_no } => {
                write!(f, "duplicate packet sequence number: {sequence_no}")
            }
            Self::MissingSequence { sequence_no } => {
                write!(f, "missing packet sequence number: {sequence_no}")
            }
            Self::UnexpectedPacketType { actual } => {
                write!(f, "expected data packet, got {actual:?}")
            }
        }
    }
}

impl std::error::Error for StreamError {}
