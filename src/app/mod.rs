//! App-facing integration layer for desktop and mobile shells.
//!
//! This module provides a stable, high-level API over the lower-level protocol,
//! transport, discovery, signaling, and security layers so UI shells such as
//! Tauri and Flutter can call into the Rust core without depending on internal
//! packet or stream-management details.

pub mod engine;
pub mod error;
pub mod types;

pub use engine::AetherCore;
pub use error::AppError;
pub use types::{
    AetherEvent, BenchmarkEvent, BenchmarkIterationSummary, BenchmarkRequest, BenchmarkSummary,
    ConnectionState, CorePermissions, DiscoverRequest, DiscoveryEvent, DiscoverySummary, EventSink,
    LogLevel, PermissionKind, ReceiveRequest, SendRequest, TransferDirection, TransferEvent,
    TransferOptions, TransferSummary,
};
