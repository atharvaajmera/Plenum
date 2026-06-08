//! Stream reassembly utilities.

use std::collections::BTreeMap;

use crate::protocol::{Packet, PacketType};
use crate::stream::StreamError;

/// Reassembles `Data` packets into their original byte order.
#[derive(Debug, Default)]
pub struct Reassembler {
    packets: BTreeMap<u32, Vec<u8>>,
}

impl Reassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a data packet into the reassembly buffer.
    pub fn insert(&mut self, packet: Packet) -> Result<(), StreamError> {
        if packet.packet_type != PacketType::Data {
            return Err(StreamError::UnexpectedPacketType {
                actual: packet.packet_type,
            });
        }

        if self.packets.contains_key(&packet.sequence_no) {
            return Err(StreamError::DuplicateSequence {
                sequence_no: packet.sequence_no,
            });
        }

        self.packets.insert(packet.sequence_no, packet.payload);
        Ok(())
    }

    /// Returns the first missing sequence number between zero and the highest
    /// sequence number seen so far.
    pub fn first_missing_sequence(&self) -> Option<u32> {
        let highest = *self.packets.keys().next_back()?;
        (0..=highest).find(|sequence_no| !self.packets.contains_key(sequence_no))
    }

    /// Emits the complete reassembled byte stream.
    pub fn finish(self) -> Result<Vec<u8>, StreamError> {
        if let Some(sequence_no) = self.first_missing_sequence() {
            return Err(StreamError::MissingSequence { sequence_no });
        }

        let total_len = self.packets.values().map(Vec::len).sum();
        let mut bytes = Vec::with_capacity(total_len);

        for payload in self.packets.into_values() {
            bytes.extend_from_slice(&payload);
        }

        Ok(bytes)
    }
}

/// Reassembles a collection of packets into the original byte stream.
///
/// Packets may be supplied out of order. Duplicate, missing, or non-data packets
/// are rejected.
pub fn reassemble_packets(
    packets: impl IntoIterator<Item = Packet>,
) -> Result<Vec<u8>, StreamError> {
    let mut reassembler = Reassembler::new();

    for packet in packets {
        reassembler.insert(packet)?;
    }

    reassembler.finish()
}
