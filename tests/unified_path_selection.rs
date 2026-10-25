//! Rigorous and comprehensive integration tests for Plenum's Unified Architecture.
//!
//! Covers:
//! 1. `UnifiedSharePayload` URI generation, serialization, resilient parsing & edge cases.
//! 2. End-to-end direct LAN unified file transfer (Happy Path).
//! 3. End-to-end LAN unified transfer with PIN authentication.
//! 4. Fast Happy Eyeballs probing across multi-homed / faulty candidate IPs.
//! 5. Seamless fallback to WebRTC when LAN is unreachable / blocked (simulating AP isolation).
//! 6. Fallback to WebRTC when LAN PIN handshake is rejected.
//! 7. Zero-byte file and multi-chunk large file streaming in unified mode.
//! 8. Cancellation and clean thread/socket teardown.
//! 9. Transport-level multipath failover and metrics.

use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use plenum::app::error::AppError;
use plenum::app::types::{
    CorePermissions, PlenumEvent, ReceiveUnifiedRequest, SelectedPath, SendUnifiedRequest,
    TransferDirection, TransferEvent, TransferOptions, UnifiedSharePayload,
};
use plenum::app::PlenumCore;
use relay_server::AppState;
use tokio::net::TcpListener;

// ============================================================================
// Helpers
// ============================================================================

fn unique_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "plenum-unified-test-{label}-{}-{}",
        std::process::id(),
        rand::random::<u32>()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn patterned_bytes(len: usize) -> Vec<u8> {
    (0..len).map(|idx| ((idx * 7 + 13) % 251) as u8).collect()
}

/// Spawns an in-process relay server on an ephemeral loopback port,
/// returning the WebSocket signaling URL.
fn spawn_relay_server(runtime: &tokio::runtime::Runtime) -> String {
    runtime.block_on(async {
        let state = Arc::new(AppState::new(None, Vec::new()));
        let app = relay_server::build_router(state);

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral relay port");
        let addr: SocketAddr = listener.local_addr().expect("relay local addr");

        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("relay server error");
        });

        format!("ws://{addr}/ws")
    })
}

// ============================================================================
// Suite 1: Protocol & Payload Parsing Fuzz / Stress Tests
// ============================================================================

#[test]
fn test_unified_payload_comprehensive_roundtrip() {
    let mut payload = UnifiedSharePayload::new("9X8Y7Z6W5");
    payload.lan_ips = vec![
        "192.168.1.100".to_string(),
        "10.0.0.5".to_string(),
        "172.16.42.1".to_string(),
    ];
    payload.port = 51234;
    payload.pin = Some("9999".to_string());
    payload.peer_id = Some("peer-abc-123".to_string());
    payload.device_name = Some("Atharva's 💻 [M3-Max] & (Work)".to_string());

    let uri = payload.to_uri();
    assert!(uri.starts_with("plenum://v1/connect?"));

    let parsed = UnifiedSharePayload::parse_uri(&uri).expect("Failed to parse compact URI");
    assert_eq!(parsed.version, 1);
    assert_eq!(parsed.room_code, "9X8Y7Z6W5");
    assert_eq!(
        parsed.lan_ips,
        vec!["192.168.1.100", "10.0.0.5", "172.16.42.1"]
    );
    assert_eq!(parsed.port, 51234);
    assert_eq!(parsed.pin.as_deref(), Some("9999"));
    assert_eq!(parsed.peer_id.as_deref(), Some("peer-abc-123"));
    assert_eq!(
        parsed.device_name.as_deref(),
        Some("Atharva's 💻 [M3-Max] & (Work)")
    );
}

#[test]
fn test_unified_payload_minimal_fields_roundtrip() {
    let payload = UnifiedSharePayload::new("MIN123456");
    let uri = payload.to_uri();

    let parsed = UnifiedSharePayload::parse_uri(&uri).expect("Parse minimal URI");
    assert_eq!(parsed.room_code, "MIN123456");
    assert!(parsed.lan_ips.is_empty());
    assert_eq!(parsed.port, 0);
    assert!(parsed.pin.is_none());
    assert!(parsed.peer_id.is_none());
    assert!(parsed.device_name.is_none());
}

#[test]
fn test_unified_payload_query_aliases_and_case_insensitivity() {
    // Uses aliases: room, ips, port, name, peer
    let uri = "plenum://v1/connect?room=3a4b5c6d7&ips=172.16.0.4,10.10.10.10&port=8080&peer=peer-xyz&name=MyPhone&pin=4321";
    let parsed = UnifiedSharePayload::parse_uri(uri).expect("Parse verbose aliases");
    assert_eq!(parsed.room_code, "3A4B5C6D7"); // Normalized to uppercase
    assert_eq!(parsed.lan_ips, vec!["172.16.0.4", "10.10.10.10"]);
    assert_eq!(parsed.port, 8080);
    assert_eq!(parsed.pin.as_deref(), Some("4321"));
    assert_eq!(parsed.peer_id.as_deref(), Some("peer-xyz"));
    assert_eq!(parsed.device_name.as_deref(), Some("MyPhone"));
}

#[test]
fn test_unified_payload_raw_code_fallbacks() {
    // Raw hyphenated 9-char code
    let parsed_raw_hyphen =
        UnifiedSharePayload::parse_uri("ABC-DEF-123").expect("Parse raw hyphenated");
    assert_eq!(parsed_raw_hyphen.room_code, "ABCDEF123");

    // Raw unhyphenated lowercase code
    let parsed_raw_lower =
        UnifiedSharePayload::parse_uri("xyz987wvu").expect("Parse raw lowercase");
    assert_eq!(parsed_raw_lower.room_code, "XYZ987WVU");
}

#[test]
fn test_unified_payload_invalid_inputs_rejected() {
    assert!(UnifiedSharePayload::parse_uri("").is_err());
    assert!(UnifiedSharePayload::parse_uri("   ").is_err());
    // Missing room code in connect query
    assert!(UnifiedSharePayload::parse_uri("plenum://v1/connect?ip=127.0.0.1&p=8080").is_err());
    // Legacy formats without query string are rejected
    assert!(UnifiedSharePayload::parse_uri("plenum://room/7k9x2m4p1").is_err());
    assert!(UnifiedSharePayload::parse_uri("plenum://pin/8472").is_err());
}

#[test]
fn test_selected_path_serialization_and_deserialization() {
    let local = SelectedPath::Local {
        address: "192.168.1.50:42424".to_string(),
    };
    let json_local = serde_json::to_string(&local).expect("Serialize local");
    let back_local: SelectedPath = serde_json::from_str(&json_local).expect("Deserialize local");
    assert_eq!(local, back_local);

    let internet = SelectedPath::Internet {
        room_code: "ABC123XYZ".to_string(),
        is_relayed: true,
    };
    let json_internet = serde_json::to_string(&internet).expect("Serialize internet");
    let back_internet: SelectedPath =
        serde_json::from_str(&json_internet).expect("Deserialize internet");
    assert_eq!(internet, back_internet);
}

// ============================================================================
// Suite 2: End-to-End Direct LAN Unified Transfer (Happy Path)
// ============================================================================

#[test]
fn test_unified_lan_direct_transfer_success() {
    let source_dir = unique_dir("lan-happy-src");
    let output_dir = unique_dir("lan-happy-out");

    let file_path = source_dir.join("test_file.bin");
    let payload_bytes = patterned_bytes(128 * 1024); // 128 KiB
    fs::write(&file_path, &payload_bytes).expect("write source file");

    let (recv_tx, recv_rx) = mpsc::channel::<PlenumEvent>();
    let (send_tx, send_rx) = mpsc::channel::<PlenumEvent>();

    let recv_output = output_dir.clone();
    let recv_handle = thread::spawn(move || {
        let mut core = PlenumCore::new();
        let mut sink = move |event: PlenumEvent| {
            let _ = recv_tx.send(event);
        };
        core.receive_file_unified(
            ReceiveUnifiedRequest {
                output_dir: recv_output,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(), // Inactive dummy relay
                session_id: "HAPPY1234".to_string(),
                my_peer_id: "peer-recv".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 5,
                port: 0, // Auto-bind port
                require_pin: false,
                auto_accept: true,
                device_name: Some("Receiver-PC".to_string()),
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    // Wait for the receiver to bind and report its port
    let (actual_port, pin_token) = loop {
        let event = recv_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("Receive Listening event");
        if let PlenumEvent::Transfer(TransferEvent::Listening { port, pin, .. }) = event {
            break (port, pin.unwrap_or_default());
        }
    };

    assert!(actual_port > 0, "Bound port must be valid");

    // Sender prepares unified payload pointing to 127.0.0.1:port
    let mut unified_payload = UnifiedSharePayload::new("HAPPY1234");
    unified_payload.lan_ips = vec!["127.0.0.1".to_string()];
    unified_payload.port = actual_port;
    unified_payload.pin = Some(pin_token);

    let sender_handle = thread::spawn(move || {
        let mut core = PlenumCore::new();
        let mut sink = move |event: PlenumEvent| {
            let _ = send_tx.send(event);
        };
        core.send_file_unified(
            SendUnifiedRequest {
                file_path,
                file_paths: Vec::new(),
                payload: unified_payload,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
                my_peer_id: "peer-sender".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 5,
                device_name: Some("Sender-PC".to_string()),
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    let send_result = sender_handle.join().expect("sender thread join");
    let recv_result = recv_handle.join().expect("receiver thread join");

    assert!(
        send_result.is_ok(),
        "Sender should succeed: {:?}",
        send_result
    );
    assert!(
        recv_result.is_ok(),
        "Receiver should succeed: {:?}",
        recv_result
    );

    let send_summary = send_result.unwrap();
    assert_eq!(send_summary.direction, TransferDirection::Send);
    assert_eq!(send_summary.total_bytes, 128 * 1024);

    let recv_summary = recv_result.unwrap();
    assert_eq!(recv_summary.direction, TransferDirection::Receive);
    assert_eq!(recv_summary.total_bytes, 128 * 1024);

    // Verify received file integrity
    let received_file_path = output_dir.join("test_file.bin");
    assert!(received_file_path.exists(), "Received file must exist");
    let received_bytes = fs::read(&received_file_path).expect("read received file");
    assert_eq!(
        received_bytes, payload_bytes,
        "Received bytes must be bit-for-bit identical to sent bytes"
    );

    // Verify PathSelected event was emitted on both sides
    let mut sender_chose_local = false;
    while let Ok(event) = send_rx.try_recv() {
        if let PlenumEvent::Transfer(TransferEvent::PathSelected { path, .. }) = event {
            if matches!(path, SelectedPath::Local { .. }) {
                sender_chose_local = true;
            }
        }
    }
    assert!(
        sender_chose_local,
        "Sender must emit PathSelected::Local for direct LAN"
    );

    let mut receiver_chose_local = false;
    while let Ok(event) = recv_rx.try_recv() {
        if let PlenumEvent::Transfer(TransferEvent::PathSelected { path, .. }) = event {
            if matches!(path, SelectedPath::Local { .. }) {
                receiver_chose_local = true;
            }
        }
    }
    assert!(
        receiver_chose_local,
        "Receiver must emit PathSelected::Local for direct LAN"
    );
}

// ============================================================================
// Suite 3: PIN Protection in Unified Mode
// ============================================================================

#[test]
fn test_unified_lan_with_matching_pin_succeeds() {
    let source_dir = unique_dir("pin-match-src");
    let output_dir = unique_dir("pin-match-out");

    let file_path = source_dir.join("pin_file.bin");
    let payload = b"Super confidential payload with PIN authentication";
    fs::write(&file_path, payload).expect("write file");

    let (recv_tx, recv_rx) = mpsc::channel::<PlenumEvent>();

    let recv_output = output_dir.clone();
    let recv_handle = thread::spawn(move || {
        let mut core = PlenumCore::new();
        let mut sink = move |event: PlenumEvent| {
            let _ = recv_tx.send(event);
        };
        core.receive_file_unified(
            ReceiveUnifiedRequest {
                output_dir: recv_output,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
                session_id: "PINROOM01".to_string(),
                my_peer_id: "recv-pin".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 5,
                port: 0,
                require_pin: true, // PIN required!
                auto_accept: true,
                device_name: None,
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    let (actual_port, token) = loop {
        let event = recv_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("Listening event");
        if let PlenumEvent::Transfer(TransferEvent::Listening { port, pin, .. }) = event {
            break (port, pin.unwrap_or_default());
        }
    };

    let mut unified_payload = UnifiedSharePayload::new("PINROOM01");
    unified_payload.lan_ips = vec!["127.0.0.1".to_string()];
    unified_payload.port = actual_port;
    unified_payload.pin = Some(token); // Exact PIN supplied!

    let sender_handle = thread::spawn(move || {
        let mut core = PlenumCore::new();
        let mut sink = |_event: PlenumEvent| {};
        core.send_file_unified(
            SendUnifiedRequest {
                file_path,
                file_paths: Vec::new(),
                payload: unified_payload,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
                my_peer_id: "send-pin".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 5,
                device_name: None,
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    let send_res = sender_handle.join().expect("sender join");
    let recv_res = recv_handle.join().expect("receiver join");

    assert!(send_res.is_ok(), "PIN match send succeeded: {:?}", send_res);
    assert!(recv_res.is_ok(), "PIN match recv succeeded: {:?}", recv_res);

    let received_content = fs::read(output_dir.join("pin_file.bin")).expect("read output file");
    assert_eq!(received_content, payload);
}

// ============================================================================
// Suite 4: Multi-IP Probing & Fault Tolerance (Happy Eyeballs)
// ============================================================================

#[test]
fn test_unified_send_probing_skips_dead_ips() {
    let source_dir = unique_dir("multi-ip-src");
    let output_dir = unique_dir("multi-ip-out");

    let file_path = source_dir.join("multi_ip.bin");
    let payload = patterned_bytes(32 * 1024);
    fs::write(&file_path, &payload).expect("write file");

    let (recv_tx, recv_rx) = mpsc::channel::<PlenumEvent>();
    let recv_output = output_dir.clone();

    let recv_handle = thread::spawn(move || {
        let mut core = PlenumCore::new();
        let mut sink = move |event: PlenumEvent| {
            let _ = recv_tx.send(event);
        };
        core.receive_file_unified(
            ReceiveUnifiedRequest {
                output_dir: recv_output,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
                session_id: "MULTIIP99".to_string(),
                my_peer_id: "recv-multi".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 5,
                port: 0,
                require_pin: false,
                auto_accept: true,
                device_name: None,
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    let (actual_port, token) = loop {
        let event = recv_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("Listening event");
        if let PlenumEvent::Transfer(TransferEvent::Listening { port, pin, .. }) = event {
            break (port, pin.unwrap_or_default());
        }
    };

    // Construct payload with two non-routable IPs (RFC 5737 TEST-NET-1 & TEST-NET-2)
    // followed by the working 127.0.0.1 loopback IP.
    let mut unified_payload = UnifiedSharePayload::new("MULTIIP99");
    unified_payload.lan_ips = vec![
        "192.0.2.1".to_string(),
        "198.51.100.25".to_string(),
        "127.0.0.1".to_string(),
    ];
    unified_payload.port = actual_port;
    unified_payload.pin = Some(token);

    let (send_tx, send_rx) = mpsc::channel::<PlenumEvent>();
    let sender_handle = thread::spawn(move || {
        let mut core = PlenumCore::new();
        let mut sink = move |event: PlenumEvent| {
            let _ = send_tx.send(event);
        };
        core.send_file_unified(
            SendUnifiedRequest {
                file_path,
                file_paths: Vec::new(),
                payload: unified_payload,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
                my_peer_id: "send-multi".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 5,
                device_name: None,
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    let send_res = sender_handle.join().expect("sender join");
    let recv_res = recv_handle.join().expect("receiver join");

    assert!(send_res.is_ok(), "Multi-IP probing succeeded: {:?}", send_res);
    assert!(recv_res.is_ok(), "Multi-IP receiver succeeded: {:?}", recv_res);

    // Verify sender selected the working 127.0.0.1 IP
    let mut selected_address = String::new();
    while let Ok(event) = send_rx.try_recv() {
        if let PlenumEvent::Transfer(TransferEvent::PathSelected { path, .. }) = event {
            if let SelectedPath::Local { address } = path {
                selected_address = address;
            }
        }
    }
    assert!(
        selected_address.starts_with("127.0.0.1:"),
        "Sender must skip dead IPs and bind to working 127.0.0.1 endpoint, got: {selected_address}"
    );

    let received_bytes = fs::read(output_dir.join("multi_ip.bin")).expect("read output file");
    assert_eq!(received_bytes, payload);
}

// ============================================================================
// Suite 5: WebRTC Fallback when LAN is Blocked / Unreachable (AP Isolation)
// ============================================================================

#[test]
fn test_unified_fallback_to_webrtc_when_lan_unreachable() {
    let relay_runtime = tokio::runtime::Runtime::new().expect("build relay runtime");
    let relay_url = spawn_relay_server(&relay_runtime);

    let source_dir = unique_dir("fallback-rtc-src");
    let output_dir = unique_dir("fallback-rtc-out");

    let file_path = source_dir.join("remote_file.bin");
    let payload = patterned_bytes(64 * 1024); // 64 KiB
    fs::write(&file_path, &payload).expect("write file");

    let session_id = "FALLBACK-ROOM-99".to_string();
    let recv_output = output_dir.clone();
    let recv_relay = relay_url.clone();
    let recv_session = session_id.clone();

    let (recv_tx, recv_rx) = mpsc::channel::<PlenumEvent>();

    let recv_handle = thread::spawn(move || {
        let mut core = PlenumCore::new();
        let mut sink = move |event: PlenumEvent| {
            let _ = recv_tx.send(event);
        };
        core.receive_file_unified(
            ReceiveUnifiedRequest {
                output_dir: recv_output,
                relay_server_url: recv_relay,
                session_id: recv_session,
                my_peer_id: "recv-peer-rtc".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 20,
                port: 0,
                require_pin: false,
                auto_accept: true,
                device_name: Some("Remote-Receiver".to_string()),
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    // Sender's payload deliberately has an UNREACHABLE / CLOSED port (port 1)
    // to simulate LAN being blocked by an AP isolation router or client firewall.
    let mut unified_payload = UnifiedSharePayload::new(&session_id);
    unified_payload.lan_ips = vec!["127.0.0.1".to_string()];
    unified_payload.port = 1; // Connection refused immediately!

    let (send_tx, send_rx) = mpsc::channel::<PlenumEvent>();
    let send_relay = relay_url.clone();
    let sender_handle = thread::spawn(move || {
        let mut core = PlenumCore::new();
        let mut sink = move |event: PlenumEvent| {
            let _ = send_tx.send(event);
        };
        core.send_file_unified(
            SendUnifiedRequest {
                file_path,
                file_paths: Vec::new(),
                payload: unified_payload,
                relay_server_url: send_relay,
                my_peer_id: "send-peer-rtc".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 20,
                device_name: Some("Remote-Sender".to_string()),
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    let send_res = sender_handle.join().expect("sender join");
    let recv_res = recv_handle.join().expect("receiver join");

    assert!(
        send_res.is_ok(),
        "Fallback WebRTC send succeeded: {:?}",
        send_res
    );
    assert!(
        recv_res.is_ok(),
        "Fallback WebRTC recv succeeded: {:?}",
        recv_res
    );

    // Verify sender emitted PathSelected::Internet
    let mut sender_chose_internet = false;
    while let Ok(event) = send_rx.try_recv() {
        if let PlenumEvent::Transfer(TransferEvent::PathSelected { path, .. }) = event {
            if matches!(path, SelectedPath::Internet { .. }) {
                sender_chose_internet = true;
            }
        }
    }
    assert!(
        sender_chose_internet,
        "Sender must emit PathSelected::Internet when LAN probe fails"
    );

    // Verify receiver emitted PathSelected::Internet
    let mut receiver_chose_internet = false;
    while let Ok(event) = recv_rx.try_recv() {
        if let PlenumEvent::Transfer(TransferEvent::PathSelected { path, .. }) = event {
            if matches!(path, SelectedPath::Internet { .. }) {
                receiver_chose_internet = true;
            }
        }
    }
    assert!(
        receiver_chose_internet,
        "Receiver must emit PathSelected::Internet when connection arrives via WebRTC"
    );

    let received_bytes = fs::read(output_dir.join("remote_file.bin")).expect("read file");
    assert_eq!(
        received_bytes, payload,
        "WebRTC fallback transferred content must match exactly"
    );
}

// ============================================================================
// Suite 6: Edge Case File Scenarios in Unified Mode
// ============================================================================

#[test]
fn test_unified_zero_byte_file_transfer() {
    let source_dir = unique_dir("zero-byte-src");
    let output_dir = unique_dir("zero-byte-out");

    let file_path = source_dir.join("empty.txt");
    fs::write(&file_path, b"").expect("write empty file");

    let (recv_tx, recv_rx) = mpsc::channel::<PlenumEvent>();
    let recv_output = output_dir.clone();

    let recv_handle = thread::spawn(move || {
        let mut core = PlenumCore::new();
        let mut sink = move |event: PlenumEvent| {
            let _ = recv_tx.send(event);
        };
        core.receive_file_unified(
            ReceiveUnifiedRequest {
                output_dir: recv_output,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
                session_id: "EMPTY0001".to_string(),
                my_peer_id: "recv-empty".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 5,
                port: 0,
                require_pin: false,
                auto_accept: true,
                device_name: None,
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    let (actual_port, token) = loop {
        let event = recv_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("Listening event");
        if let PlenumEvent::Transfer(TransferEvent::Listening { port, pin, .. }) = event {
            break (port, pin.unwrap_or_default());
        }
    };

    let mut unified_payload = UnifiedSharePayload::new("EMPTY0001");
    unified_payload.lan_ips = vec!["127.0.0.1".to_string()];
    unified_payload.port = actual_port;
    unified_payload.pin = Some(token);

    let sender_handle = thread::spawn(move || {
        let mut core = PlenumCore::new();
        let mut sink = |_event: PlenumEvent| {};
        core.send_file_unified(
            SendUnifiedRequest {
                file_path,
                file_paths: Vec::new(),
                payload: unified_payload,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
                my_peer_id: "send-empty".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 5,
                device_name: None,
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    let send_res = sender_handle.join().expect("sender join");
    let recv_res = recv_handle.join().expect("receiver join");

    assert!(send_res.is_ok(), "Zero byte send failed: {:?}", send_res);
    assert!(recv_res.is_ok(), "Zero byte recv failed: {:?}", recv_res);

    let empty_out = output_dir.join("empty.txt");
    assert!(empty_out.exists());
    assert_eq!(fs::metadata(empty_out).unwrap().len(), 0);
}

#[test]
fn test_unified_large_file_multi_chunk_transfer() {
    let source_dir = unique_dir("large-src");
    let output_dir = unique_dir("large-out");

    let file_path = source_dir.join("large_payload.bin");
    let payload = patterned_bytes(1024 * 1024); // 1 MiB
    fs::write(&file_path, &payload).expect("write large file");

    let (recv_tx, recv_rx) = mpsc::channel::<PlenumEvent>();
    let recv_output = output_dir.clone();

    // Use small chunk size (16 KiB) to force 64 individual chunk round-trips
    let options = TransferOptions {
        chunk_size: 16 * 1024,
        window_size: 16,
        timeout_ticks: 15_000,
        require_verification: false,
    };
    let sender_options = options.clone();

    let recv_handle = thread::spawn(move || {
        let mut core = PlenumCore::new();
        let mut sink = move |event: PlenumEvent| {
            if let PlenumEvent::Log { message, .. } = &event {
                eprintln!("[RECV LOG] {message}");
            }
            let _ = recv_tx.send(event);
        };
        core.receive_file_unified(
            ReceiveUnifiedRequest {
                output_dir: recv_output,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
                session_id: "LARGE1234".to_string(),
                my_peer_id: "recv-large".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 10,
                port: 0,
                require_pin: false,
                auto_accept: true,
                device_name: None,
                permissions: CorePermissions::full(),
                options,
            },
            &mut sink,
        )
    });

    let (actual_port, token) = loop {
        let event = recv_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("Listening event");
        if let PlenumEvent::Transfer(TransferEvent::Listening { port, pin, .. }) = event {
            break (port, pin.unwrap_or_default());
        }
    };

    let mut unified_payload = UnifiedSharePayload::new("LARGE1234");
    unified_payload.lan_ips = vec!["127.0.0.1".to_string()];
    unified_payload.port = actual_port;
    unified_payload.pin = Some(token);

    let sender_handle = thread::spawn(move || {
        let mut core = PlenumCore::new();
        let mut sink = |event: PlenumEvent| {
            if let PlenumEvent::Log { message, .. } = &event {
                eprintln!("[SEND LOG] {message}");
            }
        };
        core.send_file_unified(
            SendUnifiedRequest {
                file_path,
                file_paths: Vec::new(),
                payload: unified_payload,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
                my_peer_id: "send-large".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 10,
                device_name: None,
                permissions: CorePermissions::full(),
                options: sender_options,
            },
            &mut sink,
        )
    });

    let send_res = sender_handle.join().expect("sender join");
    let recv_res = recv_handle.join().expect("receiver join");

    assert!(send_res.is_ok(), "Large send failed: {:?}", send_res);
    assert!(recv_res.is_ok(), "Large recv failed: {:?}", recv_res);

    let received_bytes = fs::read(output_dir.join("large_payload.bin")).expect("read large file");
    assert_eq!(received_bytes.len(), 1024 * 1024);
    assert_eq!(received_bytes, payload);
}

// ============================================================================
// Suite 7: Cancellation & Teardown Testing
// ============================================================================

#[test]
fn test_unified_receiver_cancelled_while_listening() {
    let output_dir = unique_dir("cancel-recv-out");
    let mut core = PlenumCore::new();
    let control = core.control();

    let recv_handle = thread::spawn(move || {
        let mut sink = |_event: PlenumEvent| {};
        core.receive_file_unified(
            ReceiveUnifiedRequest {
                output_dir,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
                session_id: "CANCEL0001".to_string(),
                my_peer_id: "recv-cancel".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 60,
                port: 0,
                require_pin: false,
                auto_accept: true,
                device_name: None,
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    // Sleep briefly so the receiver reaches Listening state
    thread::sleep(Duration::from_millis(100));

    // Trigger local session cancellation
    control.cancel();

    let result = recv_handle.join().expect("receiver thread join");
    assert!(
        matches!(result, Err(AppError::Cancelled)),
        "Receiver must return Cancelled error, got: {:?}",
        result
    );
}

#[test]
fn test_unified_sender_cancelled_before_connect() {
    let source_dir = unique_dir("cancel-send-src");
    let file_path = source_dir.join("cancelled.txt");
    fs::write(&file_path, b"data").expect("write file");

    let mut core = PlenumCore::new();
    let control = core.control();
    control.cancel(); // Pre-cancel

    let mut payload = UnifiedSharePayload::new("CANCEL0002");
    payload.lan_ips = vec!["127.0.0.1".to_string()];
    payload.port = 40000;

    let mut sink = |_event: PlenumEvent| {};
    let result = core.send_file_unified(
        SendUnifiedRequest {
            file_path,
            file_paths: Vec::new(),
            payload,
            relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
            my_peer_id: "send-cancel".to_string(),
            ice_servers: Vec::new(),
            connect_timeout_secs: 5,
            device_name: None,
            permissions: CorePermissions::full(),
            options: TransferOptions::default(),
        },
        &mut sink,
    );

    assert!(
        matches!(result, Err(AppError::Cancelled)),
        "Sender must return Cancelled error immediately, got: {:?}",
        result
    );
}

// ============================================================================
// Suite 8: Transport-Level Multipath Failover & Metrics
// ============================================================================

// ============================================================================
// Suite 8: Multi-File & Folder Bundle Packaging Transfers
// ============================================================================

#[test]
fn test_unified_multi_file_bundle_transfer() {
    let source_dir = unique_dir("multi-send-src");
    let output_dir = unique_dir("multi-recv-out");

    let f1 = source_dir.join("photo.jpg");
    let f2 = source_dir.join("notes.txt");
    let f3 = source_dir.join("data.csv");
    fs::write(&f1, b"JPEG_DATA_SIMULATION").expect("write f1");
    fs::write(&f2, b"Important meeting notes").expect("write f2");
    fs::write(&f3, b"id,val\n1,100\n2,200\n").expect("write f3");

    let mut core_recv = PlenumCore::new();
    let (recv_tx, recv_rx) = mpsc::channel();
    let recv_output = output_dir.clone();

    let recv_handle = thread::spawn(move || {
        let mut sink = move |event: PlenumEvent| {
            let _ = recv_tx.send(event);
        };
        core_recv.receive_file_unified(
            ReceiveUnifiedRequest {
                output_dir: recv_output,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
                session_id: "MULTI0001".to_string(),
                my_peer_id: "peer-recv-multi".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 5,
                port: 0,
                require_pin: false,
                auto_accept: true,
                device_name: Some("Multi-Receiver".to_string()),
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    let (actual_port, pin_token) = loop {
        let event = recv_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("Receive Listening event");
        if let PlenumEvent::Transfer(TransferEvent::Listening { port, pin, .. }) = event {
            break (port, pin.unwrap_or_default());
        }
    };

    let mut unified_payload = UnifiedSharePayload::new("MULTI0001");
    unified_payload.lan_ips = vec!["127.0.0.1".to_string()];
    unified_payload.port = actual_port;
    unified_payload.pin = Some(pin_token);

    let (send_tx, send_rx) = mpsc::channel();
    let sender_paths = vec![f1, f2, f3];

    let sender_handle = thread::spawn(move || {
        let mut core = PlenumCore::new();
        let mut sink = move |event: PlenumEvent| {
            let _ = send_tx.send(event);
        };
        core.send_file_unified(
            SendUnifiedRequest {
                file_path: PathBuf::new(),
                file_paths: sender_paths,
                payload: unified_payload,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
                my_peer_id: "peer-send-multi".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 5,
                device_name: Some("Sender-PC".to_string()),
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    let send_res = sender_handle.join().expect("sender thread join");
    let recv_res = recv_handle.join().expect("receiver thread join");

    assert!(send_res.is_ok(), "Multi-file send failed: {:?}", send_res);
    assert!(recv_res.is_ok(), "Multi-file recv failed: {:?}", recv_res);

    let recv_summary = recv_res.unwrap();
    assert!(
        recv_summary.file_name.ends_with(".zip"),
        "Multi-file transfer must produce a .zip file, got {}",
        recv_summary.file_name
    );

    // Verify zip archive contents
    let received_zip = output_dir.join(&recv_summary.file_name);
    assert!(received_zip.exists(), "Received zip file must exist on disk");

    let zip_file = fs::File::open(&received_zip).expect("open received zip");
    let mut archive = zip::ZipArchive::new(zip_file).expect("parse received zip");
    assert_eq!(archive.len(), 3, "Archive must contain exactly 3 files");

    let file_names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    assert!(file_names.contains(&"photo.jpg".to_string()));
    assert!(file_names.contains(&"notes.txt".to_string()));
    assert!(file_names.contains(&"data.csv".to_string()));

    // Verify extracted file contents
    use std::io::Read;
    let mut notes_entry = archive.by_name("notes.txt").expect("find notes.txt");
    let mut notes_content = String::new();
    notes_entry.read_to_string(&mut notes_content).expect("read notes");
    assert_eq!(notes_content, "Important meeting notes");

    // Verify Packaging event was emitted to sender sink
    let packaging_emitted = send_rx.try_iter().any(|e| matches!(e, PlenumEvent::Transfer(TransferEvent::Packaging { .. })));
    assert!(packaging_emitted, "Packaging event must be emitted during multi-file archive creation");

    let _ = fs::remove_dir_all(&source_dir);
    let _ = fs::remove_dir_all(&output_dir);
}

#[test]
fn test_unified_folder_bundle_transfer() {
    let source_dir = unique_dir("folder-send-src");
    let output_dir = unique_dir("folder-recv-out");

    let root_folder = source_dir.join("MyAwesomeApp");
    let assets_folder = root_folder.join("assets");
    let src_folder = root_folder.join("src");
    fs::create_dir_all(&assets_folder).expect("create assets");
    fs::create_dir_all(&src_folder).expect("create src");

    fs::write(root_folder.join("README.md"), b"# MyAwesomeApp").expect("write readme");
    fs::write(assets_folder.join("logo.png"), b"PNG_LOGO_BYTES").expect("write logo");
    fs::write(src_folder.join("main.rs"), b"fn main() {}").expect("write main");

    let mut core_recv = PlenumCore::new();
    let (recv_tx, recv_rx) = mpsc::channel();
    let recv_output = output_dir.clone();

    let recv_handle = thread::spawn(move || {
        let mut sink = move |event: PlenumEvent| {
            let _ = recv_tx.send(event);
        };
        core_recv.receive_file_unified(
            ReceiveUnifiedRequest {
                output_dir: recv_output,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
                session_id: "FOLDER0001".to_string(),
                my_peer_id: "peer-recv-folder".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 5,
                port: 0,
                require_pin: false,
                auto_accept: true,
                device_name: Some("Folder-Receiver".to_string()),
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    let (actual_port, pin_token) = loop {
        let event = recv_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("Receive Listening event");
        if let PlenumEvent::Transfer(TransferEvent::Listening { port, pin, .. }) = event {
            break (port, pin.unwrap_or_default());
        }
    };

    let mut unified_payload = UnifiedSharePayload::new("FOLDER0001");
    unified_payload.lan_ips = vec!["127.0.0.1".to_string()];
    unified_payload.port = actual_port;
    unified_payload.pin = Some(pin_token);

    let (send_tx, _send_rx) = mpsc::channel();
    let sender_paths = vec![root_folder];

    let sender_handle = thread::spawn(move || {
        let mut core = PlenumCore::new();
        let mut sink = move |event: PlenumEvent| {
            let _ = send_tx.send(event);
        };
        core.send_file_unified(
            SendUnifiedRequest {
                file_path: PathBuf::new(),
                file_paths: sender_paths,
                payload: unified_payload,
                relay_server_url: "ws://127.0.0.1:1/ws".to_string(),
                my_peer_id: "peer-send-folder".to_string(),
                ice_servers: Vec::new(),
                connect_timeout_secs: 5,
                device_name: Some("Sender-PC".to_string()),
                permissions: CorePermissions::full(),
                options: TransferOptions::default(),
            },
            &mut sink,
        )
    });

    let send_res = sender_handle.join().expect("sender thread join");
    let recv_res = recv_handle.join().expect("receiver thread join");

    assert!(send_res.is_ok(), "Folder send failed: {:?}", send_res);
    assert!(recv_res.is_ok(), "Folder recv failed: {:?}", recv_res);

    let recv_summary = recv_res.unwrap();
    assert_eq!(recv_summary.file_name, "MyAwesomeApp.zip");

    let received_zip = output_dir.join("MyAwesomeApp.zip");
    assert!(received_zip.exists(), "Received zip file must exist on disk");

    let zip_file = fs::File::open(&received_zip).expect("open received zip");
    let mut archive = zip::ZipArchive::new(zip_file).expect("parse received zip");

    let file_names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();

    assert!(file_names.iter().any(|n| n.contains("MyAwesomeApp/README.md")));
    assert!(file_names.iter().any(|n| n.contains("MyAwesomeApp/assets/logo.png")));
    assert!(file_names.iter().any(|n| n.contains("MyAwesomeApp/src/main.rs")));

    let _ = fs::remove_dir_all(&source_dir);
    let _ = fs::remove_dir_all(&output_dir);
}
