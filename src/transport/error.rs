//! Transport-level errors.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    Closed,
    BufferFull { capacity: usize, requested: usize },
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => write!(f, "transport is closed"),
            Self::BufferFull {
                capacity,
                requested,
            } => write!(
                f,
                "transport buffer is full: capacity {capacity}, requested {requested}"
            ),
        }
    }
}

impl std::error::Error for TransportError {}
