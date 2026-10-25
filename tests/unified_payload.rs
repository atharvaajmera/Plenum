use plenum::app::types::UnifiedSharePayload;

#[test]
fn test_unified_payload_roundtrip() {
    let mut payload = UnifiedSharePayload::new("7K9X2M4P1");
    payload.lan_ips = vec!["192.168.1.45".to_string(), "10.0.0.12".to_string()];
    payload.port = 42424;
    payload.pin = Some("8472".to_string());
    payload.peer_id = Some("p-9f8e7d".to_string());
    payload.device_name = Some("Atharva-PC".to_string());

    let uri = payload.to_uri();
    assert!(uri.starts_with("plenum://v1/connect?"));

    let parsed = UnifiedSharePayload::parse_uri(&uri).expect("Failed to parse URI");
    assert_eq!(parsed.room_code, "7K9X2M4P1");
    assert_eq!(parsed.lan_ips, vec!["192.168.1.45", "10.0.0.12"]);
    assert_eq!(parsed.port, 42424);
    assert_eq!(parsed.pin.as_deref(), Some("8472"));
    assert_eq!(parsed.peer_id.as_deref(), Some("p-9f8e7d"));
    assert_eq!(parsed.device_name.as_deref(), Some("Atharva-PC"));
}

#[test]
fn test_unified_payload_minimal_room_uri() {
    let parsed = UnifiedSharePayload::parse_uri("plenum://v1/connect?r=ABCDEF123").expect("Failed room URI");
    assert_eq!(parsed.room_code, "ABCDEF123");
    assert!(parsed.lan_ips.is_empty());
    assert_eq!(parsed.port, 0);
    assert_eq!(parsed.pin, None);
}

#[test]
fn test_raw_room_code_fallback() {
    let parsed = UnifiedSharePayload::parse_uri("7K9-X2M-4P1").expect("Failed raw code");
    assert_eq!(parsed.room_code, "7K9X2M4P1");
}

#[test]
fn test_legacy_and_invalid_uri_rejected() {
    // Missing query string / legacy formats
    assert!(UnifiedSharePayload::parse_uri("plenum://room/ABCDEF123").is_err());
    assert!(UnifiedSharePayload::parse_uri("plenum://pin/4892").is_err());
    assert!(UnifiedSharePayload::parse_uri("").is_err());
    // Missing required room code
    assert!(UnifiedSharePayload::parse_uri("plenum://v1/connect?p=8080").is_err());
}

#[test]
fn test_unified_payload_with_public_key_roundtrip() {
    let mut payload = UnifiedSharePayload::new("7K9X2M4P1");
    let hex_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    payload.public_key = Some(hex_key.to_string());

    let uri = payload.to_uri();
    assert!(uri.contains(&format!("pk={hex_key}")));

    let parsed = UnifiedSharePayload::parse_uri(&uri).expect("Parse URI with public key");
    assert_eq!(parsed.public_key.as_deref(), Some(hex_key));

    // Aliases test: pub, key, public_key
    let alias_uri = format!("plenum://v1/connect?r=7K9X2M4P1&pub={hex_key}");
    let parsed_alias = UnifiedSharePayload::parse_uri(&alias_uri).expect("Parse pub alias");
    assert_eq!(parsed_alias.public_key.as_deref(), Some(hex_key));
}

