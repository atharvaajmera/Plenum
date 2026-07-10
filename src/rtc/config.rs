//! Mapping from `crate::signaling` NAT-traversal types to webrtc-rs configuration types.

use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;

use crate::signaling::{IceServer, NatTraversalConfig};

/// Convert a single `crate::signaling::IceServer` into the webrtc-rs `RTCIceServer`.
///
/// `RTCIceServer::username`/`credential` are plain (non-`Option`) `String`s, so
/// `None` maps to an empty string, matching webrtc-rs's own `Default` behavior.
pub fn to_rtc_ice_server(ice_server: &IceServer) -> RTCIceServer {
    RTCIceServer {
        urls: ice_server.urls.clone(),
        username: ice_server.username.clone().unwrap_or_default(),
        credential: ice_server.credential.clone().unwrap_or_default(),
        ..Default::default()
    }
}

/// Build a webrtc-rs `RTCConfiguration` from a list of `crate::signaling::IceServer`s.
pub fn to_rtc_configuration(ice_servers: &[IceServer]) -> RTCConfiguration {
    RTCConfiguration {
        ice_servers: ice_servers.iter().map(to_rtc_ice_server).collect(),
        ..Default::default()
    }
}

/// Build a webrtc-rs `RTCConfiguration` from a `NatTraversalConfig` (as carried
/// inside a `SignalMessage::Offer`).
pub fn from_nat_traversal_config(nat: &NatTraversalConfig) -> RTCConfiguration {
    to_rtc_configuration(&nat.ice_servers)
}
