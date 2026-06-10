use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use aether::protocol::{Packet, PacketType, encode_packet, parse_packet};
use aether::transport::{TcpTransport, Transport, TransportError};

fn wait_recv(transport: &mut TcpTransport) -> Vec<u8> {
    for _ in 0..100 {
        if let Some(frame) = transport.recv().expect("recv should not fail") {
            return frame;
        }

        thread::sleep(Duration::from_millis(5));
    }

    panic!("timed out waiting for TCP frame");
}

#[test]
fn exchanges_length_prefixed_frames_over_localhost() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let addr = listener
        .local_addr()
        .expect("listener should have local addr");

    let server = thread::spawn(move || {
        let mut server = TcpTransport::accept(&listener).expect("server should accept client");
        let received = wait_recv(&mut server);
        assert_eq!(received, b"ping".to_vec());
        server.send(b"pong").expect("server send should succeed");
    });

    let mut client = TcpTransport::connect(addr).expect("client should connect");
    client.send(b"ping").expect("client send should succeed");

    let response = wait_recv(&mut client);

    assert_eq!(response, b"pong".to_vec());
    server.join().expect("server thread should finish");
}

#[test]
fn carries_encoded_protocol_packets_over_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let addr = listener
        .local_addr()
        .expect("listener should have local addr");
    let packet = Packet::new(PacketType::Data, 12, b"tcp payload".to_vec());
    let expected = packet.clone();

    let server = thread::spawn(move || {
        let mut server = TcpTransport::accept(&listener).expect("server should accept client");
        let frame = wait_recv(&mut server);
        let parsed = parse_packet(&frame).expect("packet should parse");
        assert_eq!(parsed, expected);

        let ack = Packet::new(PacketType::Ack, parsed.sequence_no, Vec::new());
        let encoded_ack = encode_packet(&ack).expect("ack should encode");
        server
            .send(&encoded_ack)
            .expect("server ack send should succeed");
    });

    let mut client = TcpTransport::connect(addr).expect("client should connect");
    let encoded = encode_packet(&packet).expect("packet should encode");
    client.send(&encoded).expect("client send should succeed");

    let ack_frame = wait_recv(&mut client);
    let ack = parse_packet(&ack_frame).expect("ack should parse");

    assert_eq!(ack.packet_type, PacketType::Ack);
    assert_eq!(ack.sequence_no, 12);
    server.join().expect("server thread should finish");
}

#[test]
fn rejects_frames_larger_than_configured_limit() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let addr = listener
        .local_addr()
        .expect("listener should have local addr");

    let server = thread::spawn(move || {
        let _server = TcpTransport::accept(&listener).expect("server should accept client");
    });

    let mut client = TcpTransport::connect(addr).expect("client should connect");
    client.set_max_frame_len(3);

    let err = client
        .send(b"four")
        .expect_err("oversized frame should fail");

    assert_eq!(err, TransportError::FrameTooLarge { len: 4, max: 3 });
    drop(client);
    server.join().expect("server thread should finish");
}
