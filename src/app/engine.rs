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
    PlenumEvent, BenchmarkEvent, BenchmarkIterationSummary, BenchmarkRequest, BenchmarkSummary,
    ConnectionState, CorePermissions, DiscoverRequest, DiscoveryEvent, DiscoverySummary, EventSink,
    PermissionKind, ReceiveRequest, SendRequest, TransferDirection, TransferEvent,
};
use crate::discovery::{Beacon, PairingToken};
use crate::flow::{ReceiverWindow, SenderWindow};
use crate::protocol::{Packet, PacketType, encode_packet, parse_packet};
use crate::signaling::{RoutedSignal, SignalMessage, SignalingState};
use crate::stream::{ResumeCheckpoint, chunk_bytes};
use crate::transport::{MemoryTransport, MemoryTransportConfig, TcpTransport, Transport};

const RESUME_NEGOTIATION_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Debug, Default)]
pub struct PlenumCore {
    signaling: SignalingState,
}

impl PlenumCore {
    pub fn new() -> Self {
        Self::default()
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

        let mut start_payload = Vec::new();
        start_payload.extend_from_slice(&file_size.to_be_bytes());
        start_payload.extend_from_slice(file_name.as_bytes());
        transport.send(&encode_packet(&Packet::new(
            PacketType::Start,
            0,
            start_payload,
        ))?)?;

        let (mut sequence_no, resume_bytes) = negotiate_resume(&mut transport)?;
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

        let mut sender =
            SenderWindow::new(request.options.window_size, request.options.timeout_ticks)?;
        let mut ack_sizes = BTreeMap::<u32, usize>::new();
        let mut file_done = resume_bytes >= file_size;
        let mut buffer = vec![0u8; request.options.chunk_size];
        let mut bytes_acked = resume_bytes;

        loop {
            let now = now_ms();
            while !file_done && sender.pending_len() < request.options.window_size * 2 {
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
                let control = parse_packet(&frame)?;
                if control.packet_type == PacketType::Ack {
                    if let Some(size) = ack_sizes.remove(&control.sequence_no) {
                        bytes_acked = bytes_acked.saturating_add(size as u64);
                        sink.emit(PlenumEvent::Transfer(TransferEvent::Progress {
                            direction: TransferDirection::Send,
                            transferred_bytes: bytes_acked.min(file_size),
                            total_bytes: file_size,
                        }));
                    }
                }
                sender.handle_control_packet(&control)?;
            }

            sender.retransmit_due(&mut transport, now)?;
            sender.send_available(&mut transport, now)?;

            if file_done && sender.is_empty() {
                break;
            }

            thread::sleep(Duration::from_millis(1));
        }

        transport.send(&encode_packet(&Packet::new(
            PacketType::Finish,
            sequence_no,
            Vec::new(),
        ))?)?;
        transport.close()?;

        let summary = crate::app::types::TransferSummary {
            direction: TransferDirection::Send,
            file_name,
            peer: Some(address),
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

    pub fn receive_file<S: EventSink>(
        &mut self,
        request: ReceiveRequest,
        sink: &mut S,
    ) -> Result<crate::app::types::TransferSummary, AppError> {
        validate_receive_request(&request)?;
        create_dir_all(&request.output_dir)?;

        let listener = TcpListener::bind(format!("0.0.0.0:{}", request.port))?;
        let actual_port = listener.local_addr()?.port();
        
        sink.emit(PlenumEvent::Transfer(TransferEvent::StateChanged {
            direction: TransferDirection::Receive,
            state: ConnectionState::Listening,
            peer: Some(format!("0.0.0.0:{}", actual_port)),
        }));

        let token = PairingToken::generate();
        let broadcast_handle = if request.announce_on_lan {
            let beacon = Beacon::new();
            let handle = beacon.broadcast(&token, actual_port)?;
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

        let started_at = Instant::now();
        let tcp_transport = TcpTransport::accept(&listener)?;
        let peer = tcp_transport.peer_addr()?.to_string();
        
        let control_transport = MemoryTransport::new(MemoryTransportConfig::default());
        let mut transport = crate::transport::MultipathTransport::new(
            Box::new(tcp_transport),
            Box::new(control_transport),
        );
        
        sink.emit(PlenumEvent::Transfer(TransferEvent::StateChanged {
            direction: TransferDirection::Receive,
            state: ConnectionState::Connected,
            peer: Some(peer.clone()),
        }));

        stop_flag.store(true, Ordering::Relaxed);
        if let Some(thread) = broadcast_thread {
            let _ = thread.join();
        }

        let mut receiver = ReceiverWindow::new();
        let mut file: Option<File> = None;
        let mut file_name = String::from("received_file");
        let mut file_size = 0u64;
        let mut bytes_received = 0u64;
        let mut checkpoint: Option<ResumeCheckpoint> = None;
        let mut checkpoint_path: Option<PathBuf> = None;
        let mut peak_receiver_buffered = 0usize;

        loop {
            let frame = match transport.recv() {
                Ok(Some(frame)) => frame,
                Ok(None) => continue,
                Err(error) => {
                    if transport.is_closed() {
                        break;
                    }
                    return Err(error.into());
                }
            };

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
                    let out_path = request.output_dir.join(&clean_name);
                    let cp_path = resume_checkpoint_path(&out_path);
                    let (resume_sequence, resume_bytes, open_file, cp) = prepare_resume_state(
                        &out_path,
                        &cp_path,
                        &clean_name,
                        file_size,
                        request.options.chunk_size,
                    )?;

                    file = Some(open_file);
                    checkpoint = Some(cp);
                    checkpoint_path = Some(cp_path.clone());
                    receiver = ReceiverWindow::with_next_expected(resume_sequence);
                    bytes_received = resume_bytes;
                    file_name = clean_name;

                    sink.emit(PlenumEvent::Transfer(TransferEvent::Started {
                        direction: TransferDirection::Receive,
                        file_name: file_name.clone(),
                        total_bytes: file_size,
                        resumed_bytes: resume_bytes,
                    }));

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
                }
                PacketType::Data => {
                    let controls = receiver.receive_data_packet(packet)?;
                    for control in controls {
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
                                sink.emit(PlenumEvent::Transfer(
                                    TransferEvent::CheckpointUpdated {
                                        checkpoint_path: path.clone(),
                                        next_sequence: cp.next_sequence,
                                        bytes_written: cp.bytes_written,
                                    },
                                ));
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
                PacketType::Close => break,
                PacketType::Resume => {}
                _ => {}
            }
        }

        // Tolerant close: the sender may have already disconnected, so
        // closing an already-severed TCP stream is expected and not fatal.
        let _ = transport.close();
        let summary = crate::app::types::TransferSummary {
            direction: TransferDirection::Receive,
            file_name,
            peer: Some(peer.clone()),
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
            peer: Some(peer),
        }));
        Ok(summary)
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

fn negotiate_resume<T: Transport>(transport: &mut T) -> Result<(u32, u64), AppError> {
    let deadline = Instant::now() + RESUME_NEGOTIATION_TIMEOUT;
    while Instant::now() < deadline {
        match transport.recv()? {
            Some(frame) => {
                let packet = parse_packet(&frame)?;
                if packet.packet_type == PacketType::Resume {
                    if packet.payload.len() != 8 {
                        return Ok((packet.sequence_no, 0));
                    }
                    let mut bytes = [0u8; 8];
                    bytes.copy_from_slice(&packet.payload);
                    return Ok((packet.sequence_no, u64::from_be_bytes(bytes)));
                }
            }
            None => thread::sleep(Duration::from_millis(5)),
        }
    }

    Ok((0, 0))
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
