use std::time::Duration;

use plenum::security::{
    AuthenticatedPairingToken, ReplayProtector, SecurityError, SessionCipher, SessionId,
};

#[test]
fn generates_valid_secure_session_ids() {
    let first = SessionId::generate();
    let second = SessionId::generate();

    assert_eq!(first.as_str().len(), 32);
    assert!(first.as_str().bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ne!(first, second);
    assert_eq!(SessionId::parse(first.as_str().to_string()).unwrap(), first);
}

#[test]
fn rejects_invalid_session_id_format() {
    let err = SessionId::parse("not-a-valid-session").expect_err("invalid session id should fail");
    assert_eq!(err, SecurityError::InvalidSessionId);
}

#[test]
fn pairing_token_roundtrip_and_verification_succeeds() {
    let secret = b"super-secret-shared-key";
    let token = AuthenticatedPairingToken::issue(
        secret,
        SessionId::generate(),
        "peer-a",
        Duration::from_secs(60),
    )
    .expect("token should issue");

    let json = token.to_json().expect("token should serialize");
    let decoded = AuthenticatedPairingToken::from_json(&json).expect("token should deserialize");

    decoded.verify(secret).expect("token should verify");
    assert_eq!(decoded, token);
}

#[test]
fn pairing_token_detects_tampering() {
    let secret = b"super-secret-shared-key";
    let mut token = AuthenticatedPairingToken::issue(
        secret,
        SessionId::generate(),
        "peer-a",
        Duration::from_secs(60),
    )
    .expect("token should issue");
    token.peer_id = "peer-b".into();

    let err = token
        .verify(secret)
        .expect_err("tampered token should fail");
    assert_eq!(err, SecurityError::InvalidSignature);
}

#[test]
fn pairing_token_detects_expiry() {
    let secret = b"super-secret-shared-key";
    let mut token = AuthenticatedPairingToken::issue(
        secret,
        SessionId::generate(),
        "peer-a",
        Duration::from_secs(60),
    )
    .expect("token should issue");
    token.expires_at_unix_secs = 0;

    let err = token.verify(secret).expect_err("expired token should fail");
    assert_eq!(err, SecurityError::TokenExpired);
}

#[test]
fn replay_protector_rejects_duplicates_and_evicts_old_entries() {
    let mut replay = ReplayProtector::new(2).expect("replay protector should build");

    replay.check_and_insert(b"a".to_vec()).unwrap();
    replay.check_and_insert(b"b".to_vec()).unwrap();

    let err = replay
        .check_and_insert(b"a".to_vec())
        .expect_err("duplicate should fail");
    assert_eq!(err, SecurityError::ReplayDetected);

    replay.check_and_insert(b"c".to_vec()).unwrap();
    replay
        .check_and_insert(b"a".to_vec())
        .expect("oldest entry should have been evicted");
}

#[test]
fn session_cipher_encrypts_and_decrypts_payloads() {
    let key = SessionCipher::generate_key();
    let sender = SessionCipher::new(&key).expect("cipher should build");
    let mut receiver = SessionCipher::new(&key).expect("cipher should build");
    let aad = b"session-boundary";

    let frame = sender
        .encrypt(b"top secret payload", aad)
        .expect("encryption should succeed");
    let plaintext = receiver
        .decrypt(&frame, aad)
        .expect("decryption should succeed");

    assert_eq!(plaintext, b"top secret payload");
}

#[test]
fn session_cipher_detects_replay_and_aad_mismatch() {
    let key = SessionCipher::generate_key();
    let sender = SessionCipher::new(&key).expect("cipher should build");
    let mut receiver = SessionCipher::new(&key).expect("cipher should build");

    let frame = sender
        .encrypt(b"payload", b"aad-1")
        .expect("encryption should succeed");

    let err = receiver
        .decrypt(&frame, b"aad-2")
        .expect_err("aad mismatch should fail");
    assert_eq!(err, SecurityError::DecryptionFailed);

    let frame = sender
        .encrypt(b"payload", b"aad-3")
        .expect("encryption should succeed");
    let _ = receiver.decrypt(&frame, b"aad-3").unwrap();
    let err = receiver
        .decrypt(&frame, b"aad-3")
        .expect_err("replay should fail");
    assert_eq!(err, SecurityError::ReplayDetected);
}
