//! Local peer discovery via UDP broadcast.
//!
//! Peers broadcast short-lived announcements on the local network so that
//! senders and receivers can find each other without manually exchanging
//! IP addresses.

pub mod beacon;
pub mod error;
pub mod token;

pub use beacon::{Announcement, Beacon};
pub use error::DiscoveryError;
pub use token::PairingToken;
