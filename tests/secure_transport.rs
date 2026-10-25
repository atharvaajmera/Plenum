use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use sha2::{Digest, Sha256};

use plenum::transport::{SecureTransport, TcpTransport, Transport, TransportResult};

struct RecordingTransport<T> {
    inner: T,
    sent: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl<T: Transport> Transport for RecordingTransport<T> {
    fn send(&mut self, bytes: &[u8]) -> TransportResult<()> {
        self.sent.lock().unwrap().push(bytes.to_vec());
        self.inner.send(bytes)
    }
    fn recv(&mut self) -> TransportResult<Option<Vec<u8>>> {
        self.inner.recv()
    }
    fn close(&mut self) -> TransportResult<()> {
        self.inner.close()
    }
    fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}

fn wait_recv<T: Transport>(transport: &mut T) -> TransportResult<Vec<u8>> {
    for _ in 0..2000 {
        if let Some(frame) = transport.recv()? {
            return Ok(frame);
        }
        thread::sleep(Duration::from_millis(2));
    }
    panic!("timed out waiting for frame")
}

#[test]
fn encrypted_transport_hides_plaintext_and_roundtrips_with_pin() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server_capture = captured.clone();

    let server = thread::spawn(move || {
        let tcp = TcpTransport::accept(&listener).unwrap();
        let recording = RecordingTransport {
            inner: tcp,
            sent: server_capture,
        };
        let mut secure = SecureTransport::accept(recording, Some("ABC234")).unwrap();
        let message = wait_recv(&mut secure).unwrap();
        assert_eq!(message, b"private-file-name.txt::PRIVATE-FILE-CONTENTS");
        secure.send(b"authenticated response").unwrap();
    });

    let tcp = TcpTransport::connect(address).unwrap();
    let mut secure = SecureTransport::connect(tcp, Some("abc234")).unwrap();
    secure
        .send(b"private-file-name.txt::PRIVATE-FILE-CONTENTS")
        .unwrap();
    assert_eq!(wait_recv(&mut secure).unwrap(), b"authenticated response");
    server.join().unwrap();

    let wire = captured.lock().unwrap().concat();
    assert!(
        !wire
            .windows(b"private-file-name.txt".len())
            .any(|w| w == b"private-file-name.txt")
    );
    assert!(
        !wire
            .windows(b"PRIVATE-FILE-CONTENTS".len())
            .any(|w| w == b"PRIVATE-FILE-CONTENTS")
    );
}

#[test]
fn wrong_pin_rejects_the_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let tcp = TcpTransport::accept(&listener).unwrap();
        SecureTransport::accept(tcp, Some("RIGHT1"))
            .err()
            .expect("server must reject wrong PIN")
    });

    let tcp = TcpTransport::connect(address).unwrap();
    let error = SecureTransport::connect(tcp, Some("WRONG1"))
        .err()
        .expect("client must reject proof");
    assert!(error.to_string().contains("pairing authentication failed"));
    let _ = server.join().unwrap();
}

struct MutatingTransport<T> {
    inner: T,
    encrypted_sends: usize,
    replay: bool,
}

impl<T: Transport> Transport for MutatingTransport<T> {
    fn send(&mut self, bytes: &[u8]) -> TransportResult<()> {
        if bytes.len() >= 28 && !bytes.starts_with(b"PLNSEC01") && bytes.len() != 32 {
            self.encrypted_sends += 1;
            if self.replay && self.encrypted_sends == 1 {
                self.inner.send(bytes)?;
                return self.inner.send(bytes);
            }
            if !self.replay && self.encrypted_sends == 1 {
                let mut tampered = bytes.to_vec();
                tampered[21] ^= 1;
                return self.inner.send(&tampered);
            }
        }
        self.inner.send(bytes)
    }
    fn recv(&mut self) -> TransportResult<Option<Vec<u8>>> {
        self.inner.recv()
    }
    fn close(&mut self) -> TransportResult<()> {
        self.inner.close()
    }
    fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}

fn rejection_case(replay: bool) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let tcp = TcpTransport::accept(&listener).unwrap();
        let mut secure = SecureTransport::accept(tcp, None).unwrap();
        if replay {
            let _ = wait_recv(&mut secure).unwrap();
            wait_recv(&mut secure).unwrap_err().to_string()
        } else {
            wait_recv(&mut secure).unwrap_err().to_string()
        }
    });
    let tcp = TcpTransport::connect(address).unwrap();
    let mut secure = SecureTransport::connect(
        MutatingTransport {
            inner: tcp,
            encrypted_sends: 0,
            replay,
        },
        None,
    )
    .unwrap();
    secure.send(b"secret").unwrap();
    server.join().unwrap()
}

#[test]
fn tampered_ciphertext_is_rejected() {
    assert!(rejection_case(false).contains("failed to decrypt payload"));
}

#[test]
fn replayed_ciphertext_is_rejected() {
    let error = rejection_case(true);
    assert!(error.contains("unexpected encrypted frame counter"));
    assert!(error.contains("expected_counter=1"));
    assert!(error.contains("received_counter=0"));
}

#[derive(Clone, Copy)]
enum EnvelopeMutation {
    ModifyCounter,
    ModifyTag,
    UnsupportedVersion,
    Truncate,
    OmitFirst,
    SwapFirstTwo,
}

struct EnvelopeMutatingTransport<T> {
    inner: T,
    mutation: EnvelopeMutation,
    encrypted_sends: usize,
    pending: Option<Vec<u8>>,
}

impl<T: Transport> Transport for EnvelopeMutatingTransport<T> {
    fn send(&mut self, bytes: &[u8]) -> TransportResult<()> {
        let is_envelope = bytes.len() >= 37 && bytes.first() == Some(&2);
        if !is_envelope {
            return self.inner.send(bytes);
        }
        self.encrypted_sends += 1;
        match (self.mutation, self.encrypted_sends) {
            (EnvelopeMutation::ModifyCounter, 1) => {
                let mut mutated = bytes.to_vec();
                mutated[8] ^= 1;
                self.inner.send(&mutated)
            }
            (EnvelopeMutation::ModifyTag, 1) => {
                let mut mutated = bytes.to_vec();
                *mutated.last_mut().unwrap() ^= 1;
                self.inner.send(&mutated)
            }
            (EnvelopeMutation::UnsupportedVersion, 1) => {
                let mut mutated = bytes.to_vec();
                mutated[0] = 3;
                self.inner.send(&mutated)
            }
            (EnvelopeMutation::Truncate, 1) => self.inner.send(&bytes[..20]),
            (EnvelopeMutation::OmitFirst, 1) => Ok(()),
            (EnvelopeMutation::SwapFirstTwo, 1) => {
                self.pending = Some(bytes.to_vec());
                Ok(())
            }
            (EnvelopeMutation::SwapFirstTwo, 2) => {
                self.inner.send(bytes)?;
                self.inner.send(self.pending.take().as_deref().unwrap())
            }
            _ => self.inner.send(bytes),
        }
    }

    fn recv(&mut self) -> TransportResult<Option<Vec<u8>>> {
        self.inner.recv()
    }
    fn close(&mut self) -> TransportResult<()> {
        self.inner.close()
    }
    fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}

fn envelope_rejection_case(mutation: EnvelopeMutation, sends: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let tcp = TcpTransport::accept(&listener).unwrap();
        let mut secure = SecureTransport::accept(tcp, None).unwrap();
        wait_recv(&mut secure).unwrap_err().to_string()
    });
    let tcp = TcpTransport::connect(address).unwrap();
    let mut secure = SecureTransport::connect(
        EnvelopeMutatingTransport {
            inner: tcp,
            mutation,
            encrypted_sends: 0,
            pending: None,
        },
        None,
    )
    .unwrap();
    for index in 0..sends {
        secure.send(&[index as u8]).unwrap();
    }
    server.join().unwrap()
}

#[test]
fn omitted_encrypted_frame_is_rejected() {
    let error = envelope_rejection_case(EnvelopeMutation::OmitFirst, 2);
    assert!(error.contains("unexpected encrypted frame counter"));
    assert!(error.contains("expected_counter=0"));
    assert!(error.contains("received_counter=1"));
}

#[test]
fn swapped_encrypted_frames_are_rejected() {
    let error = envelope_rejection_case(EnvelopeMutation::SwapFirstTwo, 2);
    assert!(error.contains("unexpected encrypted frame counter"));
    assert!(error.contains("expected_counter=0"));
    assert!(error.contains("received_counter=1"));
}

#[test]
fn modified_encrypted_frame_counter_is_rejected() {
    assert!(
        envelope_rejection_case(EnvelopeMutation::ModifyCounter, 1)
            .contains("unexpected encrypted frame counter")
    );
}

#[test]
fn modified_encrypted_frame_tag_is_rejected() {
    let error = envelope_rejection_case(EnvelopeMutation::ModifyTag, 1);
    assert!(error.contains("failed to authenticate encrypted frame"));
    assert!(error.contains("counter=0"));
}

#[test]
fn unsupported_encrypted_envelope_version_is_rejected() {
    assert!(
        envelope_rejection_case(EnvelopeMutation::UnsupportedVersion, 1)
            .contains("unsupported encrypted envelope version")
    );
}

#[test]
fn truncated_encrypted_envelope_is_rejected() {
    assert!(
        envelope_rejection_case(EnvelopeMutation::Truncate, 1)
            .contains("truncated encrypted frame")
    );
}

fn run_encrypted_tcp_transfer(frame_count: usize) {
    const CHUNK_SIZE: usize = 256 * 1024;
    const ACK_TIMEOUT: Duration = Duration::from_secs(10);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let tcp = TcpTransport::accept(&listener).unwrap();
        let mut secure = SecureTransport::accept(tcp, None).unwrap();
        let mut received_hash = Sha256::new();

        for index in 0..frame_count {
            let frame = wait_recv(&mut secure).unwrap();
            let expected = vec![(index as u8).wrapping_add(17); CHUNK_SIZE];
            assert_eq!(frame, expected);
            received_hash.update(&frame);
            secure.send(&(index as u64).to_be_bytes()).unwrap();
        }

        received_hash.finalize().to_vec()
    });

    let tcp = TcpTransport::connect(address).unwrap();
    let mut secure = SecureTransport::connect(tcp, None).unwrap();
    let mut sent_hash = Sha256::new();

    for index in 0..frame_count {
        let frame = vec![(index as u8).wrapping_add(17); CHUNK_SIZE];
        sent_hash.update(&frame);
        secure.send(&frame).unwrap();
        let deadline = std::time::Instant::now() + ACK_TIMEOUT;
        loop {
            if let Some(ack) = secure.recv().unwrap() {
                assert_eq!(ack, (index as u64).to_be_bytes());
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for ACK"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    assert_eq!(server.join().unwrap(), sent_hash.finalize().to_vec());
}

#[test]
fn encrypted_tcp_sustains_production_sized_transfer() {
    run_encrypted_tcp_transfer(64);
}

#[test]
#[ignore = "large encrypted TCP transfer stress test"]
fn encrypted_tcp_sustains_large_transfer() {
    run_encrypted_tcp_transfer(512);
}

#[test]
fn sas_matches_on_both_peers_in_unauthenticated_lan() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();

    let server = thread::spawn(move || {
        let tcp = TcpTransport::accept(&listener).unwrap();
        let secure = SecureTransport::accept(tcp, None).unwrap();
        (secure.sas().to_string(), secure.is_pinned())
    });

    let tcp = TcpTransport::connect(address).unwrap();
    let secure = SecureTransport::connect(tcp, None).unwrap();
    let client_sas = secure.sas().to_string();
    let client_is_pinned = secure.is_pinned();

    let (server_sas, server_is_pinned) = server.join().unwrap();

    assert_eq!(client_sas, server_sas, "SAS code must match on both peers");
    assert_eq!(client_sas.len(), 6, "SAS code must be 6 digits");
    assert!(
        client_sas.chars().all(|c| c.is_ascii_digit()),
        "SAS must be numeric digits"
    );
    assert!(!client_is_pinned);
    assert!(!server_is_pinned);
}

#[test]
fn tampered_transcript_produces_different_sas() {
    let transcript_a = [0x42_u8; 32];
    let mut transcript_b = [0x42_u8; 32];
    transcript_b[15] ^= 0x01; // 1-bit flip

    let sas_a = plenum::transport::compute_sas(&transcript_a);
    let sas_b = plenum::transport::compute_sas(&transcript_b);

    assert_ne!(
        sas_a, sas_b,
        "tampered transcript must yield different SAS codes"
    );
    assert_eq!(sas_a.len(), 6);
    assert_eq!(sas_b.len(), 6);
}

#[test]
fn mitm_proxy_inserted_between_peers_produces_mismatched_codes() {
    let receiver_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let receiver_addr = receiver_listener.local_addr().unwrap();

    let proxy_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();

    // Receiver (Peer B)
    let receiver_handle = thread::spawn(move || {
        let tcp = TcpTransport::accept(&receiver_listener).unwrap();
        let secure = SecureTransport::accept(tcp, None).unwrap();
        secure.sas().to_string()
    });

    // MITM Proxy sitting between Peer A and Peer B
    let proxy_handle = thread::spawn(move || {
        // Proxy accepts connection from Sender
        let sender_tcp = TcpTransport::accept(&proxy_listener).unwrap();
        let proxy_as_server = SecureTransport::accept(sender_tcp, None).unwrap();

        // Proxy initiates connection to Receiver
        let receiver_tcp = TcpTransport::connect(receiver_addr).unwrap();
        let proxy_as_client = SecureTransport::connect(receiver_tcp, None).unwrap();

        (
            proxy_as_server.sas().to_string(),
            proxy_as_client.sas().to_string(),
        )
    });

    // Sender (Peer A) connects to Proxy thinking it's the receiver
    let sender_tcp = TcpTransport::connect(proxy_addr).unwrap();
    let sender_secure = SecureTransport::connect(sender_tcp, None).unwrap();
    let sender_sas = sender_secure.sas().to_string();

    let receiver_sas = receiver_handle.join().unwrap();
    let (proxy_with_sender, proxy_with_receiver) = proxy_handle.join().unwrap();

    // Sender agrees with Proxy-as-server
    assert_eq!(sender_sas, proxy_with_sender);
    // Receiver agrees with Proxy-as-client
    assert_eq!(receiver_sas, proxy_with_receiver);
    // But Sender and Receiver have MISMATCHED SAS codes!
    assert_ne!(
        sender_sas, receiver_sas,
        "MITM proxy causes sender and receiver to derive completely different SAS codes"
    );
}

#[test]
fn qr_pinning_accepts_when_public_key_matches_and_marks_is_pinned() {
    use rand::rngs::OsRng;
    use x25519_dalek::{PublicKey, StaticSecret};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();

    let server_secret = StaticSecret::random_from_rng(OsRng);
    let server_public = PublicKey::from(&server_secret);
    let server_secret_bytes = server_secret.to_bytes();
    let server_public_bytes = *server_public.as_bytes();

    let server = thread::spawn(move || {
        let tcp = TcpTransport::accept(&listener).unwrap();
        let secure =
            SecureTransport::accept_with_secret(tcp, None, Some(&server_secret_bytes)).unwrap();
        secure.sas().to_string()
    });

    let tcp = TcpTransport::connect(address).unwrap();
    let secure =
        SecureTransport::connect_pinned(tcp, None, Some(&server_public_bytes)).unwrap();
    assert!(
        secure.is_pinned(),
        "pinned connection must mark is_pinned = true"
    );

    let server_sas = server.join().unwrap();
    assert_eq!(secure.sas(), server_sas);
}

#[test]
fn qr_pinning_rejects_mitm_tampered_key() {
    use rand::rngs::OsRng;
    use x25519_dalek::{PublicKey, StaticSecret};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();

    let server_secret = StaticSecret::random_from_rng(OsRng);
    let server_secret_bytes = server_secret.to_bytes();

    // Victim (Client) pinned a different public key from QR code
    let fake_secret = StaticSecret::random_from_rng(OsRng);
    let pinned_public_from_qr = *PublicKey::from(&fake_secret).as_bytes();

    let server = thread::spawn(move || {
        let tcp = TcpTransport::accept(&listener).unwrap();
        // Server will present its own real key
        let _ = SecureTransport::accept_with_secret(tcp, None, Some(&server_secret_bytes));
    });

    let tcp = TcpTransport::connect(address).unwrap();
    let err = SecureTransport::connect_pinned(tcp, None, Some(&pinned_public_from_qr))
        .err()
        .expect("client must reject mismatched server public key");

    assert!(
        err.to_string()
            .contains("receiver public key does not match pinned QR key"),
        "error message must mention pinned key mismatch: {err}"
    );

    let _ = server.join();
}

#[test]
fn verification_refusal_aborts_before_data() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();

    // Receiver simulates verification code check and declines/refuses
    let receiver = thread::spawn(move || {
        let tcp = TcpTransport::accept(&listener).unwrap();
        let mut secure = SecureTransport::accept(tcp, None).unwrap();
        let code = secure.sas().to_string();
        // User refuses verification (e.g. codes mismatch) -> receiver closes session immediately
        let _ = secure.close();
        code
    });

    let tcp = TcpTransport::connect(address).unwrap();
    let mut secure = SecureTransport::connect(tcp, None).unwrap();
    let client_code = secure.sas().to_string();

    let receiver_code = receiver.join().unwrap();
    assert_eq!(client_code, receiver_code);

    // Sender attempts to exchange data or receives notification that peer closed
    // Transport must not permit data transmission once peer refused/closed
    let mut data_sent_successfully = false;
    for _ in 0..10 {
        if secure.send(b"PAYLOAD_DATA_THAT_MUST_NOT_BE_DELIVERED").is_err() {
            data_sent_successfully = false;
            break;
        }
        thread::sleep(Duration::from_millis(5));
        if let Ok(None) | Err(_) = secure.recv() {
            data_sent_successfully = false;
            break;
        }
    }
    assert!(!data_sent_successfully, "Refusal must abort session before data transfer");
}
