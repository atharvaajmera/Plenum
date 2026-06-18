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
    Io { message: String },
    Json(String),
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
            Self::Io { message } => write!(f, "stream I/O error: {message}"),
            Self::Json(message) => write!(f, "stream json error: {message}"),
        }
    }
}

impl std::error::Error for StreamError {}

impl From<std::io::Error> for StreamError {
    fn from(error: std::io::Error) -> Self {
        Self::Io {
            message: error.to_string(),
        }
    }
}

impl From<serde_json::Error> for StreamError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error.to_string())
    }
}
