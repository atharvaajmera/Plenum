//! Stable app-facing request, result, and event types.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
            timeout_ticks: 1000,
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
    pub permissions: CorePermissions,
    pub options: TransferOptions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverRequest {
    pub token: Option<String>,
    pub timeout_secs: u64,
    pub permissions: CorePermissions,
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
