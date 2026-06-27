use plenum::protocol::{
    CHECKSUM_LEN, HEADER_LEN, Packet, PacketType, ProtocolError, encode_packet, parse_packet,
};

#[test]
fn encodes_and_decodes_valid_packet() {
    let packet = Packet::new(PacketType::Data, 42, b"hello plenum".to_vec());

    let encoded = encode_packet(&packet).expect("packet should encode");
    let decoded = parse_packet(&encoded).expect("packet should parse");

    assert_eq!(decoded, packet);
}

#[test]
fn supports_empty_payload_packet() {
    let packet = Packet::new(PacketType::Ack, 7, Vec::new());

    let encoded = encode_packet(&packet).expect("packet should encode");
    let decoded = parse_packet(&encoded).expect("packet should parse");

    assert_eq!(decoded, packet);
}

#[test]
fn rejects_unknown_packet_type() {
    let packet = Packet::new(PacketType::Data, 1, b"payload".to_vec());
    let mut encoded = encode_packet(&packet).expect("packet should encode");
    encoded[0] = 0xff;

    let err = parse_packet(&encoded).expect_err("unknown packet type should fail");

    assert_eq!(err, ProtocolError::UnknownPacketType(0xff));
}

#[test]
fn rejects_truncated_header() {
    let err = parse_packet(&[0x01, 0x00]).expect_err("truncated header should fail");

    assert_eq!(
        err,
        ProtocolError::TruncatedHeader {
            actual: 2,
            expected: HEADER_LEN,
        }
    );
}

#[test]
fn rejects_truncated_payload() {
    let packet = Packet::new(PacketType::Data, 1, b"payload".to_vec());
    let encoded = encode_packet(&packet).expect("packet should encode");
    let truncated = &encoded[..HEADER_LEN + 3];

    let err = parse_packet(truncated).expect_err("truncated payload should fail");

    assert_eq!(
        err,
        ProtocolError::TruncatedPayload {
            actual: 3,
            expected: 7,
        }
    );
}

#[test]
fn rejects_truncated_checksum() {
    let packet = Packet::new(PacketType::Data, 1, b"payload".to_vec());
    let encoded = encode_packet(&packet).expect("packet should encode");
    let truncated = &encoded[..encoded.len() - 8];

    let err = parse_packet(truncated).expect_err("truncated checksum should fail");

    assert_eq!(
        err,
        ProtocolError::TruncatedChecksum {
            actual: CHECKSUM_LEN - 8,
            expected: CHECKSUM_LEN,
        }
    );
}

#[test]
fn rejects_invalid_checksum() {
    let packet = Packet::new(PacketType::Data, 1, b"payload".to_vec());
    let mut encoded = encode_packet(&packet).expect("packet should encode");
    let checksum_start = encoded.len() - CHECKSUM_LEN;
    encoded[checksum_start] ^= 0xff;

    let err = parse_packet(&encoded).expect_err("invalid checksum should fail");

    assert_eq!(err, ProtocolError::InvalidChecksum);
}

#[test]
fn rejects_trailing_bytes() {
    let packet = Packet::new(PacketType::Data, 1, b"payload".to_vec());
    let mut encoded = encode_packet(&packet).expect("packet should encode");
    let expected = encoded.len();
    encoded.push(0x00);

    let err = parse_packet(&encoded).expect_err("trailing bytes should fail");

    assert_eq!(
        err,
        ProtocolError::TrailingBytes {
            actual: expected + 1,
            expected,
        }
    );
}
