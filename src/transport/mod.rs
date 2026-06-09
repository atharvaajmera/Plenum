//! Transport abstraction and implementations.

pub mod error;
pub mod memory;

pub use error::TransportError;
pub use memory::{MemoryTransport, MemoryTransportConfig};

pub type TransportResult<T> = Result<T, TransportError>;

/// Minimal byte transport abstraction used by higher-level transfer logic.
pub trait Transport {
    fn send(&mut self, bytes: &[u8]) -> TransportResult<()>;
    fn recv(&mut self) -> TransportResult<Option<Vec<u8>>>;
    fn close(&mut self) -> TransportResult<()>;
}
