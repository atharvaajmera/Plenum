//! UDP broadcast beacon for local peer discovery.
//!
//! The receiver broadcasts `Announcement` datagrams on a configurable port so
//! that senders on the same LAN can discover it without knowing its IP address.
//!
//! ## Wire format
//!
//! ```text
//! Magic          4 bytes   "AETH"
//! Version        1 byte    0x01
//! Token Length    1 byte
//! Token          N bytes   UTF-8 pairing code
//! TCP Port       2 bytes   big-endian
//! Hostname Len   1 byte
//! Hostname       M bytes   UTF-8 machine name
//! ```

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use crate::discovery::error::DiscoveryError;
use crate::discovery::token::PairingToken;

const MAGIC: &[u8; 4] = b"AETH";
const VERSION: u8 = 0x01;
const DEFAULT_BROADCAST_PORT: u16 = 41820;
const DEFAULT_BROADCAST_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_DISCOVER_TIMEOUT: Duration = Duration::from_secs(10);

/// An announcement received from a peer on the local network.
#[derive(Debug, Clone)]
pub struct Announcement {
    /// The pairing token broadcast by the peer.
    pub token: String,
    /// The TCP port the peer is listening on for file transfers.
    pub tcp_port: u16,
    /// The hostname of the peer.
    pub hostname: String,
    /// The source IP address the announcement came from.
    pub source_addr: Ipv4Addr,
}

impl Announcement {
    /// Returns the full TCP socket address to connect to for file transfer.
    pub fn tcp_addr(&self) -> SocketAddrV4 {
        SocketAddrV4::new(self.source_addr, self.tcp_port)
    }

    /// Encodes the announcement into its binary wire format.
    pub fn encode(&self) -> Vec<u8> {
        let token_bytes = self.token.as_bytes();
        let hostname_bytes = self.hostname.as_bytes();

        let capacity = 4 + 1 + 1 + token_bytes.len() + 2 + 1 + hostname_bytes.len();
        let mut buf = Vec::with_capacity(capacity);

        buf.extend_from_slice(MAGIC);
        buf.push(VERSION);
        buf.push(token_bytes.len() as u8);
        buf.extend_from_slice(token_bytes);
        buf.extend_from_slice(&self.tcp_port.to_be_bytes());
        buf.push(hostname_bytes.len() as u8);
        buf.extend_from_slice(hostname_bytes);

        buf
    }

    /// Decodes an announcement from a binary datagram.
    pub fn decode(data: &[u8], source: Ipv4Addr) -> Result<Self, DiscoveryError> {
        if data.len() < 7 {
            return Err(DiscoveryError::MalformedAnnouncement);
        }

        if &data[0..4] != MAGIC {
            return Err(DiscoveryError::MalformedAnnouncement);
        }

        if data[4] != VERSION {
            return Err(DiscoveryError::MalformedAnnouncement);
        }

        let token_len = data[5] as usize;
        let token_end = 6 + token_len;
        if data.len() < token_end + 3 {
            return Err(DiscoveryError::MalformedAnnouncement);
        }

        let token = std::str::from_utf8(&data[6..token_end])
            .map_err(|_| DiscoveryError::MalformedAnnouncement)?
            .to_string();

        let tcp_port = u16::from_be_bytes([data[token_end], data[token_end + 1]]);

        let hostname_len = data[token_end + 2] as usize;
        let hostname_end = token_end + 3 + hostname_len;
        if data.len() < hostname_end {
            return Err(DiscoveryError::MalformedAnnouncement);
        }

        let hostname = std::str::from_utf8(&data[token_end + 3..hostname_end])
            .map_err(|_| DiscoveryError::MalformedAnnouncement)?
            .to_string();

        Ok(Self {
            token,
            tcp_port,
            hostname,
            source_addr: source,
        })
    }
}

/// Configuration for the discovery beacon.
#[derive(Debug, Clone)]
pub struct BeaconConfig {
    /// The UDP port used for broadcast announcements.
    pub broadcast_port: u16,
    /// How often to re-broadcast the announcement.
    pub broadcast_interval: Duration,
    /// How long to listen for announcements before giving up.
    pub discover_timeout: Duration,
}

impl Default for BeaconConfig {
    fn default() -> Self {
        Self {
            broadcast_port: DEFAULT_BROADCAST_PORT,
            broadcast_interval: DEFAULT_BROADCAST_INTERVAL,
            discover_timeout: DEFAULT_DISCOVER_TIMEOUT,
        }
    }
}

/// Manages broadcasting and discovering peers on the local network.
#[derive(Debug)]
pub struct Beacon {
    config: BeaconConfig,
}

impl Beacon {
    pub fn new() -> Self {
        Self {
            config: BeaconConfig::default(),
        }
    }

    pub fn with_config(config: BeaconConfig) -> Self {
        Self { config }
    }

    /// Broadcasts the receiver's availability on the LAN. This function blocks
    /// and sends periodic UDP broadcast datagrams until the returned handle
    /// is stopped or the token expires.
    ///
    /// `tcp_port` is the port the receiver is listening on for file transfers.
    pub fn broadcast(
        &self,
        token: &PairingToken,
        tcp_port: u16,
    ) -> Result<BroadcastHandle, DiscoveryError> {
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_broadcast(true)?;

        let hostname = hostname();

        let announcement = Announcement {
            token: token.code().to_string(),
            tcp_port,
            hostname,
            source_addr: Ipv4Addr::UNSPECIFIED, // Not used for encoding
        };

        let datagram = announcement.encode();
        let dest = SocketAddrV4::new(Ipv4Addr::BROADCAST, self.config.broadcast_port);

        Ok(BroadcastHandle {
            socket,
            datagram,
            dest,
            interval: self.config.broadcast_interval,
        })
    }

    /// Listens for peer announcements on the LAN. Returns the first valid
    /// announcement found, or an error if the timeout is reached.
    pub fn discover(&self) -> Result<Announcement, DiscoveryError> {
        let socket = UdpSocket::bind(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            self.config.broadcast_port,
        ))?;
        socket.set_read_timeout(Some(Duration::from_millis(500)))?;

        let deadline = Instant::now() + self.config.discover_timeout;
        let mut buf = [0u8; 512];

        while Instant::now() < deadline {
            match socket.recv_from(&mut buf) {
                Ok((len, src_addr)) => {
                    let source_ip = match src_addr {
                        SocketAddr::V4(v4) => *v4.ip(),
                        SocketAddr::V6(_) => continue,
                    };

                    match Announcement::decode(&buf[..len], source_ip) {
                        Ok(announcement) => return Ok(announcement),
                        Err(_) => continue, // Skip malformed datagrams
                    }
                }
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        }

        Err(DiscoveryError::NoPeersFound)
    }

    /// Listens for a peer announcement matching a specific pairing token.
    pub fn discover_with_token(
        &self,
        expected_token: &str,
    ) -> Result<Announcement, DiscoveryError> {
        let socket = UdpSocket::bind(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            self.config.broadcast_port,
        ))?;
        socket.set_read_timeout(Some(Duration::from_millis(500)))?;

        let deadline = Instant::now() + self.config.discover_timeout;
        let mut buf = [0u8; 512];

        while Instant::now() < deadline {
            match socket.recv_from(&mut buf) {
                Ok((len, src_addr)) => {
                    let source_ip = match src_addr {
                        SocketAddr::V4(v4) => *v4.ip(),
                        SocketAddr::V6(_) => continue,
                    };

                    match Announcement::decode(&buf[..len], source_ip) {
                        Ok(announcement) if announcement.token == expected_token => {
                            return Ok(announcement);
                        }
                        _ => continue,
                    }
                }
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        }

        Err(DiscoveryError::NoPeersFound)
    }
}

impl Default for Beacon {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle returned by `Beacon::broadcast` that can send periodic announcements.
#[derive(Debug)]
pub struct BroadcastHandle {
    socket: UdpSocket,
    datagram: Vec<u8>,
    dest: SocketAddrV4,
    interval: Duration,
}

impl BroadcastHandle {
    /// Sends one broadcast datagram. Call this periodically from your event loop.
    pub fn send_once(&self) -> Result<(), DiscoveryError> {
        self.socket.send_to(&self.datagram, self.dest)?;
        Ok(())
    }

    /// Returns the configured broadcast interval.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Blocks and broadcasts periodically until `stop` is called or the duration
    /// elapses. Useful for simple single-threaded usage.
    pub fn broadcast_for(&self, duration: Duration) -> Result<u32, DiscoveryError> {
        let start = Instant::now();
        let mut count = 0;

        while start.elapsed() < duration {
            self.send_once()?;
            count += 1;

            let remaining = duration.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                break;
            }
            std::thread::sleep(self.interval.min(remaining));
        }

        Ok(count)
    }
}

/// Returns the hostname of the local machine, or "unknown" if it cannot be determined.
fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
