use plenum::protocol::{Packet, PacketType, encode_packet, parse_packet};
use plenum::transport::{MemoryTransport, MemoryTransportConfig, Transport, TransportError};

#[test]
fn sends_and_receives_frames_immediately_by_default() {
    let mut transport = MemoryTransport::default();

    transport.send(b"frame-1").expect("send should succeed");
    transport.send(b"frame-2").expect("send should succeed");

    assert_eq!(
        transport.recv().expect("recv should succeed"),
        Some(b"frame-1".to_vec())
    );
    assert_eq!(
        transport.recv().expect("recv should succeed"),
        Some(b"frame-2".to_vec())
    );
    assert_eq!(transport.recv().expect("recv should succeed"), None);
}

#[test]
fn applies_latency_until_ticks_advance() {
    let mut transport = MemoryTransport::new(MemoryTransportConfig {
        latency_ticks: 2,
        ..MemoryTransportConfig::default()
    });

    transport.send(b"delayed").expect("send should succeed");

    assert_eq!(transport.recv().expect("recv should succeed"), None);
    transport.tick();
    assert_eq!(transport.recv().expect("recv should succeed"), None);
    transport.tick();
    assert_eq!(
        transport.recv().expect("recv should succeed"),
        Some(b"delayed".to_vec())
    );
}

#[test]
fn drops_every_configured_send_attempt() {
    let mut transport = MemoryTransport::new(MemoryTransportConfig {
        drop_every: Some(2),
        ..MemoryTransportConfig::default()
    });

    transport.send(b"one").expect("send should succeed");
    transport.send(b"two").expect("dropped send still succeeds");
    transport.send(b"three").expect("send should succeed");

    assert_eq!(
        transport.recv().expect("recv should succeed"),
        Some(b"one".to_vec())
    );
    assert_eq!(
        transport.recv().expect("recv should succeed"),
        Some(b"three".to_vec())
    );
    assert_eq!(transport.recv().expect("recv should succeed"), None);
}

#[test]
fn duplicates_every_configured_accepted_send() {
    let mut transport = MemoryTransport::new(MemoryTransportConfig {
        duplicate_every: Some(2),
        ..MemoryTransportConfig::default()
    });

    transport.send(b"one").expect("send should succeed");
    transport.send(b"two").expect("send should succeed");

    assert_eq!(
        transport.recv().expect("recv should succeed"),
        Some(b"one".to_vec())
    );
    assert_eq!(
        transport.recv().expect("recv should succeed"),
        Some(b"two".to_vec())
    );
    assert_eq!(
        transport.recv().expect("recv should succeed"),
        Some(b"two".to_vec())
    );
    assert_eq!(transport.recv().expect("recv should succeed"), None);
}

#[test]
fn can_reorder_configured_frames() {
    let mut transport = MemoryTransport::new(MemoryTransportConfig {
        reorder_every: Some(2),
        ..MemoryTransportConfig::default()
    });

    transport.send(b"one").expect("send should succeed");
    transport.send(b"two").expect("send should succeed");
    transport.send(b"three").expect("send should succeed");

    assert_eq!(
        transport.recv().expect("recv should succeed"),
        Some(b"one".to_vec())
    );
    assert_eq!(
        transport.recv().expect("recv should succeed"),
        Some(b"three".to_vec())
    );
    assert_eq!(transport.recv().expect("recv should succeed"), None);

    transport.tick();

    assert_eq!(
        transport.recv().expect("recv should succeed"),
        Some(b"two".to_vec())
    );
}

#[test]
fn enforces_buffer_limits() {
    let mut transport = MemoryTransport::new(MemoryTransportConfig {
        latency_ticks: 5,
        max_buffered_frames: Some(1),
        ..MemoryTransportConfig::default()
    });

    transport.send(b"one").expect("first frame should fit");
    let err = transport
        .send(b"two")
        .expect_err("second frame should exceed capacity");

    assert_eq!(
        err,
        TransportError::BufferFull {
            capacity: 1,
            requested: 1,
        }
    );
}

#[test]
fn rejects_send_and_recv_after_close() {
    let mut transport = MemoryTransport::default();

    transport.send(b"queued").expect("send should succeed");
    transport.close().expect("close should succeed");

    assert_eq!(transport.send(b"after-close"), Err(TransportError::Closed));
    assert_eq!(transport.recv(), Err(TransportError::Closed));
    assert!(transport.is_closed());
    assert_eq!(transport.buffered_len(), 0);
}

#[test]
fn carries_encoded_protocol_packets() {
    let packet = Packet::new(PacketType::Data, 9, b"payload".to_vec());
    let encoded = encode_packet(&packet).expect("packet should encode");
    let mut transport = MemoryTransport::default();

    transport.send(&encoded).expect("send should succeed");
    let received = transport
        .recv()
        .expect("recv should succeed")
        .expect("frame should be available");
    let parsed = parse_packet(&received).expect("packet should parse");

    assert_eq!(parsed, packet);
}
