use flutter_rust_bridge::frb;
use plenum::app::engine::PlenumCore;
use plenum::app::types::{
    generate_peer_id, generate_room_code, CorePermissions, DiscoverRequest, ReceiveRemoteRequest,
    ReceiveRequest, SendRemoteRequest, SendRequest, TransferOptions,
};
use crate::frb_generated::StreamSink;
use std::path::PathBuf;

#[frb(sync)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}

pub fn start_discovery(sink: StreamSink<String>, timeout_secs: u64) -> anyhow::Result<()> {
    let req = DiscoverRequest {
        token: None,
        timeout_secs,
        permissions: CorePermissions::mobile_defaults(),
    };

    let mut core = PlenumCore::new();
    let mut sink_wrapper = |event: plenum::app::types::PlenumEvent| {
        if let Ok(json) = serde_json::to_string(&event) {
            let _ = sink.add(json);
        }
    };

    match core.discover_peer(req, &mut sink_wrapper) {
        Ok(_) => Ok(()),
        Err(e) => Err(anyhow::anyhow!("Discovery failed: {}", e)),
    }
}

pub fn start_send(
    sink: StreamSink<String>,
    file_path: String,
    peer_address: String,
    optional_pin: Option<String>,
) -> anyhow::Result<String> {
    let req = SendRequest {
        file_path: PathBuf::from(file_path),
        address: Some(peer_address),
        discovery_token: optional_pin,
        permissions: CorePermissions::mobile_defaults(),
        options: TransferOptions::default(),
    };

    let mut core = PlenumCore::new();
    let mut sink_wrapper = |event: plenum::app::types::PlenumEvent| {
        if let Ok(json) = serde_json::to_string(&event) {
            let _ = sink.add(json);
        }
    };

    match core.send_file(req, &mut sink_wrapper) {
        Ok(summary) => Ok(serde_json::to_string(&summary).unwrap_or_default()),
        Err(e) => Err(anyhow::anyhow!("Send failed: {}", e)),
    }
}

pub fn start_receive(
    sink: StreamSink<String>,
    output_dir: String,
    port: u16,
    announce: bool,
) -> anyhow::Result<String> {
    let req = ReceiveRequest {
        port,
        output_dir: PathBuf::from(output_dir),
        announce_on_lan: announce,
        permissions: CorePermissions::mobile_defaults(),
        options: TransferOptions::default(),
    };

    let mut core = PlenumCore::new();
    let mut sink_wrapper = |event: plenum::app::types::PlenumEvent| {
        if let Ok(json) = serde_json::to_string(&event) {
            let _ = sink.add(json);
        }
    };

    match core.receive_file(req, &mut sink_wrapper) {
        Ok(summary) => Ok(serde_json::to_string(&summary).unwrap_or_default()),
        Err(e) => Err(anyhow::anyhow!("Receive failed: {}", e)),
    }
}

/// Sends a file over the internet via a relay/signaling server, negotiating a
/// WebRTC data channel. Mirrors `start_send`, but for internet (non-LAN) transfers.
///
/// `ice_servers_json` is a JSON-encoded array of `{ urls: string[], username?:
/// string, credential?: string }`, matching `plenum::signaling::IceServer`.
/// Passed as JSON (rather than a plain FFI struct) because `IceServer` is
/// defined in the `plenum` crate, so flutter_rust_bridge would otherwise
/// generate it as an opaque handle Dart cannot construct field-by-field.
pub fn start_send_remote(
    sink: StreamSink<String>,
    file_path: String,
    relay_server_url: String,
    session_id: String,
    my_peer_id: String,
    ice_servers_json: String,
    connect_timeout_secs: u64,
) -> anyhow::Result<String> {
    let ice_servers = serde_json::from_str(&ice_servers_json)
        .map_err(|e| anyhow::anyhow!("Invalid ice_servers_json: {}", e))?;

    let req = SendRemoteRequest {
        file_path: PathBuf::from(file_path),
        relay_server_url,
        session_id,
        my_peer_id,
        ice_servers,
        connect_timeout_secs,
        permissions: CorePermissions::mobile_defaults(),
        options: TransferOptions::default(),
    };

    let mut core = PlenumCore::new();
    let mut sink_wrapper = |event: plenum::app::types::PlenumEvent| {
        if let Ok(json) = serde_json::to_string(&event) {
            let _ = sink.add(json);
        }
    };

    match core.send_file_remote(req, &mut sink_wrapper) {
        Ok(summary) => Ok(serde_json::to_string(&summary).unwrap_or_default()),
        Err(e) => Err(anyhow::anyhow!("Send failed: {}", e)),
    }
}

/// Receives a file over the internet via a relay/signaling server, negotiating
/// a WebRTC data channel. Mirrors `start_receive`, but for internet (non-LAN) transfers.
///
/// See [`start_send_remote`] for the `ice_servers_json` shape/rationale.
pub fn start_receive_remote(
    sink: StreamSink<String>,
    output_dir: String,
    relay_server_url: String,
    session_id: String,
    my_peer_id: String,
    ice_servers_json: String,
    connect_timeout_secs: u64,
) -> anyhow::Result<String> {
    let ice_servers = serde_json::from_str(&ice_servers_json)
        .map_err(|e| anyhow::anyhow!("Invalid ice_servers_json: {}", e))?;

    let req = ReceiveRemoteRequest {
        output_dir: PathBuf::from(output_dir),
        relay_server_url,
        session_id,
        my_peer_id,
        ice_servers,
        connect_timeout_secs,
        permissions: CorePermissions::mobile_defaults(),
        options: TransferOptions::default(),
    };

    let mut core = PlenumCore::new();
    let mut sink_wrapper = |event: plenum::app::types::PlenumEvent| {
        if let Ok(json) = serde_json::to_string(&event) {
            let _ = sink.add(json);
        }
    };

    match core.receive_file_remote(req, &mut sink_wrapper) {
        Ok(summary) => Ok(serde_json::to_string(&summary).unwrap_or_default()),
        Err(e) => Err(anyhow::anyhow!("Receive failed: {}", e)),
    }
}

/// Generates a display-ready room code for internet transfers, without
/// blocking on a relay-server connection (so the receive UI can show it
/// immediately).
#[frb(sync)]
pub fn generate_room_code_sync() -> String {
    generate_room_code()
}

/// Generates a random per-connection peer id for internet transfers.
#[frb(sync)]
pub fn generate_peer_id_sync() -> String {
    generate_peer_id()
}
