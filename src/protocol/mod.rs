//! Binary protocol framing and parsing.

pub mod checksum;
pub mod encoder;
pub mod error;
pub mod packet;
pub mod parser;

pub use encoder::encode_packet;
pub use error::ProtocolError;
pub use packet::{CHECKSUM_LEN, HEADER_LEN, Packet, PacketType};
pub use parser::parse_packet;
