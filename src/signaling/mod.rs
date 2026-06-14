//! Remote signaling and NAT traversal support.
//!
//! This module is intentionally separate from the transfer protocol. It defines
//! the signaling messages used to negotiate remote peer connections and a small
//! in-memory signaling state machine that a WebSocket server can use to route
//! messages between peers.

pub mod error;
pub mod message;
pub mod nat;
pub mod state;

pub use error::SignalingError;
pub use message::SignalMessage;
pub use nat::{IceServer, NatTraversalConfig};
pub use state::{RoutedSignal, SignalingState};
