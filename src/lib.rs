//! Plenum core library.
//!
//! This crate contains the protocol, stream, transport, and discovery layers
//! for the Plenum peer-to-peer file transfer engine.

pub mod app;
pub mod discovery;
pub mod flow;
pub mod protocol;
pub mod rtc;
pub mod security;
pub mod signaling;
pub mod stream;
pub mod transport;
