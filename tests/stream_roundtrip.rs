use plenum::protocol::{PacketType, encode_packet, parse_packet};
use plenum::stream::{StreamError, chunk_bytes};

#[test]
fn chunks_small_buffer_with_sequence_numbers() {
    let packets = chunk_bytes(b"hello world", 5).expect("chunking should succeed");

    assert_eq!(packets.len(), 3);
    assert_eq!(packets[0].sequence_no, 0);
    assert_eq!(packets[0].payload, b"hello");
    assert_eq!(packets[1].sequence_no, 1);
    assert_eq!(packets[1].payload, b" worl");
    assert_eq!(packets[2].sequence_no, 2);
    assert_eq!(packets[2].payload, b"d");
    assert!(
        packets
            .iter()
            .all(|packet| packet.packet_type == PacketType::Data)
    );
}

#[test]
fn rejects_zero_chunk_size() {
    let err = chunk_bytes(b"data", 0).expect_err("zero chunk size should fail");

    assert_eq!(err, StreamError::InvalidChunkSize);
}

#[test]
fn reassembles_large_buffer() {
    let original: Vec<u8> = (0..10_000).map(|value| (value % 251) as u8).collect();
    let packets = chunk_bytes(&original, 1024).expect("chunking should succeed");

    let mut restored = Vec::new();
    for packet in packets {
        restored.extend_from_slice(&packet.payload);
    }

    assert_eq!(restored, original);
}

#[test]
fn handles_final_partial_chunk() {
    let packets = chunk_bytes(b"abcdefghij", 4).expect("chunking should succeed");

    let payload_lengths: Vec<usize> = packets.iter().map(|packet| packet.payload.len()).collect();

    assert_eq!(payload_lengths, vec![4, 4, 2]);
}

#[test]
fn chunks_encode_parse_and_reassemble_original_bytes() {
    let original = b"phase two proves packet framing can carry stream chunks";
    let packets = chunk_bytes(original, 7).expect("chunking should succeed");

    let parsed_packets: Vec<_> = packets
        .iter()
        .map(|packet| {
            let encoded = encode_packet(packet).expect("packet should encode");
            parse_packet(&encoded).expect("packet should parse")
        })
        .collect();

    let mut restored = Vec::new();
    for packet in parsed_packets {
        restored.extend_from_slice(&packet.payload);
    }

    assert_eq!(restored, original);
}
