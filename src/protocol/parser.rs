//! Packet parsing utilities.

use crate::protocol::ProtocolError;
use crate::protocol::checksum::compute_checksum;
use crate::protocol::packet::{CHECKSUM_LEN, HEADER_LEN, Packet, PacketType};

/// Parses one complete packet from the Plenum binary wire format.
///
/// The input must contain exactly one packet. Extra bytes are rejected so callers
/// do not accidentally ignore malformed framing boundaries.
pub fn parse_packet(bytes: &[u8]) -> Result<Packet, ProtocolError> {
    if bytes.len() < HEADER_LEN {
        return Err(ProtocolError::TruncatedHeader {
            actual: bytes.len(),
            expected: HEADER_LEN,
        });
    }

    let packet_type = PacketType::from_u8(bytes[0])?;
    let sequence_no = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
    let payload_len = u32::from_be_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
    let payload_start = HEADER_LEN;
    let payload_end = payload_start + payload_len;

    if bytes.len() < payload_end {
        return Err(ProtocolError::TruncatedPayload {
            actual: bytes.len().saturating_sub(payload_start),
            expected: payload_len,
        });
    }

    let checksum_end = payload_end + CHECKSUM_LEN;
    if bytes.len() < checksum_end {
        return Err(ProtocolError::TruncatedChecksum {
            actual: bytes.len().saturating_sub(payload_end),
            expected: CHECKSUM_LEN,
        });
    }

    if bytes.len() > checksum_end {
        return Err(ProtocolError::TrailingBytes {
            actual: bytes.len(),
            expected: checksum_end,
        });
    }

    let expected_checksum = compute_checksum(&bytes[..payload_end]);
    let actual_checksum = &bytes[payload_end..checksum_end];

    if actual_checksum != expected_checksum {
        return Err(ProtocolError::InvalidChecksum);
    }

    Ok(Packet::new(
        packet_type,
        sequence_no,
        bytes[payload_start..payload_end].to_vec(),
    ))
}
