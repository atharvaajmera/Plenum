use flutter_rust_bridge::frb;
use plenum::app::engine::PlenumCore;
use plenum::app::types::{
    CorePermissions, DiscoverRequest, ReceiveRequest, SendRequest, TransferOptions,
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
