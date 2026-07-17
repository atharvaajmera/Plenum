//! High-level app integration engine.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions, create_dir_all};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::app::error::AppError;
use crate::app::types::{
    AcceptDecision, PlenumEvent, BenchmarkEvent, BenchmarkIterationSummary, BenchmarkRequest,
    BenchmarkSummary, ConnectionState, CorePermissions, DiscoverRequest, DiscoveryEvent,
    DiscoverySummary, EventSink, LogLevel, PermissionKind, ReceiveRemoteRequest, ReceiveRequest,
    SendRemoteRequest, SendRequest, SessionControl, TransferDirection, TransferEvent,
    TransferSummary,
};
use crate::discovery::{Beacon, PairingToken};
use crate::flow::{ReceiverWindow, SenderWindow};
use crate::protocol::{Packet, PacketType, encode_packet, parse_packet};
use crate::rtc::RtcTransport;
use crate::signaling::{RoutedSignal, SignalMessage, SignalingState};
use crate::stream::{ResumeCheckpoint, chunk_bytes};
use crate::transport::{MemoryTransport, MemoryTransportConfig, TcpTransport, Transport};

/// How long the receiver holds an incoming `Start` open waiting for the local
/// user to accept or decline it (when `auto_accept` is off) before declining.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

/// How long the sender waits for the receiver's `Accept` after `Start`.
/// Slightly longer than [`APPROVAL_TIMEOUT`] so the receiver's own timeout
/// (which sends an explicit decline) wins the race and the sender gets a
/// clear "declined" instead of a generic timeout.
const ACCEPT_WAIT_TIMEOUT: Duration = Duration::from_secs(150);

/// How long the receiver waits for the sender's `Auth` packet when a PIN is
/// required, before dropping that connection and listening again.
const AUTH_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

/// Machine-readable reasons carried in a `Close` packet's payload.
const CLOSE_REASON_DECLINED: &str = "declined";
const CLOSE_REASON_PIN_REJECTED: &str = "pin_rejected";
const CLOSE_REASON_CANCELLED: &str = "cancelled";

/// Sender watchdog: abort if packets are awaiting acknowledgement but nothing
/// has arrived from the receiver for this long. A healthy receiver ACKs every
/// data packet, so prolonged silence means the connection is dead/half-open.
const SEND_STALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Receiver watchdog: abort if no frames at all arrive for this long.
/// Longer than the sender's timeout so the sender aborts first and the
/// receiver's checkpoint stays valid for resume.
const RECEIVE_STALL_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Default)]
pub struct PlenumCore {
    signaling: SignalingState,
    control: SessionControl,
}

impl PlenumCore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The control handle for this core's blocking calls. Clone it *before*
    /// starting `send_file`/`receive_file`/... on a worker thread; the clone
    /// can then cancel the session or answer an incoming-file request from
    /// any other thread.
    pub fn control(&self) -> SessionControl {
        self.control.clone()
    }

    pub fn send_file<S: EventSink>(
        &mut self,
        request: SendRequest,
        sink: &mut S,
    ) -> Result<crate::app::types::TransferSummary, AppError> {
        validate_send_request(&request)?;
        let started_at = Instant::now();
        let mut file = File::open(&request.file_path)?;
        let file_size = file.metadata()?.len();
        let file_name = request
            .file_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let address = match request.address.clone() {
            Some(addr) => addr,
            None => {
                sink.emit(PlenumEvent::Transfer(TransferEvent::StateChanged {
                    direction: TransferDirection::Send,
                    state: ConnectionState::Discovering,
                    peer: None,
                }));
                let discovery = self.discover_peer(
                    DiscoverRequest {
                        token: request.discovery_token.clone(),
                        timeout_secs: 10,
                        permissions: request.permissions.clone(),
                    },
                    sink,
                )?;
                discovery.address
            }
        };

        sink.emit(PlenumEvent::Transfer(TransferEvent::StateChanged {
            direction: TransferDirection::Send,
            state: ConnectionState::Connecting,
            peer: Some(address.clone()),
        }));
        let tcp_transport = TcpTransport::connect(&address)?;
        
        // Dummy control path for now, until relay transport is implemented
        let control_transport = MemoryTransport::new(MemoryTransportConfig::default());
        
        let mut transport = crate::transport::MultipathTransport::new(
            Box::new(tcp_transport),
            Box::new(control_transport),
        );
        
        sink.emit(PlenumEvent::Transfer(TransferEvent::StateChanged {
            direction: TransferDirection::Send,
            state: ConnectionState::Connected,
            peer: Some(address.clone()),
        }));

        run_send_transfer(
            &mut transport,
            sink,
            &mut file,
            file_size,
            &file_name,
            &request.options,
            Some(address),
            started_at,
            &self.control,
            request.discovery_token.as_deref(),
        )
    }

    /// Sends a file over the internet via a relay/signaling server, acting as
    /// the WebRTC offerer. See `crate::rtc::RtcTransport::connect_as_offerer`.
    pub fn send_file_remote<S: EventSink>(
        &mut self,
        request: SendRemoteRequest,
        sink: &mut S,
    ) -> Result<TransferSummary, AppError> {
        validate_send_remote_request(&request)?;
        let started_at = Instant::now();
        let mut file = File::open(&request.file_path)?;
        let file_size = file.metadata()?.len();
        let file_name = request
            .file_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        sink.emit(PlenumEvent::Transfer(TransferEvent::StateChanged {
            direction: TransferDirection::Send,
            state: ConnectionState::SignalingConnected,
            peer: Some(request.session_id.clone()),
        }));

        let mut transport = RtcTransport::connect_as_offerer_cancellable(
            &request.relay_server_url,
            &request.session_id,
            &request.my_peer_id,
            request.ice_servers.clone(),
            Duration::from_secs(request.connect_timeout_secs),
            self.control.cancel_flag(),
        )
        .map_err(|error| {
            rtc_connect_error(error, TransferDirection::Send, sink)
        })?;

        sink.emit(PlenumEvent::Transfer(TransferEvent::StateChanged {
            direction: TransferDirection::Send,
            state: ConnectionState::Connected,
            peer: Some(request.session_id.clone()),
        }));

        run_send_transfer(
            &mut transport,
            sink,
            &mut file,
            file_size,
            &file_name,
            &request.options,
            Some(request.session_id.clone()),
            started_at,
            &self.control,
            // Internet transfers have no LAN PIN: the room code is the secret.
            None,
        )
    }

    pub fn receive_file<S: EventSink>(
        &mut self,
        request: ReceiveRequest,
        sink: &mut S,
    ) -> Result<crate::app::types::TransferSummary, AppError> {
        validate_receive_request(&request)?;
        create_dir_all(&request.output_dir)?;
        let control = self.control.clone();
        control.reset_decision();

        let listener = TcpListener::bind(format!("0.0.0.0:{}", request.port))?;
        // Non-blocking so the accept loop below can poll the cancel flag
        // instead of parking inside `accept()` forever.
        listener.set_nonblocking(true)?;
        let actual_port = listener.local_addr()?.port();

        sink.emit(PlenumEvent::Transfer(TransferEvent::StateChanged {
            direction: TransferDirection::Receive,
            state: ConnectionState::Listening,
            peer: Some(format!("0.0.0.0:{}", actual_port)),
        }));

        let token = PairingToken::generate();
        let broadcast_handle = if request.announce_on_lan {
            let beacon = Beacon::new();
            let handle = beacon.broadcast(
                &token,
                actual_port,
                request.device_name.clone(),
                request.require_pin,
            )?;
            sink.emit(PlenumEvent::Discovery(DiscoveryEvent::BroadcastStarted {
                token: token.code().to_string(),
                port: actual_port,
            }));
            Some(handle)
        } else {
            None
        };

        let stop_flag = Arc::new(AtomicBool::new(false));
        let broadcast_thread = if let Some(handle) = broadcast_handle {
            let flag = stop_flag.clone();
            Some(thread::spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    let _ = handle.send_once();
                    thread::sleep(handle.interval());
                }
            }))
        } else {
            None
        };

        // Ensure the broadcast thread is stopped on every exit path (cancel,
        // auth failure, transfer error), not just the happy path.
        let result =
            self.accept_and_receive(&request, &listener, &token, &control, &stop_flag, sink);

        stop_flag.store(true, Ordering::Relaxed);
        if let Some(thread) = broadcast_thread {
            let _ = thread.join();
        }

        result
    }

    /// Accept loop + auth gate + transfer for `receive_file`, split out so the
    /// caller can stop the announce thread regardless of how this exits.
    fn accept_and_receive<S: EventSink>(
        &mut self,
        request: &ReceiveRequest,
        listener: &TcpListener,
        token: &PairingToken,
        control: &SessionControl,
        announce_stop: &AtomicBool,
        sink: &mut S,
    ) -> Result<crate::app::types::TransferSummary, AppError> {
        // Outer loop: a sender that fails the PIN check is dropped and the
        // listener keeps accepting, so one bad/mistyped attempt doesn't kill
        // the whole receive session.
        loop {
            let started_at = Instant::now();
            let stream = loop {
                if control.is_cancelled() {
                    sink.emit(PlenumEvent::Transfer(TransferEvent::Cancelled {
                        direction: TransferDirection::Receive,
                    }));
                    return Err(AppError::Cancelled);
                }
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        thread::sleep(Duration::from_millis(25));
                    }
                    Err(error) => return Err(error.into()),
                }
            };
            // The transfer stream itself uses blocking reads with a short
            // read timeout (see TcpTransport), not the listener's mode.
            stream.set_nonblocking(false)?;

            let peer = stream
                .peer_addr()
                .map(|addr| addr.to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            let tcp_transport = TcpTransport::from_stream(stream)?;

            let control_transport = MemoryTransport::new(MemoryTransportConfig::default());
            let mut transport = crate::transport::MultipathTransport::new(
                Box::new(tcp_transport),
                Box::new(control_transport),
            );

            // PIN gate: require an Auth packet proving the pairing code
            // before anything else happens on this connection.
            if request.require_pin {
                match verify_sender_auth(&mut transport, token, control) {
                    Ok(()) => {}
                    Err(AuthGateOutcome::WrongPin) => {
                        sink.emit(PlenumEvent::Log {
                            level: LogLevel::Warn,
                            message: format!("rejected connection from {peer}: invalid PIN"),
                        });
                        let _ = transport.send(&encode_packet(&Packet::new(
                            PacketType::Close,
                            0,
                            CLOSE_REASON_PIN_REJECTED.as_bytes().to_vec(),
                        ))?);
                        let _ = transport.close();
                        continue;
                    }
                    Err(AuthGateOutcome::Cancelled) => {
                        sink.emit(PlenumEvent::Transfer(TransferEvent::Cancelled {
                            direction: TransferDirection::Receive,
                        }));
                        return Err(AppError::Cancelled);
                    }
                    Err(AuthGateOutcome::Error(error)) => return Err(error),
                }
            }

            sink.emit(PlenumEvent::Transfer(TransferEvent::StateChanged {
                direction: TransferDirection::Receive,
                state: ConnectionState::Connected,
                peer: Some(peer.clone()),
            }));

            // A sender is committed; stop announcing on the LAN.
            announce_stop.store(true, Ordering::Relaxed);

            return run_receive_transfer(
                &mut transport,
                sink,
                &request.output_dir,
                &request.options,
                peer,
                started_at,
                control,
                request.auto_accept,
            );
        }
    }

    /// Receives a file over the internet via a relay/signaling server, acting
    /// as the WebRTC answerer. See `crate::rtc::RtcTransport::connect_as_answerer`.
    pub fn receive_file_remote<S: EventSink>(
        &mut self,
        request: ReceiveRemoteRequest,
        sink: &mut S,
    ) -> Result<TransferSummary, AppError> {
        validate_receive_remote_request(&request)?;
        create_dir_all(&request.output_dir)?;
        let started_at = Instant::now();
        let control = self.control.clone();
        control.reset_decision();

        sink.emit(PlenumEvent::Transfer(TransferEvent::StateChanged {
            direction: TransferDirection::Receive,
            state: ConnectionState::SignalingConnected,
            peer: Some(request.session_id.clone()),
        }));

        let mut transport = RtcTransport::connect_as_answerer_cancellable(
            &request.relay_server_url,
            &request.session_id,
            &request.my_peer_id,
            request.ice_servers.clone(),
            Duration::from_secs(request.connect_timeout_secs),
            control.cancel_flag(),
        )
        .map_err(|error| {
            rtc_connect_error(error, TransferDirection::Receive, sink)
        })?;

        sink.emit(PlenumEvent::Transfer(TransferEvent::StateChanged {
            direction: TransferDirection::Receive,
            state: ConnectionState::Connected,
            peer: Some(request.session_id.clone()),
        }));

        run_receive_transfer(
            &mut transport,
            sink,
            &request.output_dir,
            &request.options,
            request.session_id.clone(),
            started_at,
            &control,
            request.auto_accept,
        )
    }

    pub fn discover_peer<S: EventSink>(
        &mut self,
        request: DiscoverRequest,
        sink: &mut S,
    ) -> Result<DiscoverySummary, AppError> {
        validate_discover_request(&request)?;
        sink.emit(PlenumEvent::Discovery(DiscoveryEvent::SearchStarted {
            token: request.token.clone(),
            timeout_secs: request.timeout_secs,
        }));

        let beacon = Beacon::with_config(crate::discovery::beacon::BeaconConfig {
            discover_timeout: Duration::from_secs(request.timeout_secs),
            ..Default::default()
        });

        let result = match request.token.as_deref() {
            Some(token) => beacon.discover_with_token(token),
            None => beacon.discover(),
        };

        match result {
            Ok(announcement) => {
                let address = announcement.tcp_addr().to_string();
                let summary = DiscoverySummary {
                    hostname: announcement.hostname,
                    address,
                    token: announcement.token,
                    pin_required: announcement.pin_required,
                };
                sink.emit(PlenumEvent::Discovery(DiscoveryEvent::PeerFound(
                    summary.clone(),
                )));
                Ok(summary)
            }
            Err(crate::discovery::DiscoveryError::NoPeersFound) => {
                sink.emit(PlenumEvent::Discovery(DiscoveryEvent::PeerNotFound));
                Err(AppError::from(
                    crate::discovery::DiscoveryError::NoPeersFound,
                ))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn benchmark<S: EventSink>(
        &mut self,
        request: BenchmarkRequest,
        sink: &mut S,
    ) -> Result<BenchmarkSummary, AppError> {
        if request.size_mb == 0 {
            return Err(AppError::InvalidRequest(
                "benchmark size must be greater than zero".into(),
            ));
        }
        if request.iterations == 0 {
            return Err(AppError::InvalidRequest(
                "benchmark iterations must be greater than zero".into(),
            ));
        }
        if request.options.chunk_size == 0 || request.options.window_size == 0 {
            return Err(AppError::InvalidRequest(
                "transfer tuning values must be greater than zero".into(),
            ));
        }

        sink.emit(PlenumEvent::Benchmark(BenchmarkEvent::Started {
            size_mb: request.size_mb,
            iterations: request.iterations,
            latency_ticks: request.latency_ticks,
        }));

        let size_bytes = request.size_mb * 1024 * 1024;
        let payload: Vec<u8> = (0..size_bytes).map(|idx| (idx % 251) as u8).collect();
        let mut total_secs = 0.0;
        let mut iterations = Vec::with_capacity(request.iterations);

        for iteration in 0..request.iterations {
            let started = Instant::now();
            let packets = chunk_bytes(&payload, request.options.chunk_size)?;
            let mut sender =
                SenderWindow::new(request.options.window_size, request.options.timeout_ticks)?;
            for packet in packets {
                sender.enqueue(packet)?;
            }
            let mut receiver = ReceiverWindow::new();
            let mut data_transport = MemoryTransport::new(MemoryTransportConfig {
                latency_ticks: request.latency_ticks,
                reorder_every: Some(3),
                ..MemoryTransportConfig::default()
            });
            let mut control_transport = MemoryTransport::new(MemoryTransportConfig {
                latency_ticks: request.latency_ticks,
                ..MemoryTransportConfig::default()
            });
            let mut restored = Vec::with_capacity(payload.len());
            let mut peak_sender_buffered_bytes = 0usize;
            let mut peak_receiver_buffered_bytes = 0usize;

            for tick in 0..200_000_u64 {
                peak_sender_buffered_bytes =
                    peak_sender_buffered_bytes.max(sender.buffered_payload_bytes());
                peak_receiver_buffered_bytes =
                    peak_receiver_buffered_bytes.max(receiver.buffered_payload_bytes());

                sender.retransmit_due(&mut data_transport, tick)?;
                sender.send_available(&mut data_transport, tick)?;

                while let Some(frame) = data_transport.recv()? {
                    let packet = parse_packet(&frame)?;
                    let controls = receiver.receive_data_packet(packet)?;
                    for (_, payload) in receiver.drain_ordered_packets() {
                        restored.extend_from_slice(&payload);
                    }
                    for control in controls {
                        control_transport.send(&encode_packet(&control)?)?;
                    }
                }

                while let Some(frame) = control_transport.recv()? {
                    let control = parse_packet(&frame)?;
                    sender.handle_control_packet(&control)?;
                }

                if sender.is_empty() && restored == payload {
                    break;
                }

                data_transport.tick();
                control_transport.tick();
            }

            let secs = started.elapsed().as_secs_f64();
            total_secs += secs;
            let iteration_summary = BenchmarkIterationSummary {
                iteration: iteration + 1,
                throughput_mib_s: if secs > 0.0 {
                    (size_bytes as f64 / (1024.0 * 1024.0)) / secs
                } else {
                    0.0
                },
                peak_sender_buffered_bytes,
                peak_receiver_buffered_bytes,
            };
            sink.emit(PlenumEvent::Benchmark(BenchmarkEvent::IterationCompleted(
                iteration_summary.clone(),
            )));
            iterations.push(iteration_summary);
        }

        let summary = BenchmarkSummary {
            size_mb: request.size_mb,
            average_throughput_mib_s: total_secs
                .is_normal()
                .then_some(
                    (size_bytes as f64 / (1024.0 * 1024.0))
                        / (total_secs / request.iterations as f64),
                )
                .unwrap_or(0.0),
            iterations,
        };
        sink.emit(PlenumEvent::Benchmark(BenchmarkEvent::Completed(
            summary.clone(),
        )));
        Ok(summary)
    }

    pub fn handle_signal(&mut self, message: SignalMessage) -> Result<Vec<RoutedSignal>, AppError> {
        Ok(self.signaling.handle(message)?)
    }

    pub fn session_of(&self, peer_id: &str) -> Option<String> {
        self.signaling.session_of(peer_id).map(str::to_string)
    }

    pub fn peers_in_session(&self, session_id: &str) -> Option<Vec<String>> {
        self.signaling.peers_in_session(session_id)
    }
}

fn validate_send_request(request: &SendRequest) -> Result<(), AppError> {
    require_permission(
        &request.permissions,
        PermissionKind::FileSystemRead,
        "send_file",
    )?;
    require_permission(
        &request.permissions,
        PermissionKind::LocalNetwork,
        "send_file",
    )?;
    validate_transfer_options(&request.options)?;
    Ok(())
}

fn validate_receive_request(request: &ReceiveRequest) -> Result<(), AppError> {
    require_permission(
        &request.permissions,
        PermissionKind::FileSystemWrite,
        "receive_file",
    )?;
    require_permission(
        &request.permissions,
        PermissionKind::LocalNetwork,
        "receive_file",
    )?;
    validate_transfer_options(&request.options)?;
    Ok(())
}

fn validate_discover_request(request: &DiscoverRequest) -> Result<(), AppError> {
    require_permission(
        &request.permissions,
        PermissionKind::LocalNetwork,
        "discover_peer",
    )?;
    if request.timeout_secs == 0 {
        return Err(AppError::InvalidRequest(
            "discover timeout must be greater than zero".into(),
        ));
    }
    Ok(())
}

fn validate_send_remote_request(request: &SendRemoteRequest) -> Result<(), AppError> {
    require_permission(
        &request.permissions,
        PermissionKind::FileSystemRead,
        "send_file_remote",
    )?;
    validate_remote_request_fields(
        &request.relay_server_url,
        &request.session_id,
        request.connect_timeout_secs,
    )?;
    validate_transfer_options(&request.options)?;
    Ok(())
}

fn validate_receive_remote_request(request: &ReceiveRemoteRequest) -> Result<(), AppError> {
    require_permission(
        &request.permissions,
        PermissionKind::FileSystemWrite,
        "receive_file_remote",
    )?;
    validate_remote_request_fields(
        &request.relay_server_url,
        &request.session_id,
        request.connect_timeout_secs,
    )?;
    validate_transfer_options(&request.options)?;
    Ok(())
}

fn validate_remote_request_fields(
    relay_server_url: &str,
    session_id: &str,
    connect_timeout_secs: u64,
) -> Result<(), AppError> {
    if relay_server_url.trim().is_empty() {
        return Err(AppError::InvalidRequest(
            "relay server url must not be empty".into(),
        ));
    }
    if session_id.trim().is_empty() {
        return Err(AppError::InvalidRequest(
            "session id must not be empty".into(),
        ));
    }
    if connect_timeout_secs == 0 {
        return Err(AppError::InvalidRequest(
            "connect timeout must be greater than zero".into(),
        ));
    }
    Ok(())
}

fn validate_transfer_options(options: &crate::app::types::TransferOptions) -> Result<(), AppError> {
    if options.chunk_size == 0 {
        return Err(AppError::InvalidRequest(
            "chunk size must be greater than zero".into(),
        ));
    }
    if options.window_size == 0 {
        return Err(AppError::InvalidRequest(
            "window size must be greater than zero".into(),
        ));
    }
    Ok(())
}

fn require_permission(
    permissions: &CorePermissions,
    kind: PermissionKind,
    operation: &'static str,
) -> Result<(), AppError> {
    let granted = match kind {
        PermissionKind::LocalNetwork => permissions.local_network,
        PermissionKind::FileSystemRead => permissions.file_system_read,
        PermissionKind::FileSystemWrite => permissions.file_system_write,
        PermissionKind::BackgroundTransfer => permissions.background_transfer,
    };

    if granted {
        Ok(())
    } else {
        Err(AppError::PermissionDenied {
            permission: kind,
            operation,
        })
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Runs the sender-side framing/window transfer loop over an already-connected
/// transport (LAN TCP or internet WebRTC), given an open source file. Shared by
/// `send_file` and `send_file_remote`.
fn run_send_transfer<T: Transport, S: EventSink>(
    transport: &mut T,
    sink: &mut S,
    file: &mut File,
    file_size: u64,
    file_name: &str,
    options: &crate::app::types::TransferOptions,
    peer_label: Option<String>,
    started_at: Instant,
    control: &SessionControl,
    pin: Option<&str>,
) -> Result<TransferSummary, AppError> {
    let file_name = file_name.to_string();

    // Prove knowledge of the pairing code first: a receiver with
    // "require PIN" enabled reads this before anything else and drops the
    // connection if it is missing or wrong. Harmless when not required.
    if let Some(pin) = pin {
        transport.send(&encode_packet(&Packet::new(
            PacketType::Auth,
            0,
            pin.trim().as_bytes().to_vec(),
        ))?)?;
    }

    let mut start_payload = Vec::new();
    start_payload.extend_from_slice(&file_size.to_be_bytes());
    start_payload.extend_from_slice(file_name.as_bytes());
    transport.send(&encode_packet(&Packet::new(
        PacketType::Start,
        0,
        start_payload,
    ))?)?;

    // The receiver replies with `Accept` once the transfer is approved
    // (instantly under auto-accept; after a user decision otherwise). Any
    // `Resume` arrives before the `Accept` on the ordered transport, so no
    // separate resume-negotiation window is needed afterwards.
    sink.emit(PlenumEvent::Transfer(TransferEvent::AwaitingApproval {
        direction: TransferDirection::Send,
        file_name: file_name.clone(),
    }));
    let (mut sequence_no, resume_bytes) =
        wait_for_accept(transport, control, sink, TransferDirection::Send)?;

    if resume_bytes > 0 {
        sink.emit(PlenumEvent::Transfer(TransferEvent::Resumed {
            direction: TransferDirection::Send,
            next_sequence: sequence_no,
            resumed_bytes: resume_bytes,
        }));
        file.seek(SeekFrom::Start(resume_bytes))?;
    }

    sink.emit(PlenumEvent::Transfer(TransferEvent::Started {
        direction: TransferDirection::Send,
        file_name: file_name.clone(),
        total_bytes: file_size,
        resumed_bytes: resume_bytes,
    }));

    let mut sender = SenderWindow::new(options.window_size, options.timeout_ticks)?;
    let mut ack_sizes = BTreeMap::<u32, usize>::new();
    let mut file_done = resume_bytes >= file_size;
    let mut buffer = vec![0u8; options.chunk_size];
    let mut bytes_acked = resume_bytes;
    let mut last_inbound = Instant::now();

    // TEMP DIAG: instrumentation to diagnose the internet-mode stall.
    let mut diag_acks_recv: u64 = 0;
    let mut diag_data_sent: u64 = 0;
    let mut diag_last = now_ms();
    sink.emit(PlenumEvent::Log {
        level: LogLevel::Info,
        message: format!(
            "DIAG send: transfer loop start, file={file_name} size={file_size} chunk={} window={}",
            options.chunk_size, options.window_size
        ),
    });

    loop {
        // Cooperative cancel: tell the receiver why we're going away (its
        // checkpoint stays on disk for a later resume), then bail out.
        if control.is_cancelled() {
            let _ = transport.send(&encode_packet(&Packet::new(
                PacketType::Close,
                sequence_no,
                CLOSE_REASON_CANCELLED.as_bytes().to_vec(),
            ))?);
            let _ = transport.close();
            sink.emit(PlenumEvent::Transfer(TransferEvent::Cancelled {
                direction: TransferDirection::Send,
            }));
            return Err(AppError::Cancelled);
        }

        let now = now_ms();
        while !file_done && sender.pending_len() < options.window_size * 2 {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                file_done = true;
                break;
            }

            let packet = Packet::new(PacketType::Data, sequence_no, buffer[..n].to_vec());
            sender.enqueue(packet)?;
            ack_sizes.insert(sequence_no, n);
            sequence_no = sequence_no.saturating_add(1);
        }

        while let Some(frame) = transport.recv()? {
            last_inbound = Instant::now();
            let ctrl_packet = parse_packet(&frame)?;
            match ctrl_packet.packet_type {
                // The receiver bailed mid-transfer (cancel/decline).
                PacketType::Close => {
                    let reason = String::from_utf8_lossy(&ctrl_packet.payload).into_owned();
                    sink.emit(PlenumEvent::Transfer(TransferEvent::Declined {
                        direction: TransferDirection::Send,
                        reason: reason.clone(),
                    }));
                    return Err(AppError::Rejected(rejection_message(&reason)));
                }
                // Late/duplicate handshake packets: not control traffic.
                PacketType::Accept | PacketType::Resume | PacketType::Auth => continue,
                _ => {}
            }
            if ctrl_packet.packet_type == PacketType::Ack {
                diag_acks_recv = diag_acks_recv.saturating_add(1);
                if let Some(size) = ack_sizes.remove(&ctrl_packet.sequence_no) {
                    bytes_acked = bytes_acked.saturating_add(size as u64);
                    sink.emit(PlenumEvent::Transfer(TransferEvent::Progress {
                        direction: TransferDirection::Send,
                        transferred_bytes: bytes_acked.min(file_size),
                        total_bytes: file_size,
                    }));
                }
            }
            sender.handle_control_packet(&ctrl_packet)?;
        }

        sender.retransmit_due(transport, now)?;
        diag_data_sent = diag_data_sent.saturating_add(sender.send_available(transport, now)? as u64);

        // TEMP DIAG: heartbeat every ~1s so we can see whether data leaves and
        // whether ACKs ever come back.
        if now.saturating_sub(diag_last) >= 1000 {
            diag_last = now;
            sink.emit(PlenumEvent::Log {
                level: LogLevel::Info,
                message: format!(
                    "DIAG send: data_sent={diag_data_sent} acks_recv={diag_acks_recv} bytes_acked={bytes_acked} in_flight={} pending={} seq_next={sequence_no}",
                    sender.in_flight_len(),
                    sender.pending_len()
                ),
            });
        }

        for diag in transport.poll_diagnostics() {
            sink.emit(PlenumEvent::Log {
                level: LogLevel::Info,
                message: diag,
            });
        }

        if file_done && sender.is_empty() {
            break;
        }

        // Watchdog: packets are awaiting ACKs but the receiver has gone
        // silent — the connection is dead or half-open. Abort instead of
        // spinning forever; the receiver's checkpoint allows a later resume.
        if !sender.is_empty() && last_inbound.elapsed() >= SEND_STALL_TIMEOUT {
            return Err(AppError::Stalled(format!(
                "no packets from receiver for {}s ({} in flight, {} pending)",
                SEND_STALL_TIMEOUT.as_secs(),
                sender.in_flight_len(),
                sender.pending_len()
            )));
        }

        thread::sleep(Duration::from_millis(1));
    }

    sink.emit(PlenumEvent::Log {
        level: LogLevel::Info,
        message: format!(
            "DIAG send: transfer loop END data_sent={diag_data_sent} acks_recv={diag_acks_recv} bytes_acked={bytes_acked}"
        ),
    });

    transport.send(&encode_packet(&Packet::new(
        PacketType::Finish,
        sequence_no,
        Vec::new(),
    ))?)?;
    transport.close()?;

    let summary = TransferSummary {
        direction: TransferDirection::Send,
        file_name,
        peer: peer_label,
        total_bytes: file_size,
        transferred_bytes: file_size,
        resumed_bytes: resume_bytes,
        elapsed_ms: started_at.elapsed().as_millis(),
    };
    sink.emit(PlenumEvent::Transfer(TransferEvent::Completed(
        summary.clone(),
    )));
    sink.emit(PlenumEvent::Transfer(TransferEvent::StateChanged {
        direction: TransferDirection::Send,
        state: ConnectionState::Closed,
        peer: summary.peer.clone(),
    }));
    Ok(summary)
}

/// Runs the receiver-side framing/window transfer loop over an already-connected
/// transport (LAN TCP or internet WebRTC), writing into `output_dir`. Shared by
/// `receive_file` and `receive_file_remote`.
fn run_receive_transfer<T: Transport, S: EventSink>(
    transport: &mut T,
    sink: &mut S,
    output_dir: &Path,
    options: &crate::app::types::TransferOptions,
    peer_label: String,
    started_at: Instant,
    control: &SessionControl,
    auto_accept: bool,
) -> Result<TransferSummary, AppError> {
    let mut receiver = ReceiverWindow::new();
    let mut file: Option<File> = None;
    let mut file_name = String::from("received_file");
    let mut file_size = 0u64;
    let mut bytes_received = 0u64;
    let mut checkpoint: Option<ResumeCheckpoint> = None;
    let mut checkpoint_path: Option<PathBuf> = None;
    let mut peak_receiver_buffered = 0usize;

    // TEMP DIAG: instrumentation to diagnose the internet-mode stall.
    let mut diag_data_recv: u64 = 0;
    let mut diag_acks_sent: u64 = 0;
    let mut diag_frames: u64 = 0;
    let mut diag_last = now_ms();
    let mut last_frame_at = Instant::now();
    sink.emit(PlenumEvent::Log {
        level: LogLevel::Info,
        message: "DIAG recv: transfer loop start, waiting for packets".to_string(),
    });

    loop {
        // Cooperative cancel: tell the sender why we're going away (our
        // checkpoint stays on disk for a later resume), then bail out.
        if control.is_cancelled() {
            if let Ok(bytes) = encode_packet(&Packet::new(
                PacketType::Close,
                receiver.next_expected(),
                CLOSE_REASON_CANCELLED.as_bytes().to_vec(),
            )) {
                let _ = transport.send(&bytes);
            }
            let _ = transport.close();
            sink.emit(PlenumEvent::Transfer(TransferEvent::Cancelled {
                direction: TransferDirection::Receive,
            }));
            return Err(AppError::Cancelled);
        }

        // TEMP DIAG: heartbeat every ~1s, even while idle, so we can see whether
        // ANY packet ever reaches the receiver.
        let diag_now = now_ms();
        if diag_now.saturating_sub(diag_last) >= 1000 {
            diag_last = diag_now;
            sink.emit(PlenumEvent::Log {
                level: LogLevel::Info,
                message: format!(
                    "DIAG recv: frames={diag_frames} data_recv={diag_data_recv} acks_sent={diag_acks_sent} bytes_recv={bytes_received} next_expected={}",
                    receiver.next_expected()
                ),
            });
        }

        for diag in transport.poll_diagnostics() {
            sink.emit(PlenumEvent::Log {
                level: LogLevel::Info,
                message: diag,
            });
        }

        let frame = match transport.recv() {
            Ok(Some(frame)) => {
                last_frame_at = Instant::now();
                frame
            }
            Ok(None) => {
                // Watchdog: nothing at all from the sender for too long —
                // connection is dead or half-open. Abort; the checkpoint on
                // disk allows a later resume.
                if last_frame_at.elapsed() >= RECEIVE_STALL_TIMEOUT {
                    return Err(AppError::Stalled(format!(
                        "no packets from sender for {}s",
                        RECEIVE_STALL_TIMEOUT.as_secs()
                    )));
                }
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(error) => {
                if transport.is_closed() {
                    break;
                }
                return Err(error.into());
            }
        };
        diag_frames = diag_frames.saturating_add(1);

        let packet = parse_packet(&frame)?;
        match packet.packet_type {
            PacketType::Start => {
                if packet.payload.len() < 8 {
                    return Err(AppError::InvalidRequest(
                        "start packet payload must contain file size".into(),
                    ));
                }

                let mut size_bytes = [0u8; 8];
                size_bytes.copy_from_slice(&packet.payload[0..8]);
                file_size = u64::from_be_bytes(size_bytes);
                file_name = String::from_utf8_lossy(&packet.payload[8..]).into_owned();
                let clean_name = Path::new(&file_name)
                    .file_name()
                    .unwrap_or_else(|| std::ffi::OsStr::new("received_file"))
                    .to_string_lossy()
                    .to_string();
                let out_path = output_dir.join(&clean_name);
                let cp_path = resume_checkpoint_path(&out_path);
                let (resume_sequence, resume_bytes, open_file, cp) = prepare_resume_state(
                    &out_path,
                    &cp_path,
                    &clean_name,
                    file_size,
                    options.chunk_size,
                )?;

                file = Some(open_file);
                checkpoint = Some(cp);
                checkpoint_path = Some(cp_path.clone());
                receiver = ReceiverWindow::with_next_expected(resume_sequence);
                bytes_received = resume_bytes;
                file_name = clean_name;

                // TEMP DIAG
                sink.emit(PlenumEvent::Log {
                    level: LogLevel::Info,
                    message: format!(
                        "DIAG recv: START received file={file_name} size={file_size} resume={resume_bytes}"
                    ),
                });

                // Accept gate: surface the offer to the UI and wait for a
                // decision before any data flows. Auto-accept (the default,
                // and the only behaviour desktop/CLI expose) skips the wait.
                sink.emit(PlenumEvent::Transfer(TransferEvent::IncomingRequest {
                    direction: TransferDirection::Receive,
                    file_name: file_name.clone(),
                    total_bytes: file_size,
                    peer: Some(peer_label.clone()),
                }));
                if !auto_accept {
                    let deadline = Instant::now() + APPROVAL_TIMEOUT;
                    loop {
                        if control.is_cancelled() {
                            let _ = transport.send(&encode_packet(&Packet::new(
                                PacketType::Close,
                                resume_sequence,
                                CLOSE_REASON_CANCELLED.as_bytes().to_vec(),
                            ))?);
                            let _ = transport.close();
                            sink.emit(PlenumEvent::Transfer(TransferEvent::Cancelled {
                                direction: TransferDirection::Receive,
                            }));
                            return Err(AppError::Cancelled);
                        }
                        match control.decision() {
                            AcceptDecision::Accepted => break,
                            AcceptDecision::Declined => {
                                let _ = transport.send(&encode_packet(&Packet::new(
                                    PacketType::Close,
                                    resume_sequence,
                                    CLOSE_REASON_DECLINED.as_bytes().to_vec(),
                                ))?);
                                let _ = transport.close();
                                sink.emit(PlenumEvent::Transfer(TransferEvent::Declined {
                                    direction: TransferDirection::Receive,
                                    reason: CLOSE_REASON_DECLINED.to_string(),
                                }));
                                return Err(AppError::Rejected(
                                    "transfer declined".to_string(),
                                ));
                            }
                            AcceptDecision::Pending => {
                                if Instant::now() >= deadline {
                                    let _ = transport.send(&encode_packet(&Packet::new(
                                        PacketType::Close,
                                        resume_sequence,
                                        CLOSE_REASON_DECLINED.as_bytes().to_vec(),
                                    ))?);
                                    let _ = transport.close();
                                    sink.emit(PlenumEvent::Transfer(TransferEvent::Declined {
                                        direction: TransferDirection::Receive,
                                        reason: CLOSE_REASON_DECLINED.to_string(),
                                    }));
                                    return Err(AppError::Rejected(format!(
                                        "no decision within {}s; transfer declined",
                                        APPROVAL_TIMEOUT.as_secs()
                                    )));
                                }
                                thread::sleep(Duration::from_millis(50));
                            }
                        }
                    }
                    control.reset_decision();
                }

                if resume_bytes > 0 {
                    sink.emit(PlenumEvent::Transfer(TransferEvent::Resumed {
                        direction: TransferDirection::Receive,
                        next_sequence: resume_sequence,
                        resumed_bytes: resume_bytes,
                    }));
                    transport.send(&encode_packet(&Packet::new(
                        PacketType::Resume,
                        resume_sequence,
                        resume_bytes.to_be_bytes().to_vec(),
                    ))?)?;
                }

                // Go-ahead: the sender blocks in `wait_for_accept` until this
                // arrives. It is sent after any `Resume` so the ordered
                // transport delivers the resume offset first.
                transport.send(&encode_packet(&Packet::new(
                    PacketType::Accept,
                    resume_sequence,
                    Vec::new(),
                ))?)?;

                sink.emit(PlenumEvent::Transfer(TransferEvent::Started {
                    direction: TransferDirection::Receive,
                    file_name: file_name.clone(),
                    total_bytes: file_size,
                    resumed_bytes: resume_bytes,
                }));

                // A human decision may have taken minutes; don't let the
                // stall watchdog count that time against the sender.
                last_frame_at = Instant::now();
            }
            PacketType::Data => {
                diag_data_recv = diag_data_recv.saturating_add(1);
                let controls = receiver.receive_data_packet(packet)?;
                for control in controls {
                    if control.packet_type == PacketType::Ack {
                        diag_acks_sent = diag_acks_sent.saturating_add(1);
                    }
                    transport.send(&encode_packet(&control)?)?;
                }

                peak_receiver_buffered =
                    peak_receiver_buffered.max(receiver.buffered_payload_bytes());
                let drained = receiver.drain_ordered_packets();
                if !drained.is_empty() {
                    for (_, payload) in drained {
                        bytes_received = bytes_received.saturating_add(payload.len() as u64);
                        if let Some(file) = file.as_mut() {
                            file.write_all(&payload)?;
                        }
                    }

                    if let Some(cp) = checkpoint.as_mut() {
                        cp.update(receiver.next_expected(), bytes_received);
                        if let Some(path) = checkpoint_path.as_ref() {
                            cp.save(path)?;
                            sink.emit(PlenumEvent::Transfer(TransferEvent::CheckpointUpdated {
                                checkpoint_path: path.clone(),
                                next_sequence: cp.next_sequence,
                                bytes_written: cp.bytes_written,
                            }));
                        }
                    }

                    sink.emit(PlenumEvent::Transfer(TransferEvent::Progress {
                        direction: TransferDirection::Receive,
                        transferred_bytes: bytes_received.min(file_size),
                        total_bytes: file_size,
                    }));
                }
            }
            PacketType::Finish => {
                if let Some(path) = checkpoint_path.as_ref() {
                    ResumeCheckpoint::clear(path)?;
                }
                break;
            }
            PacketType::Close => {
                // The sender bailed mid-transfer (cancel or error). Keep the
                // checkpoint on disk so a later attempt can resume.
                let reason = String::from_utf8_lossy(&packet.payload).into_owned();
                if reason.is_empty() {
                    break;
                }
                sink.emit(PlenumEvent::Transfer(TransferEvent::Declined {
                    direction: TransferDirection::Receive,
                    reason: reason.clone(),
                }));
                return Err(AppError::Rejected(rejection_message(&reason)));
            }
            PacketType::Resume => {}
            _ => {}
        }
    }

    // TEMP DIAG
    sink.emit(PlenumEvent::Log {
        level: LogLevel::Info,
        message: format!(
            "DIAG recv: transfer loop END frames={diag_frames} data_recv={diag_data_recv} acks_sent={diag_acks_sent} bytes_recv={bytes_received}"
        ),
    });

    // Tolerant close: the sender may have already disconnected, so
    // closing an already-severed connection is expected and not fatal.
    let _ = transport.close();
    let summary = TransferSummary {
        direction: TransferDirection::Receive,
        file_name,
        peer: Some(peer_label.clone()),
        total_bytes: file_size,
        transferred_bytes: bytes_received,
        resumed_bytes: checkpoint
            .as_ref()
            .map(|cp| cp.bytes_written)
            .unwrap_or(0)
            .min(bytes_received),
        elapsed_ms: started_at.elapsed().as_millis(),
    };
    let _ = peak_receiver_buffered;
    sink.emit(PlenumEvent::Transfer(TransferEvent::Completed(
        summary.clone(),
    )));
    sink.emit(PlenumEvent::Transfer(TransferEvent::StateChanged {
        direction: TransferDirection::Receive,
        state: ConnectionState::Closed,
        peer: Some(peer_label),
    }));
    Ok(summary)
}

/// Why the PIN gate turned a connection away (or failed outright).
enum AuthGateOutcome {
    /// No `Auth` packet, a wrong code, or garbage: drop this connection and
    /// keep listening.
    WrongPin,
    /// The local user cancelled the receive session while we waited.
    Cancelled,
    /// A transport-level failure that should abort the whole session.
    Error(AppError),
}

/// Receiver side of the PIN gate: the first packet on a connection must be an
/// `Auth` carrying the pairing code when "require PIN" is enabled.
fn verify_sender_auth<T: Transport>(
    transport: &mut T,
    token: &PairingToken,
    control: &SessionControl,
) -> Result<(), AuthGateOutcome> {
    let deadline = Instant::now() + AUTH_WAIT_TIMEOUT;
    loop {
        if control.is_cancelled() {
            return Err(AuthGateOutcome::Cancelled);
        }
        match transport.recv() {
            Ok(Some(frame)) => {
                // Anything unparsable or anything other than `Auth` first
                // (e.g. a `Start` from a sender that never prompted for the
                // code) counts as a failed proof.
                let Ok(packet) = parse_packet(&frame) else {
                    return Err(AuthGateOutcome::WrongPin);
                };
                if packet.packet_type != PacketType::Auth {
                    return Err(AuthGateOutcome::WrongPin);
                }
                let candidate = String::from_utf8_lossy(&packet.payload).into_owned();
                return if token.code().eq_ignore_ascii_case(candidate.trim()) {
                    Ok(())
                } else {
                    Err(AuthGateOutcome::WrongPin)
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    return Err(AuthGateOutcome::WrongPin);
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(AuthGateOutcome::Error(error.into())),
        }
    }
}

/// Sender side of the accept gate: blocks after `Start` until the receiver's
/// `Accept` arrives. Any `Resume` is guaranteed by the ordered transport to
/// arrive first, so this also returns the negotiated resume position as
/// `(next_sequence, resumed_bytes)`.
fn wait_for_accept<T: Transport, S: EventSink>(
    transport: &mut T,
    control: &SessionControl,
    sink: &mut S,
    direction: TransferDirection,
) -> Result<(u32, u64), AppError> {
    let deadline = Instant::now() + ACCEPT_WAIT_TIMEOUT;
    let mut resume_bytes = 0u64;
    loop {
        if control.is_cancelled() {
            let _ = transport.send(&encode_packet(&Packet::new(
                PacketType::Close,
                0,
                CLOSE_REASON_CANCELLED.as_bytes().to_vec(),
            ))?);
            let _ = transport.close();
            sink.emit(PlenumEvent::Transfer(TransferEvent::Cancelled { direction }));
            return Err(AppError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(AppError::Stalled(format!(
                "receiver did not answer the transfer offer within {}s",
                ACCEPT_WAIT_TIMEOUT.as_secs()
            )));
        }
        match transport.recv()? {
            Some(frame) => {
                let packet = parse_packet(&frame)?;
                match packet.packet_type {
                    // `Accept` carries the resume sequence (0 when starting
                    // fresh), matching any `Resume` that preceded it.
                    PacketType::Accept => return Ok((packet.sequence_no, resume_bytes)),
                    PacketType::Resume => {
                        resume_bytes = if packet.payload.len() == 8 {
                            let mut bytes = [0u8; 8];
                            bytes.copy_from_slice(&packet.payload);
                            u64::from_be_bytes(bytes)
                        } else {
                            0
                        };
                    }
                    // The receiver declined, rejected the PIN, or cancelled.
                    PacketType::Close => {
                        let reason = String::from_utf8_lossy(&packet.payload).into_owned();
                        sink.emit(PlenumEvent::Transfer(TransferEvent::Declined {
                            direction,
                            reason: reason.clone(),
                        }));
                        return Err(AppError::Rejected(rejection_message(&reason)));
                    }
                    _ => {}
                }
            }
            None => thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// Turns a machine-readable `Close` reason into a human-readable message.
fn rejection_message(reason: &str) -> String {
    match reason {
        CLOSE_REASON_DECLINED => "the receiver declined the transfer".to_string(),
        CLOSE_REASON_PIN_REJECTED => "the receiver rejected the pairing code".to_string(),
        CLOSE_REASON_CANCELLED => "the peer cancelled the transfer".to_string(),
        other => format!("the peer closed the connection ({other})"),
    }
}

/// Maps an RTC connect failure to an app error, emitting the `Cancelled`
/// event when the failure was a local cancel rather than a network problem.
fn rtc_connect_error<S: EventSink>(
    error: crate::rtc::RtcError,
    direction: TransferDirection,
    sink: &mut S,
) -> AppError {
    if matches!(error, crate::rtc::RtcError::Cancelled) {
        sink.emit(PlenumEvent::Transfer(TransferEvent::Cancelled { direction }));
        AppError::Cancelled
    } else {
        AppError::Rtc(error)
    }
}

fn resume_checkpoint_path(out_path: &Path) -> PathBuf {
    let file_name = out_path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("received_file"))
        .to_string_lossy();
    out_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{}.plenum.resume.json", file_name))
}

fn prepare_resume_state(
    out_path: &Path,
    checkpoint_path: &Path,
    file_name: &str,
    file_size: u64,
    chunk_size: usize,
) -> Result<(u32, u64, File, ResumeCheckpoint), AppError> {
    if checkpoint_path.exists() {
        let checkpoint = ResumeCheckpoint::load(checkpoint_path)?;
        if checkpoint.matches(file_name, file_size, chunk_size) {
            let mut file = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(false)
                .open(out_path)?;
            file.seek(SeekFrom::Start(checkpoint.bytes_written))?;
            return Ok((
                checkpoint.next_sequence,
                checkpoint.bytes_written,
                file,
                checkpoint,
            ));
        }

        ResumeCheckpoint::clear(checkpoint_path)?;
    }

    let file = File::create(out_path)?;
    let checkpoint = ResumeCheckpoint::new(file_name.to_string(), file_size, chunk_size);
    checkpoint.save(checkpoint_path)?;
    Ok((0, 0, file, checkpoint))
}
