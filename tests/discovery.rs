use plenum::discovery::beacon::{Announcement, Beacon, BeaconConfig};
use plenum::discovery::error::DiscoveryError;
use plenum::discovery::token::PairingToken;

use std::net::Ipv4Addr;
use std::time::Duration;

#[test]
fn pairing_token_generates_six_character_code() {
    let token = PairingToken::generate();
    assert_eq!(token.code().len(), 6);
    assert!(token.is_valid());
}

#[test]
fn pairing_token_verifies_matching_code() {
    let token = PairingToken::generate();
    let code = token.code().to_string();
    assert!(token.verify(&code).is_ok());
}

#[test]
fn pairing_token_rejects_mismatched_code() {
    let token = PairingToken::generate();
    let err = token.verify("WRONG1").unwrap_err();
    assert_eq!(err, DiscoveryError::TokenMismatch);
}

#[test]
fn pairing_token_expires_after_ttl() {
    let token = PairingToken::generate_with_ttl(Duration::from_millis(1));
    std::thread::sleep(Duration::from_millis(10));
    assert!(!token.is_valid());
    let err = token.verify(token.code()).unwrap_err();
    assert_eq!(err, DiscoveryError::TokenExpired);
}

#[test]
fn announcement_roundtrip_encode_decode() {
    let original = Announcement {
        token: "ABC123".to_string(),
        tcp_port: 9090,
        hostname: "test-machine".to_string(),
        source_addr: Ipv4Addr::LOCALHOST,
        pin_required: false,
    };

    let encoded = original.encode();
    let decoded =
        Announcement::decode(&encoded, Ipv4Addr::new(192, 168, 1, 42)).expect("should decode");

    assert_eq!(decoded.token, "ABC123");
    assert_eq!(decoded.tcp_port, 9090);
    assert_eq!(decoded.hostname, "test-machine");
    assert_eq!(decoded.source_addr, Ipv4Addr::new(192, 168, 1, 42));
}

#[test]
fn announcement_rejects_truncated_data() {
    let err = Announcement::decode(b"AET", Ipv4Addr::LOCALHOST).unwrap_err();
    assert_eq!(err, DiscoveryError::MalformedAnnouncement);
}

#[test]
fn announcement_rejects_wrong_magic() {
    let err = Announcement::decode(b"XXXX\x01\x00\x00\x00\x00", Ipv4Addr::LOCALHOST).unwrap_err();
    assert_eq!(err, DiscoveryError::MalformedAnnouncement);
}

#[test]
fn beacon_broadcast_and_discover_on_localhost() {
    // Use a random high port to avoid conflicts
    let port: u16 = 41821 + (std::process::id() as u16 % 100);
    let tcp_port: u16 = 9999;

    let config = BeaconConfig {
        broadcast_port: port,
        broadcast_interval: Duration::from_millis(50),
        discover_timeout: Duration::from_secs(3),
    };

    let token = PairingToken::generate();
    let token_code = token.code().to_string();

    // Broadcaster in a background thread
    let broadcast_beacon = Beacon::with_config(config.clone());
    let handle = broadcast_beacon
        .broadcast(&token, tcp_port, None, false)
        .expect("broadcast should start");

    let broadcaster = std::thread::spawn(move || {
        handle
            .broadcast_for(Duration::from_secs(2))
            .expect("broadcast should succeed");
    });

    // Give broadcaster a moment to start
    std::thread::sleep(Duration::from_millis(100));

    // Discover
    let discover_beacon = Beacon::with_config(config);
    let announcement = discover_beacon
        .discover()
        .expect("should discover the broadcaster");

    assert_eq!(announcement.token, token_code);
    assert_eq!(announcement.tcp_port, tcp_port);

    broadcaster
        .join()
        .expect("broadcaster thread should finish");
}
