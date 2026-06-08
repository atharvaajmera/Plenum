//! Packet encoding utilities.

use crate::protocol::ProtocolError;
use crate::protocol::checksum::compute_checksum;
use crate::protocol::packet::{CHECKSUM_LEN, HEADER_LEN, Packet};

/// Encodes a packet into the Aether binary wire format.
pub fn encode_packet(packet: &Packet) -> Result<Vec<u8>, ProtocolError> {
    let payload_len =
        u32::try_from(packet.payload.len()).map_err(|_| ProtocolError::PayloadTooLarge {
            len: packet.payload.len(),
        })?;

    let packet_len = HEADER_LEN + packet.payload.len() + CHECKSUM_LEN;
    let mut bytes = Vec::with_capacity(packet_len);

    bytes.push(packet.packet_type.as_u8());
    bytes.extend_from_slice(&packet.sequence_no.to_be_bytes());
    bytes.extend_from_slice(&payload_len.to_be_bytes());
    bytes.extend_from_slice(&packet.payload);

    let checksum = compute_checksum(&bytes);
    bytes.extend_from_slice(&checksum);

    Ok(bytes)
}
