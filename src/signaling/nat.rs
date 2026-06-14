//! NAT traversal configuration shared during remote negotiation.

use serde::{Deserialize, Serialize};

/// ICE server configuration that can represent either STUN or TURN endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IceServer {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

impl IceServer {
    pub fn new(urls: impl Into<Vec<String>>) -> Self {
        Self {
            urls: urls.into(),
            username: None,
            credential: None,
        }
    }

    pub fn with_credentials(
        urls: impl Into<Vec<String>>,
        username: impl Into<String>,
        credential: impl Into<String>,
    ) -> Self {
        Self {
            urls: urls.into(),
            username: Some(username.into()),
            credential: Some(credential.into()),
        }
    }
}

/// Optional STUN/TURN configuration that a signaling server or client can
/// advertise while negotiating a remote connection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NatTraversalConfig {
    pub ice_servers: Vec<IceServer>,
}

impl NatTraversalConfig {
    pub fn new(ice_servers: impl Into<Vec<IceServer>>) -> Self {
        Self {
            ice_servers: ice_servers.into(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ice_servers.is_empty()
    }
}
