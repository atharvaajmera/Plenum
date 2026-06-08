//! Packet type and layout definitions.

use crate::protocol::ProtocolError;

/// Number of bytes in the fixed packet header.
///
/// Layout:
/// - packet type: 1 byte
/// - sequence number: 4 bytes
/// - payload length: 4 bytes
pub const HEADER_LEN: usize = 9;

/// Number of bytes in the packet checksum.
pub const CHECKSUM_LEN: usize = 32;

/// Packet categories supported by the Aether protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    Data,
    Ack,
    Nack,
    Start,
    Finish,
    Close,
}

impl PacketType {
    pub(crate) fn as_u8(self) -> u8 {
        match self {
            Self::Data => 0x01,
            Self::Ack => 0x02,
            Self::Nack => 0x03,
            Self::Start => 0x04,
            Self::Finish => 0x05,
            Self::Close => 0x06,
        }
    }

    pub(crate) fn from_u8(value: u8) -> Result<Self, ProtocolError> {
        match value {
            0x01 => Ok(Self::Data),
            0x02 => Ok(Self::Ack),
            0x03 => Ok(Self::Nack),
            0x04 => Ok(Self::Start),
            0x05 => Ok(Self::Finish),
            0x06 => Ok(Self::Close),
            other => Err(ProtocolError::UnknownPacketType(other)),
        }
    }
}

/// A decoded Aether protocol packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub packet_type: PacketType,
    pub sequence_no: u32,
    pub payload: Vec<u8>,
}

impl Packet {
    pub fn new(packet_type: PacketType, sequence_no: u32, payload: impl Into<Vec<u8>>) -> Self {
        Self {
            packet_type,
            sequence_no,
            payload: payload.into(),
        }
    }
}
