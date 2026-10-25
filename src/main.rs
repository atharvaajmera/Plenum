use std::path::PathBuf;

use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use plenum::app::{
    BenchmarkEvent, BenchmarkRequest, ConnectionState, CorePermissions, LogLevel, PlenumCore,
    PlenumEvent, ReceiveUnifiedRequest, SendUnifiedRequest, TransferEvent, TransferOptions,
    UnifiedSharePayload, generate_peer_id, generate_room_code, get_local_ip_addresses,
};

#[derive(Parser)]
#[command(author, version, about = "Plenum peer-to-peer file transfer engine", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Send a file or folder using unified transfer (LAN direct or WebRTC relay)
    Send {
        /// Path to the file or directory to send
        file_path: PathBuf,
        /// 9-character room code, connection URI (plenum://v1/connect?...), or direct IP:port
        target: String,
        /// Relay server URL
        #[arg(long, default_value = "wss://relay.plenumonline.me/ws")]
        relay_server: String,
        /// Fallback IP(s) for the relay if DNS resolution fails (comma-separated or multiple flags)
        #[arg(long, env = "PLENUM_RELAY_FALLBACK_IPS", value_delimiter = ',')]
        relay_fallback_ips: Option<Vec<String>>,
    },
    /// Receive a file using unified transfer
    Receive {
        /// Port to listen on (0 for random available port)
        #[arg(short, long, default_value_t = 0)]
        port: u16,
        /// Directory to save the received file
        #[arg(short, long, default_value = ".")]
        output_dir: PathBuf,
        /// Require PIN for pairing
        #[arg(long, default_value_t = false)]
        require_pin: bool,
        /// Relay server URL
        #[arg(long, default_value = "wss://relay.plenumonline.me/ws")]
        relay_server: String,
        /// Fallback IP(s) for the relay if DNS resolution fails (comma-separated or multiple flags)
        #[arg(long, env = "PLENUM_RELAY_FALLBACK_IPS", value_delimiter = ',')]
        relay_fallback_ips: Option<Vec<String>>,
    },
    /// Benchmark the transfer engine without disk or network setup
    Benchmark {
        /// Payload size in MiB
        #[arg(long, default_value_t = 64)]
        size_mb: usize,
        /// Number of benchmark iterations
        #[arg(long, default_value_t = 3)]
        iterations: usize,
        /// Simulated one-way latency in transport ticks
        #[arg(long, default_value_t = 1)]
        latency_ticks: u64,
    },
}

struct CliEventSink {
    progress: Option<ProgressBar>,
}

impl CliEventSink {
    fn new() -> Self {
        Self { progress: None }
    }

    fn println(&self, message: impl AsRef<str>) {
        if let Some(pb) = &self.progress {
            pb.println(message.as_ref());
        } else {
            println!("{}", message.as_ref());
        }
    }
}

impl plenum::app::EventSink for CliEventSink {
    fn emit(&mut self, event: PlenumEvent) {
        match event {
            PlenumEvent::Log { level, message } => match level {
                LogLevel::Error => eprintln!("{message}"),
                _ => self.println(message),
            },
            PlenumEvent::Transfer(event) => match event {
                TransferEvent::Listening { port, pin, public_key } => {
                    self.println(format!("Local listener active on port {port}"));
                    if let Some(pin) = pin {
                        self.println(format!("Pairing PIN: {pin}"));
                    }
                    if let Some(pk) = public_key {
                        self.println(format!("Identity Public Key: {pk}"));
                    }
                }
                TransferEvent::VerificationCode(code) => {
                    self.println(format!("Security Verification Code: {code}"));
                }
                TransferEvent::StateChanged { state, peer, .. } => match state {
                    ConnectionState::Discovering => self.println("Discovering peer..."),
                    ConnectionState::Listening => {
                        if let Some(peer) = peer {
                            self.println(format!("Listening on {peer}..."));
                        }
                    }
                    ConnectionState::Connecting => {
                        if let Some(peer) = peer {
                            self.println(format!("Connecting to {peer}..."));
                        }
                    }
                    ConnectionState::SignalingConnected => {
                        if let Some(peer) = peer {
                            self.println(format!(
                                "Connected to relay server, joining room {peer}..."
                            ));
                        }
                    }
                    ConnectionState::NegotiatingIce => {
                        self.println("Negotiating WebRTC connection...");
                    }
                    ConnectionState::Connected => {
                        if let Some(peer) = peer {
                            self.println(format!("Connected to {peer}."));
                        }
                    }
                    ConnectionState::Closed => {
                        if let Some(pb) = self.progress.take() {
                            pb.finish_and_clear();
                        }
                    }
                },
                TransferEvent::PathSelected { description, .. } => {
                    self.println(format!("Path selected: {description}"));
                }
                TransferEvent::Started {
                    direction,
                    file_name,
                    total_bytes,
                    resumed_bytes,
                } => {
                    self.println(format!(
                        "{:?} '{}' ({} bytes)",
                        direction, file_name, total_bytes
                    ));
                    let pb = ProgressBar::new(total_bytes);
                    pb.set_style(
                        ProgressStyle::with_template(
                            "[{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
                        )
                        .unwrap()
                        .progress_chars("#>-"),
                    );
                    pb.set_position(resumed_bytes.min(total_bytes));
                    self.progress = Some(pb);
                }
                TransferEvent::Resumed {
                    next_sequence,
                    resumed_bytes,
                    ..
                } => {
                    self.println(format!(
                        "Resuming transfer from sequence {} ({} bytes)",
                        next_sequence, resumed_bytes
                    ));
                }
                TransferEvent::Progress {
                    transferred_bytes,
                    total_bytes,
                    ..
                } => {
                    if let Some(pb) = &self.progress {
                        pb.set_position(transferred_bytes.min(total_bytes));
                    }
                }
                TransferEvent::CheckpointUpdated {
                    checkpoint_path,
                    next_sequence,
                    bytes_written,
                } => {
                    self.println(format!(
                        "Checkpoint saved: {} (next seq {}, {} bytes)",
                        checkpoint_path.display(),
                        next_sequence,
                        bytes_written
                    ));
                }
                TransferEvent::Completed(summary) => {
                    if let Some(pb) = self.progress.take() {
                        pb.set_position(summary.total_bytes);
                        pb.finish_with_message("Transfer complete");
                    } else {
                        self.println("Transfer complete");
                    }
                    self.println(format!(
                        "Completed {:?} of '{}' in {} ms",
                        summary.direction, summary.file_name, summary.elapsed_ms
                    ));
                }
                TransferEvent::IncomingRequest {
                    file_name,
                    total_bytes,
                    peer,
                    ..
                } => {
                    self.println(format!(
                        "Incoming transfer '{}' ({} bytes) from {}",
                        file_name,
                        total_bytes,
                        peer.unwrap_or_else(|| "unknown peer".to_string())
                    ));
                }
                TransferEvent::AwaitingApproval { .. } => {
                    self.println("Waiting for the receiver to accept the transfer...");
                }
                TransferEvent::ConnectionEstablished { mode, .. } => {
                    self.println(format!("Connection established ({mode:?})"));
                }
                TransferEvent::Cancelled { .. } => {
                    if let Some(pb) = self.progress.take() {
                        pb.finish_and_clear();
                    }
                    self.println("Transfer cancelled.");
                }
                TransferEvent::Declined { reason, .. } => {
                    if let Some(pb) = self.progress.take() {
                        pb.finish_and_clear();
                    }
                    self.println(format!("Transfer refused by peer ({reason})."));
                }
                TransferEvent::Packaging {
                    current_item,
                    total_items,
                    current_name,
                    bytes_packaged,
                } => {
                    self.println(format!(
                        "Archiving ({current_item}/{total_items}): {current_name} ({bytes_packaged} bytes)"
                    ));
                }
                TransferEvent::Failed { message, .. } => {
                    if let Some(pb) = self.progress.take() {
                        pb.finish_and_clear();
                    }
                    self.println(message);
                }
            },
            PlenumEvent::Benchmark(event) => match event {
                BenchmarkEvent::Started {
                    size_mb,
                    iterations,
                    latency_ticks,
                } => {
                    println!(
                        "Benchmarking {} MiB transfer for {} iteration(s) with latency_ticks={}...",
                        size_mb, iterations, latency_ticks
                    );
                }
                BenchmarkEvent::IterationCompleted(iteration) => {
                    println!(
                        "Iteration {}: {:.2} MiB/s, peak sender buffer {} KiB, peak receiver buffer {} KiB",
                        iteration.iteration,
                        iteration.throughput_mib_s,
                        iteration.peak_sender_buffered_bytes / 1024,
                        iteration.peak_receiver_buffered_bytes / 1024,
                    );
                }
                BenchmarkEvent::Completed(summary) => {
                    println!(
                        "Average throughput: {:.2} MiB/s",
                        summary.average_throughput_mib_s
                    );
                }
            },
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    plenum::rtc::resolve::spawn_remote_config_fetch();
    let cli = Cli::parse();
    let mut core = PlenumCore::new();
    let mut sink = CliEventSink::new();
    let permissions = CorePermissions::desktop_defaults();
    let options = TransferOptions::default();

    match cli.command {
        Commands::Send {
            file_path,
            target,
            relay_server,
            relay_fallback_ips,
        } => {
            if let Some(ips) = relay_fallback_ips {
                plenum::rtc::resolve::set_custom_fallback_ips(ips);
            }
            let payload = if let Ok(parsed) = UnifiedSharePayload::parse_uri(&target) {
                parsed
            } else if target.contains(':') {
                let parts: Vec<&str> = target.split(':').collect();
                let ip = parts[0].to_string();
                let port = parts.get(1).and_then(|p| p.parse::<u16>().ok()).unwrap_or(0);
                UnifiedSharePayload {
                    version: 1,
                    room_code: generate_room_code(),
                    lan_ips: vec![ip],
                    port,
                    pin: None,
                    peer_id: None,
                    device_name: None,
                    public_key: None,
                }
            } else {
                UnifiedSharePayload::new(&target)
            };

            let my_peer_id = generate_peer_id();
            core.send_file_unified(
                SendUnifiedRequest {
                    file_path: file_path.clone(),
                    file_paths: vec![file_path],
                    payload,
                    relay_server_url: relay_server,
                    my_peer_id,
                    ice_servers: vec![plenum::signaling::IceServer {
                        urls: vec!["stun:stun.l.google.com:19302".into()],
                        username: None,
                        credential: None,
                    }],
                    connect_timeout_secs: 30,
                    device_name: Some("Plenum CLI".into()),
                    permissions,
                    options,
                },
                &mut sink,
            )?;
        }
        Commands::Receive {
            port,
            output_dir,
            require_pin,
            relay_server,
            relay_fallback_ips,
        } => {
            if let Some(ips) = relay_fallback_ips {
                plenum::rtc::resolve::set_custom_fallback_ips(ips);
            }
            let room_code = generate_room_code();
            let my_peer_id = generate_peer_id();
            let local_ips = get_local_ip_addresses();

            println!("Room code: {room_code}");
            let connect_payload = UnifiedSharePayload {
                version: 1,
                room_code: room_code.clone(),
                lan_ips: local_ips,
                port,
                pin: None,
                peer_id: Some(my_peer_id.clone()),
                device_name: Some("Plenum CLI".into()),
                public_key: None,
            };
            println!("Connection URI: {}", connect_payload.to_uri());

            core.receive_file_unified(
                ReceiveUnifiedRequest {
                    output_dir,
                    relay_server_url: relay_server,
                    session_id: room_code,
                    my_peer_id,
                    ice_servers: vec![plenum::signaling::IceServer {
                        urls: vec!["stun:stun.l.google.com:19302".into()],
                        username: None,
                        credential: None,
                    }],
                    connect_timeout_secs: 30,
                    port,
                    require_pin,
                    auto_accept: true,
                    device_name: Some("Plenum CLI".into()),
                    permissions,
                    options,
                },
                &mut sink,
            )?;
        }
        Commands::Benchmark {
            size_mb,
            iterations,
            latency_ticks,
        } => {
            core.benchmark(
                BenchmarkRequest {
                    size_mb,
                    iterations,
                    latency_ticks,
                    options,
                },
                &mut sink,
            )?;
        }
    }

    Ok(())
}
