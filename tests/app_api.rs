use std::path::PathBuf;

use plenum::app::{
    AppError, BenchmarkRequest, CorePermissions, PermissionKind, PlenumCore,
    ReceiveUnifiedRequest, SendUnifiedRequest, TransferOptions, UnifiedSharePayload,
};

#[test]
fn benchmark_api_runs_through_high_level_core() {
    let mut core = PlenumCore::new();
    let mut events = Vec::new();

    let summary = core
        .benchmark(
            BenchmarkRequest {
                size_mb: 1,
                iterations: 1,
                latency_ticks: 1,
                options: TransferOptions::default(),
            },
            &mut |event| events.push(event),
        )
        .expect("benchmark should succeed");

    assert_eq!(summary.size_mb, 1);
    assert_eq!(summary.iterations.len(), 1);
    assert!(!events.is_empty());
}

#[test]
fn send_file_unified_requires_file_system_read_permission() {
    let mut core = PlenumCore::new();
    let mut sink = |_event| {};

    let err = core
        .send_file_unified(
            SendUnifiedRequest {
                file_path: PathBuf::from("test.txt"),
                file_paths: vec![PathBuf::from("test.txt")],
                payload: UnifiedSharePayload::new("ABC123XYZ"),
                relay_server_url: "wss://relay.example.com/ws".into(),
                my_peer_id: "peer-sender".into(),
                ice_servers: vec![],
                connect_timeout_secs: 10,
                device_name: None,
                permissions: CorePermissions {
                    local_network: true,
                    file_system_read: false,
                    file_system_write: true,
                    background_transfer: false,
                },
                options: TransferOptions::default(),
            },
            &mut sink,
        )
        .expect_err("send should require file system read permission");

    match err {
        AppError::PermissionDenied { permission, .. } => {
            assert_eq!(permission, PermissionKind::FileSystemRead);
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn receive_file_unified_requires_file_system_write_permission() {
    let mut core = PlenumCore::new();
    let mut sink = |_event| {};

    let err = core
        .receive_file_unified(
            ReceiveUnifiedRequest {
                output_dir: PathBuf::from("."),
                relay_server_url: "wss://relay.example.com/ws".into(),
                session_id: "ABC123XYZ".into(),
                my_peer_id: "peer-receiver".into(),
                ice_servers: vec![],
                connect_timeout_secs: 10,
                port: 0,
                require_pin: false,
                auto_accept: true,
                device_name: None,
                permissions: CorePermissions {
                    local_network: true,
                    file_system_read: true,
                    file_system_write: false,
                    background_transfer: false,
                },
                options: TransferOptions::default(),
            },
            &mut sink,
        )
        .expect_err("receive should require file system write permission");

    match err {
        AppError::PermissionDenied { permission, .. } => {
            assert_eq!(permission, PermissionKind::FileSystemWrite);
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn receive_file_unified_requires_local_network_permission() {
    let mut core = PlenumCore::new();
    let mut sink = |_event| {};

    let err = core
        .receive_file_unified(
            ReceiveUnifiedRequest {
                output_dir: PathBuf::from("."),
                relay_server_url: "wss://relay.example.com/ws".into(),
                session_id: "ABC123XYZ".into(),
                my_peer_id: "peer-receiver".into(),
                ice_servers: vec![],
                connect_timeout_secs: 10,
                port: 0,
                require_pin: false,
                auto_accept: true,
                device_name: None,
                permissions: CorePermissions {
                    local_network: false,
                    file_system_read: true,
                    file_system_write: true,
                    background_transfer: false,
                },
                options: TransferOptions::default(),
            },
            &mut sink,
        )
        .expect_err("receive should require local network permission");

    match err {
        AppError::PermissionDenied { permission, .. } => {
            assert_eq!(permission, PermissionKind::LocalNetwork);
        }
        other => panic!("unexpected error: {other}"),
    }
}
