//! Flow-control errors.

use std::fmt;

use crate::protocol::{PacketType, ProtocolError};
use crate::transport::TransportError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowError {
    InvalidWindowSize,
    DuplicateSequence { sequence_no: u32 },
    UnknownSequence { sequence_no: u32 },
    UnexpectedPacketType { actual: PacketType },
    Protocol(ProtocolError),
    Transport(TransportError),
}

impl fmt::Display for FlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWindowSize => write!(f, "window size must be greater than zero"),
            Self::DuplicateSequence { sequence_no } => {
                write!(f, "duplicate sequence number: {sequence_no}")
            }
            Self::UnknownSequence { sequence_no } => {
                write!(f, "unknown sequence number: {sequence_no}")
            }
            Self::UnexpectedPacketType { actual } => {
                write!(f, "unexpected packet type: {actual:?}")
            }
            Self::Protocol(error) => write!(f, "protocol error: {error}"),
            Self::Transport(error) => write!(f, "transport error: {error}"),
        }
    }
}

impl std::error::Error for FlowError {}

impl From<ProtocolError> for FlowError {
    fn from(error: ProtocolError) -> Self {
        Self::Protocol(error)
    }
}

impl From<TransportError> for FlowError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}
