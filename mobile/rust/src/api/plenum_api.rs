use crate::frb_generated::StreamSink;
use flutter_rust_bridge::frb;
use plenum::app::AppError;
use plenum::app::engine::PlenumCore;
use plenum::app::types::{
    generate_peer_id, generate_room_code, get_local_ip_addresses, CorePermissions,
    ReceiveUnifiedRequest, SendUnifiedRequest, SessionControl, TransferOptions,
    UnifiedSharePayload,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Live transfer sessions, keyed by the Dart-supplied session token. The
/// engine calls are blocking run-to-completion, so Dart needs an out-of-band
/// handle to cancel a transfer or answer an incoming-transfer prompt; this
/// registry is that side channel.
fn sessions() -> &'static Mutex<HashMap<String, SessionControl>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, SessionControl>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_session(token: &str, control: SessionControl) {
    if token.is_empty() {
        return;
    }
    sessions()
        .lock()
        .unwrap()
        .insert(token.to_string(), control);
}

fn unregister_session(token: &str) {
    if token.is_empty() {
        return;
    }
    sessions().lock().unwrap().remove(token);
}

/// Requests cancellation of the transfer running under `session_token`.
/// The blocking transfer loop notices the flag within tens of milliseconds,
/// sends a `Close` to the peer, emits a `Cancelled` event, and returns.
#[frb(sync)]
pub fn cancel_session(session_token: String) {
    if let Some(control) = sessions().lock().unwrap().get(&session_token) {
        control.cancel();
    }
}

/// Answers the accept gate for an incoming transfer (`IncomingRequest` event)
/// on the session running under `session_token`.
#[frb(sync)]
pub fn respond_to_incoming(session_token: String, accept: bool) {
    if let Some(control) = sessions().lock().unwrap().get(&session_token) {
        if accept {
            control.accept();
        } else {
            control.decline();
        }
    }
}

#[frb(sync)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}

fn parse_target_paths(file_path: &str) -> (PathBuf, Vec<PathBuf>) {
    let trimmed = file_path.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        if let Ok(list) = serde_json::from_str::<Vec<String>>(trimmed) {
            let paths: Vec<PathBuf> = list.into_iter().map(PathBuf::from).collect();
            let first = paths.first().cloned().unwrap_or_default();
            return (first, paths);
        }
    }
    let p = PathBuf::from(file_path);
    (p.clone(), vec![p])
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

/// Retrieves the active, non-loopback local IPv4 addresses of this device.
#[frb(sync)]
pub fn get_local_ips_sync() -> Vec<String> {
    get_local_ip_addresses()
}

/// Starts an autonomous unified send transfer. Probes LAN endpoints first (Happy Eyeballs),
/// seamlessly falling back to WebRTC remote transfer if LAN is unreachable or times out.
pub fn start_send_unified(
    sink: StreamSink<String>,
    session_token: String,
    file_path: String,
    payload_uri_or_json: String,
    relay_server_url: String,
    my_peer_id: String,
    ice_servers_json: String,
    connect_timeout_secs: u64,
    device_name: Option<String>,
) -> anyhow::Result<String> {
    let ice_servers = serde_json::from_str(&ice_servers_json)
        .map_err(|e| anyhow::anyhow!("Invalid ice_servers_json: {}", e))?;

    let payload = if let Ok(parsed) = UnifiedSharePayload::parse_uri(&payload_uri_or_json) {
        parsed
    } else {
        serde_json::from_str(&payload_uri_or_json)
            .map_err(|e| anyhow::anyhow!("Invalid unified payload: {}", e))?
    };

    let (primary_path, paths) = parse_target_paths(&file_path);
    let req = SendUnifiedRequest {
        file_path: primary_path,
        file_paths: paths,
        payload,
        relay_server_url,
        my_peer_id,
        ice_servers,
        connect_timeout_secs,
        device_name,
        permissions: CorePermissions::mobile_defaults(),
        options: TransferOptions::default(),
    };

    let mut core = PlenumCore::new();
    register_session(&session_token, core.control());
    let mut sink_wrapper = |event: plenum::app::types::PlenumEvent| {
        if let Ok(json) = serde_json::to_string(&event) {
            let _ = sink.add(json);
        }
    };

    let result = core.send_file_unified(req, &mut sink_wrapper);
    unregister_session(&session_token);
    match result {
        Ok(summary) => Ok(serde_json::to_string(&summary).unwrap_or_default()),
        Err(AppError::Cancelled) => Ok(String::new()),
        Err(e) => Err(anyhow::anyhow!("Unified send failed: {}", e)),
    }
}

/// Starts an autonomous unified receive transfer. Listens concurrently on a local TCP port
/// and on the WebRTC signaling server. The first viable connection claims the transfer.
pub fn start_receive_unified(
    sink: StreamSink<String>,
    session_token: String,
    output_dir: String,
    relay_server_url: String,
    session_id: String,
    my_peer_id: String,
    ice_servers_json: String,
    connect_timeout_secs: u64,
    port: u16,
    require_pin: bool,
    auto_accept: bool,
    device_name: Option<String>,
) -> anyhow::Result<String> {
    let ice_servers = serde_json::from_str(&ice_servers_json)
        .map_err(|e| anyhow::anyhow!("Invalid ice_servers_json: {}", e))?;

    let req = ReceiveUnifiedRequest {
        output_dir: PathBuf::from(output_dir),
        relay_server_url,
        session_id,
        my_peer_id,
        ice_servers,
        connect_timeout_secs,
        port,
        require_pin,
        auto_accept,
        device_name,
        permissions: CorePermissions::mobile_defaults(),
        options: TransferOptions::default(),
    };

    let mut core = PlenumCore::new();
    register_session(&session_token, core.control());
    let mut sink_wrapper = |event: plenum::app::types::PlenumEvent| {
        if let Ok(json) = serde_json::to_string(&event) {
            let _ = sink.add(json);
        }
    };

    let result = core.receive_file_unified(req, &mut sink_wrapper);
    unregister_session(&session_token);
    match result {
        Ok(summary) => Ok(serde_json::to_string(&summary).unwrap_or_default()),
        Err(AppError::Cancelled) => Ok(String::new()),
        Err(e) => Err(anyhow::anyhow!("Unified receive failed: {}", e)),
    }
}
