//! WebRTC-backed remote (internet) transport.
//!
//! This module implements a `crate::transport::Transport` over a WebRTC data
//! channel, negotiated via a relay/signaling WebSocket using the existing
//! `crate::signaling` message types. See `src/rtc/transport.rs` for the
//! `RtcTransport` type and `src/rtc/signaling_client.rs` for the offer/answer/ICE
//! negotiation event loop.

pub mod config;
pub mod error;
pub mod runtime;
pub mod signaling_client;
pub mod transport;

pub use error::RtcError;
pub use transport::RtcTransport;
