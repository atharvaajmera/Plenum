//! Stream chunking utilities.

use crate::protocol::{Packet, PacketType};
use crate::stream::StreamError;

/// Splits raw bytes into ordered `Data` packets.
///
/// Sequence numbers start at zero and increase by one for every chunk.
pub fn chunk_bytes(bytes: &[u8], chunk_size: usize) -> Result<Vec<Packet>, StreamError> {
    if chunk_size == 0 {
        return Err(StreamError::InvalidChunkSize);
    }

    let chunk_count = bytes.chunks(chunk_size).count();
    if chunk_count > u32::MAX as usize + 1 {
        return Err(StreamError::TooManyChunks {
            chunks: chunk_count,
        });
    }

    bytes
        .chunks(chunk_size)
        .enumerate()
        .map(|(sequence_no, chunk)| {
            Ok(Packet::new(
                PacketType::Data,
                sequence_no as u32,
                chunk.to_vec(),
            ))
        })
        .collect()
}
