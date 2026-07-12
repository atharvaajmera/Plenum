use plenum::app::engine::PlenumCore;
use plenum::app::types::{
    generate_peer_id, generate_room_code, DiscoverRequest, DiscoverySummary, PlenumEvent,
    ReceiveRemoteRequest, ReceiveRequest, SendRemoteRequest, SendRequest, TransferSummary,
};
use plenum::signaling::IceServer;
use tauri::{AppHandle, Emitter};

#[tauri::command]
pub async fn send_file_command(
    app: AppHandle,
    request: SendRequest,
) -> Result<TransferSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = PlenumCore::new();
        let mut sink = |event: PlenumEvent| {
            let _ = app.emit("plenum-event", event);
        };
        core.send_file(request, &mut sink)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn receive_file_command(
    app: AppHandle,
    request: ReceiveRequest,
) -> Result<TransferSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = PlenumCore::new();
        let mut sink = |event: PlenumEvent| {
            let _ = app.emit("plenum-event", event);
        };
        core.receive_file(request, &mut sink)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn discover_peers_command(
    app: AppHandle,
    request: DiscoverRequest,
) -> Result<DiscoverySummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = PlenumCore::new();
        let mut sink = |event: PlenumEvent| {
            let _ = app.emit("plenum-event", event);
        };
        core.discover_peer(request, &mut sink)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn send_file_remote_command(
    app: AppHandle,
    request: SendRemoteRequest,
) -> Result<TransferSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = PlenumCore::new();
        let mut sink = |event: PlenumEvent| {
            let _ = app.emit("plenum-event", event);
        };
        core.send_file_remote(request, &mut sink)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn receive_file_remote_command(
    app: AppHandle,
    request: ReceiveRemoteRequest,
) -> Result<TransferSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = PlenumCore::new();
        let mut sink = |event: PlenumEvent| {
            let _ = app.emit("plenum-event", event);
        };
        core.receive_file_remote(request, &mut sink)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Generates a display-ready room code for internet transfers, without
/// blocking on a relay-server connection (so the receive UI can show it
/// immediately).
#[tauri::command]
pub fn generate_room_code_command() -> String {
    generate_room_code()
}

/// Generates a random per-connection peer id for internet transfers.
#[tauri::command]
pub fn generate_peer_id_command() -> String {
    generate_peer_id()
}

#[derive(serde::Deserialize)]
struct TurnCredentialsResponse {
    username: String,
    credential: String,
    urls: Vec<String>,
}

/// Fetches short-lived TURN credentials from the relay's `/turn-credentials`
/// endpoint and returns them as a ready-to-use `IceServer`. The relay URL is a
/// `wss://.../ws` signaling URL; the credentials endpoint is derived from it by
/// switching to https and replacing the path.
///
/// Returns `Ok(None)` (rather than an error) when the relay has no TURN
/// configured or is unreachable, so callers can fall back to STUN-only.
#[tauri::command]
pub async fn fetch_turn_credentials_command(
    relay_server_url: String,
    peer_id: String,
) -> Result<Option<IceServer>, String> {
    let trimmed = relay_server_url.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let mut url = reqwest::Url::parse(trimmed).map_err(|e| format!("invalid relay url: {e}"))?;
    let https_scheme = match url.scheme() {
        "wss" | "https" => "https",
        "ws" | "http" => "http",
        _ => "https",
    };
    url.set_scheme(https_scheme)
        .map_err(|_| "failed to set url scheme".to_string())?;
    url.set_path("/turn-credentials");
    url.set_query(Some(&format!("peer_id={peer_id}")));

    let resp = match reqwest::Client::new().get(url).send().await {
        Ok(resp) => resp,
        // Network failure: fall back to STUN-only rather than aborting.
        Err(_) => return Ok(None),
    };
    if !resp.status().is_success() {
        return Ok(None);
    }

    match resp.json::<TurnCredentialsResponse>().await {
        Ok(creds) if !creds.urls.is_empty() => Ok(Some(IceServer {
            urls: creds.urls,
            username: Some(creds.username),
            credential: Some(creds.credential),
        })),
        _ => Ok(None),
    }
}
