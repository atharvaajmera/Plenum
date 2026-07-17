//! Stable app-facing request, result, and event types.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

/// Pending/accept/decline state of an incoming-transfer approval (see
/// [`SessionControl`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptDecision {
    Pending,
    Accepted,
    Declined,
}

/// Shared, thread-safe control handle for one engine session.
///
/// Engine calls (`send_file`, `receive_file`, ...) are blocking and
/// run-to-completion; this handle is the side channel a UI thread uses to
/// influence them while they run:
///
/// - `cancel()` — cooperative cancellation. The transfer/listen loops poll
///   [`Self::is_cancelled`] and abort with [`crate::app::AppError::Cancelled`].
/// - `accept()` / `decline()` — resolves the incoming-file approval gate the
///   receiver blocks on after emitting `TransferEvent::IncomingRequest`.
///
/// Obtain it via [`crate::app::engine::PlenumCore::control`] before starting
/// the blocking call (it is `Clone`; all clones share state).
#[derive(Debug, Clone, Default)]
pub struct SessionControl {
    cancelled: Arc<AtomicBool>,
    /// 0 = pending, 1 = accepted, 2 = declined.
    decision: Arc<AtomicU8>,
}

impl SessionControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn accept(&self) {
        self.decision.store(1, Ordering::Relaxed);
    }

    pub fn decline(&self) {
        self.decision.store(2, Ordering::Relaxed);
    }

    pub fn decision(&self) -> AcceptDecision {
        match self.decision.load(Ordering::Relaxed) {
            1 => AcceptDecision::Accepted,
            2 => AcceptDecision::Declined,
            _ => AcceptDecision::Pending,
        }
    }

    /// Clears a previous accept/decline so the next incoming file on the same
    /// session prompts again.
    pub fn reset_decision(&self) {
        self.decision.store(0, Ordering::Relaxed);
    }

    /// Raw cancellation flag, for layers (e.g. `crate::rtc`) that must not
    /// depend on app types.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionKind {
    LocalNetwork,
    FileSystemRead,
    FileSystemWrite,
    BackgroundTransfer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorePermissions {
    pub local_network: bool,
    pub file_system_read: bool,
    pub file_system_write: bool,
    pub background_transfer: bool,
}

impl CorePermissions {
    pub fn full() -> Self {
        Self {
            local_network: true,
            file_system_read: true,
            file_system_write: true,
            background_transfer: true,
        }
    }

    pub fn desktop_defaults() -> Self {
        Self::full()
    }

    pub fn mobile_defaults() -> Self {
        Self {
            local_network: true,
            file_system_read: true,
            file_system_write: true,
            background_transfer: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferOptions {
    pub chunk_size: usize,
    pub window_size: usize,
    pub timeout_ticks: u64,
}

impl Default for TransferOptions {
    fn default() -> Self {
        Self {
            chunk_size: 32 * 1024,
            window_size: 128,
            // Milliseconds before an unacknowledged packet is retransmitted.
            // Deliberately generous: the WebRTC data channel is already
            // reliable/ordered, so retransmission at this layer only matters
            // for genuinely lost connections. An aggressive value (e.g. 1s)
            // fires on packets that are merely queued in SCTP, snowballing
            // into a duplicate storm that stalls the transfer.
            timeout_ticks: 15_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendRequest {
    pub file_path: PathBuf,
    pub address: Option<String>,
    pub discovery_token: Option<String>,
    pub permissions: CorePermissions,
    pub options: TransferOptions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiveRequest {
    pub port: u16,
    pub output_dir: PathBuf,
    pub announce_on_lan: bool,
    pub device_name: Option<String>,
    /// When true, senders must prove knowledge of the pairing code (via an
    /// `Auth` packet) before a transfer is accepted, and the beacon announces
    /// `pin_required` instead of broadcasting the code itself.
    #[serde(default)]
    pub require_pin: bool,
    /// When false, the engine emits `TransferEvent::IncomingRequest` on the
    /// first `Start` packet and blocks until the app resolves it via
    /// [`SessionControl::accept`]/[`SessionControl::decline`]. Defaults to
    /// true (auto-accept) so existing CLI/desktop callers keep their behavior.
    #[serde(default = "default_true")]
    pub auto_accept: bool,
    pub permissions: CorePermissions,
    pub options: TransferOptions,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverRequest {
    pub token: Option<String>,
    pub timeout_secs: u64,
    pub permissions: CorePermissions,
}

/// Sends a file over the internet via a relay/signaling server, negotiating a
/// WebRTC data channel (see `crate::rtc`) instead of connecting directly over
/// LAN. The sender acts as the WebRTC offerer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendRemoteRequest {
    pub file_path: PathBuf,
    pub relay_server_url: String,
    pub session_id: String,
    pub my_peer_id: String,
    pub ice_servers: Vec<crate::signaling::IceServer>,
    pub connect_timeout_secs: u64,
    pub permissions: CorePermissions,
    pub options: TransferOptions,
}

/// Receives a file over the internet via a relay/signaling server, negotiating
/// a WebRTC data channel (see `crate::rtc`) instead of listening on LAN. The
/// receiver acts as the WebRTC answerer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiveRemoteRequest {
    pub output_dir: PathBuf,
    pub relay_server_url: String,
    pub session_id: String,
    pub my_peer_id: String,
    pub ice_servers: Vec<crate::signaling::IceServer>,
    pub connect_timeout_secs: u64,
    /// See [`ReceiveRequest::auto_accept`].
    #[serde(default = "default_true")]
    pub auto_accept: bool,
    pub permissions: CorePermissions,
    pub options: TransferOptions,
}

/// Generates a human-shareable room code used as the signaling `session_id`
/// for internet transfers. Longer than the LAN pairing PIN since this code
/// doubles as the actual session secret on a public relay server.
pub fn generate_room_code() -> String {
    crate::discovery::PairingToken::generate_with_len(9)
        .code()
        .to_string()
}

/// Generates a random per-connection peer identifier for internet transfers.
/// Never shown to the user; purely a wire-protocol identifier.
pub fn generate_peer_id() -> String {
    crate::security::SessionId::generate().to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkRequest {
    pub size_mb: usize,
    pub iterations: usize,
    pub latency_ticks: u64,
    pub options: TransferOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    Send,
    Receive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    Discovering,
    Listening,
    Connecting,
    SignalingConnected,
    NegotiatingIce,
    Connected,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferSummary {
    pub direction: TransferDirection,
    pub file_name: String,
    pub peer: Option<String>,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub resumed_bytes: u64,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverySummary {
    pub hostname: String,
    pub address: String,
    pub token: String,
    /// True when the peer's beacon indicates it will only accept transfers
    /// carrying a valid pairing code — the send UI should prompt for one.
    #[serde(default)]
    pub pin_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkIterationSummary {
    pub iteration: usize,
    pub throughput_mib_s: f64,
    pub peak_sender_buffered_bytes: usize,
    pub peak_receiver_buffered_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSummary {
    pub size_mb: usize,
    pub iterations: Vec<BenchmarkIterationSummary>,
    pub average_throughput_mib_s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransferEvent {
    StateChanged {
        direction: TransferDirection,
        state: ConnectionState,
        peer: Option<String>,
    },
    /// Receiver only: a sender wants to deliver this file and the request is
    /// gated on user approval (`ReceiveRequest::auto_accept == false`). The
    /// engine blocks until [`SessionControl::accept`]/[`decline`] resolves it
    /// (or the approval window times out, which declines).
    ///
    /// [`decline`]: SessionControl::decline
    IncomingRequest {
        direction: TransferDirection,
        file_name: String,
        total_bytes: u64,
        peer: Option<String>,
    },
    /// Sender only: `Start` metadata was delivered and the sender is waiting
    /// for the receiver's `Accept` go-ahead (auto-accept answers immediately;
    /// a manual receiver may take a while).
    AwaitingApproval {
        direction: TransferDirection,
        file_name: String,
    },
    /// The session was cancelled locally via [`SessionControl::cancel`]. The
    /// corresponding engine call returns `AppError::Cancelled` right after.
    Cancelled {
        direction: TransferDirection,
    },
    /// The transfer was refused: by the local user (receiver declining an
    /// incoming request) or by the peer (sender seeing the receiver's decline
    /// / PIN rejection / cancellation). `reason` is a stable machine-readable
    /// code: "declined", "pin_rejected", or "cancelled".
    Declined {
        direction: TransferDirection,
        reason: String,
    },
    Started {
        direction: TransferDirection,
        file_name: String,
        total_bytes: u64,
        resumed_bytes: u64,
    },
    Resumed {
        direction: TransferDirection,
        next_sequence: u32,
        resumed_bytes: u64,
    },
    Progress {
        direction: TransferDirection,
        transferred_bytes: u64,
        total_bytes: u64,
    },
    CheckpointUpdated {
        checkpoint_path: PathBuf,
        next_sequence: u32,
        bytes_written: u64,
    },
    Completed(TransferSummary),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscoveryEvent {
    SearchStarted {
        token: Option<String>,
        timeout_secs: u64,
    },
    BroadcastStarted {
        token: String,
        port: u16,
    },
    PeerFound(DiscoverySummary),
    PeerNotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BenchmarkEvent {
    Started {
        size_mb: usize,
        iterations: usize,
        latency_ticks: u64,
    },
    IterationCompleted(BenchmarkIterationSummary),
    Completed(BenchmarkSummary),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PlenumEvent {
    Log { level: LogLevel, message: String },
    Transfer(TransferEvent),
    Discovery(DiscoveryEvent),
    Benchmark(BenchmarkEvent),
}

pub trait EventSink {
    fn emit(&mut self, event: PlenumEvent);
}

impl<F> EventSink for F
where
    F: FnMut(PlenumEvent),
{
    fn emit(&mut self, event: PlenumEvent) {
        self(event);
    }
}
