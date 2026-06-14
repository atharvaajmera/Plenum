use aether::flow::{FlowError, ReceiverWindow, SenderWindow};
use aether::protocol::{Packet, PacketType, encode_packet, parse_packet};
use aether::stream::chunk_bytes;
use aether::transport::{MemoryTransport, MemoryTransportConfig, Transport};

#[test]
fn sender_respects_window_capacity_and_advances_on_ack() {
    let packets = chunk_bytes(b"abcdefghijklmnopqrstuvwxyz", 5).expect("chunking should succeed");
    let mut sender = SenderWindow::new(2, 10).expect("sender window should build");
    for packet in packets {
        sender.enqueue(packet).expect("enqueue should succeed");
    }
    let mut transport = MemoryTransport::default();

    let sent = sender
        .send_available(&mut transport, 0)
        .expect("send should succeed");

    assert_eq!(sent, 2);
    assert_eq!(sender.in_flight_len(), 2);

    let first = parse_packet(
        &transport
            .recv()
            .expect("recv should succeed")
            .expect("first packet should be ready"),
    )
    .expect("packet should parse");
    let second = parse_packet(
        &transport
            .recv()
            .expect("recv should succeed")
            .expect("second packet should be ready"),
    )
    .expect("packet should parse");

    assert_eq!(first.sequence_no, 0);
    assert_eq!(second.sequence_no, 1);
    assert_eq!(transport.recv().expect("recv should succeed"), None);

    sender
        .handle_control_packet(&Packet::new(PacketType::Ack, 0, Vec::new()))
        .expect("ack should be accepted");

    let sent = sender
        .send_available(&mut transport, 1)
        .expect("send should succeed");

    assert_eq!(sent, 1);
    assert_eq!(sender.in_flight_len(), 2);

    let third = parse_packet(
        &transport
            .recv()
            .expect("recv should succeed")
            .expect("third packet should be ready"),
    )
    .expect("packet should parse");

    assert_eq!(third.sequence_no, 2);
}

#[test]
fn sender_retransmits_after_timeout() {
    let packets = chunk_bytes(b"timeout", 32).expect("chunking should succeed");
    let mut sender = SenderWindow::new(1, 3).expect("sender window should build");
    for packet in packets {
        sender.enqueue(packet).expect("enqueue should succeed");
    }
    let mut transport = MemoryTransport::default();

    sender
        .send_available(&mut transport, 0)
        .expect("initial send should succeed");
    let original = transport
        .recv()
        .expect("recv should succeed")
        .expect("initial packet should be ready");

    assert_eq!(
        sender
            .retransmit_due(&mut transport, 2)
            .expect("retransmit check should succeed"),
        0
    );
    assert_eq!(transport.recv().expect("recv should succeed"), None);

    assert_eq!(
        sender
            .retransmit_due(&mut transport, 3)
            .expect("retransmit should succeed"),
        1
    );

    let retransmitted = transport
        .recv()
        .expect("recv should succeed")
        .expect("retransmitted packet should be ready");

    assert_eq!(retransmitted, original);
}

#[test]
fn sender_retransmits_after_nack() {
    let packets = chunk_bytes(b"nack", 32).expect("chunking should succeed");
    let mut sender = SenderWindow::new(1, 100).expect("sender window should build");
    for packet in packets {
        sender.enqueue(packet).expect("enqueue should succeed");
    }
    let mut transport = MemoryTransport::default();

    sender
        .send_available(&mut transport, 0)
        .expect("initial send should succeed");
    let original = transport
        .recv()
        .expect("recv should succeed")
        .expect("initial packet should be ready");

    sender
        .handle_control_packet(&Packet::new(PacketType::Nack, 0, Vec::new()))
        .expect("nack should be accepted");
    assert_eq!(
        sender
            .retransmit_due(&mut transport, 1)
            .expect("nack retransmit should succeed"),
        1
    );

    let retransmitted = transport
        .recv()
        .expect("recv should succeed")
        .expect("retransmitted packet should be ready");

    assert_eq!(retransmitted, original);
}

#[test]
fn receiver_acks_buffers_and_nacks_missing_sequence() {
    let mut receiver = ReceiverWindow::new();
    let seq_one = Packet::new(PacketType::Data, 1, b"one".to_vec());

    let controls = receiver
        .receive_data_packet(seq_one)
        .expect("receiver should accept data");

    assert_eq!(controls.len(), 2);
    assert_eq!(controls[0].packet_type, PacketType::Ack);
    assert_eq!(controls[0].sequence_no, 1);
    assert_eq!(controls[1].packet_type, PacketType::Nack);
    assert_eq!(controls[1].sequence_no, 0);
    assert!(receiver.drain_ordered().is_empty());

    let controls = receiver
        .receive_data_packet(Packet::new(PacketType::Data, 0, b"zero".to_vec()))
        .expect("receiver should accept missing data");

    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0].packet_type, PacketType::Ack);
    assert_eq!(controls[0].sequence_no, 0);

    let drained = receiver.drain_ordered();

    assert_eq!(drained, vec![b"zero".to_vec(), b"one".to_vec()]);
    assert_eq!(receiver.next_expected(), 2);
}

#[test]
fn rejects_invalid_flow_configuration_and_packets() {
    let packets = chunk_bytes(b"data", 2).expect("chunking should succeed");
    let mut sender = SenderWindow::new(0, 10);
    let err = sender.expect_err("zero window should fail");
    assert_eq!(err, FlowError::InvalidWindowSize);

    let mut sender = SenderWindow::new(1, 10).expect("sender window should build");
    let err = sender
        .enqueue(Packet::new(PacketType::Ack, 0, Vec::new()))
        .expect_err("non-data sender packet should fail");
    assert_eq!(
        err,
        FlowError::UnexpectedPacketType {
            actual: PacketType::Ack,
        }
    );

    let mut receiver = ReceiverWindow::new();
    let err = receiver
        .receive_data_packet(Packet::new(PacketType::Ack, 0, Vec::new()))
        .expect_err("non-data receiver packet should fail");
    assert_eq!(
        err,
        FlowError::UnexpectedPacketType {
            actual: PacketType::Ack,
        }
    );
}

#[test]
fn completes_transfer_over_simulated_latency_reordering_and_duplication() {
    let original = b"sliding window keeps multiple packets in flight while preserving order";
    let packets = chunk_bytes(original, 8).expect("chunking should succeed");
    let mut sender = SenderWindow::new(3, 6).expect("sender window should build");
    for packet in packets {
        sender.enqueue(packet).expect("enqueue should succeed");
    }
    let mut receiver = ReceiverWindow::new();
    let mut data_transport = MemoryTransport::new(MemoryTransportConfig {
        latency_ticks: 1,
        duplicate_every: Some(3),
        reorder_every: Some(2),
        ..MemoryTransportConfig::default()
    });
    let mut control_transport = MemoryTransport::new(MemoryTransportConfig {
        latency_ticks: 1,
        ..MemoryTransportConfig::default()
    });
    let mut restored = Vec::new();

    for now in 0..100 {
        sender
            .retransmit_due(&mut data_transport, now)
            .expect("retransmit check should succeed");
        sender
            .send_available(&mut data_transport, now)
            .expect("send should succeed");

        while let Some(frame) = data_transport.recv().expect("data recv should succeed") {
            let packet = parse_packet(&frame).expect("data packet should parse");
            let controls = receiver
                .receive_data_packet(packet)
                .expect("receiver should accept packet");

            for payload in receiver.drain_ordered() {
                restored.extend_from_slice(&payload);
            }

            for control in controls {
                let encoded = encode_packet(&control).expect("control should encode");
                control_transport
                    .send(&encoded)
                    .expect("control send should succeed");
            }
        }

        while let Some(frame) = control_transport
            .recv()
            .expect("control recv should succeed")
        {
            let control = parse_packet(&frame).expect("control packet should parse");
            sender
                .handle_control_packet(&control)
                .expect("control packet should be handled");
        }

        if sender.is_empty() && restored == original {
            return;
        }

        data_transport.tick();
        control_transport.tick();
    }

    panic!(
        "transfer did not complete; restored {} of {} bytes",
        restored.len(),
        original.len()
    );
}
