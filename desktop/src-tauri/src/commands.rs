use tauri::{AppHandle, Emitter};
use aether::app::engine::AetherCore;
use aether::app::types::{
    AetherEvent, DiscoverRequest, DiscoverySummary, ReceiveRequest, SendRequest,
    TransferSummary,
};

#[tauri::command]
pub async fn send_file_command(
    app: AppHandle,
    request: SendRequest,
) -> Result<TransferSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = AetherCore::new();
        let mut sink = |event: AetherEvent| {
            let _ = app.emit("aether-event", event);
        };
        core.send_file(request, &mut sink).map_err(|e| e.to_string())
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
        let mut core = AetherCore::new();
        let mut sink = |event: AetherEvent| {
            let _ = app.emit("aether-event", event);
        };
        core.receive_file(request, &mut sink).map_err(|e| e.to_string())
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
        let mut core = AetherCore::new();
        let mut sink = |event: AetherEvent| {
            let _ = app.emit("aether-event", event);
        };
        core.discover_peer(request, &mut sink).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
