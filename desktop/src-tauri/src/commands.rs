use plenum::app::engine::PlenumCore;
use plenum::app::types::{
    generate_peer_id, generate_room_code, DiscoverRequest, DiscoverySummary, PlenumEvent,
    ReceiveRemoteRequest, ReceiveRequest, SendRemoteRequest, SendRequest, TransferSummary,
};
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
