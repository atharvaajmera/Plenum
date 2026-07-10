//! RTC (WebRTC transport) errors.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RtcError {
    Signaling(String),
    PeerConnection(String),
    Timeout,
    WebSocket(String),
}

impl fmt::Display for RtcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Signaling(message) => write!(f, "rtc signaling error: {message}"),
            Self::PeerConnection(message) => write!(f, "rtc peer connection error: {message}"),
            Self::Timeout => write!(f, "rtc connection attempt timed out"),
            Self::WebSocket(message) => write!(f, "rtc websocket error: {message}"),
        }
    }
}

impl std::error::Error for RtcError {}
