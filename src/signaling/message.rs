//! Signaling message types and JSON wire format.

use serde::{Deserialize, Serialize};

use crate::signaling::{NatTraversalConfig, SignalingError};

/// JSON signaling messages intended to be carried over WebSocket or another
/// text-capable signaling transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalMessage {
    JoinSession {
        peer_id: String,
        session_id: String,
    },
    LeaveSession {
        peer_id: String,
        session_id: String,
    },
    Offer {
        session_id: String,
        from_peer_id: String,
        to_peer_id: String,
        sdp: String,
        nat: Option<NatTraversalConfig>,
    },
    Answer {
        session_id: String,
        from_peer_id: String,
        to_peer_id: String,
        sdp: String,
    },
    IceCandidate {
        session_id: String,
        from_peer_id: String,
        to_peer_id: String,
        candidate: String,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u16>,
    },
    PeerJoined {
        peer_id: String,
        session_id: String,
    },
    PeerLeft {
        peer_id: String,
        session_id: String,
    },
    Error {
        message: String,
    },
}

impl SignalMessage {
    pub fn to_json(&self) -> Result<String, SignalingError> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self, SignalingError> {
        Ok(serde_json::from_str(json)?)
    }
}
