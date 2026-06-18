use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};

use aether::discovery::{Beacon, PairingToken};
use aether::flow::{ReceiverWindow, SenderWindow};
use aether::protocol::{Packet, PacketType, encode_packet, parse_packet};
use aether::stream::{ResumeCheckpoint, chunk_bytes};
use aether::transport::{MemoryTransport, MemoryTransportConfig, TcpTransport, Transport};

const CHUNK_SIZE: usize = 32 * 1024;
const WINDOW_SIZE: usize = 128;
const TIMEOUT_TICKS: u64 = 1000;
const RESUME_NEGOTIATION_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Parser)]
#[command(author, version, about = "Aether peer-to-peer file transfer engine", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Send a file to a peer
    Send {
        /// Path to the file to send
        file_path: PathBuf,
        /// Address of the peer (e.g., 127.0.0.1:8080). If omitted, uses local discovery.
        address: Option<String>,
        /// Pairing token to match a specific receiver during discovery.
        #[arg(short, long)]
        token: Option<String>,
    },
    /// Receive a file from a peer
    Receive {
        /// Port to listen on
        port: u16,
        /// Directory to save the received file
        #[arg(short, long, default_value = ".")]
        output_dir: PathBuf,
        /// Disable local network discovery broadcast
        #[arg(long, default_value_t = false)]
        no_discover: bool,
    },
    /// Discover peers on the local network
    Discover {
        /// Pairing token to filter announcements
        #[arg(short, long)]
        token: Option<String>,
        /// How long to listen for announcements (in seconds)
        #[arg(short = 's', long, default_value_t = 10)]
        timeout: u64,
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

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Send {
            file_path,
            address,
            token,
        } => {
            let resolved_address = match address {
                Some(addr) => addr,
                None => {
                    println!("No address specified. Discovering peers on local network...");
                    let beacon = Beacon::new();
                    let announcement = if let Some(ref code) = token {
                        println!("Looking for peer with token: {}", code);
                        beacon.discover_with_token(code)
                    } else {
                        beacon.discover()
                    };
                    match announcement {
                        Ok(a) => {
                            let addr = a.tcp_addr().to_string();
                            println!(
                                "Found peer '{}' at {} (token: {})",
                                a.hostname, addr, a.token
                            );
                            addr
                        }
                        Err(aether::discovery::DiscoveryError::NoPeersFound) => {
                            eprintln!("No peers found on the local network.");
                            eprintln!(
                                "Make sure a receiver is running:  cargo run -- receive <port>"
                            );
                            std::process::exit(1);
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
            };
            run_send(file_path, resolved_address)
        }
        Commands::Receive {
            port,
            output_dir,
            no_discover,
        } => run_receive(port, output_dir, no_discover),
        Commands::Discover { token, timeout } => run_discover(token, timeout),
        Commands::Benchmark {
            size_mb,
            iterations,
            latency_ticks,
        } => run_benchmark(size_mb, iterations, latency_ticks),
    }
}

fn run_send(file_path: PathBuf, address: String) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::open(&file_path)?;
    let file_size = file.metadata()?.len();
    let file_name = file_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    println!("Connecting to {}...", address);
    let mut transport = TcpTransport::connect(address)?;
    println!("Connected. Sending '{}' ({} bytes)", file_name, file_size);

    let mut start_payload = Vec::new();
    start_payload.extend_from_slice(&file_size.to_be_bytes());
    start_payload.extend_from_slice(file_name.as_bytes());
    let start_packet = Packet::new(PacketType::Start, 0, start_payload);
    transport.send(&encode_packet(&start_packet)?)?;

    let (mut sequence_no, resume_bytes) = negotiate_resume(&mut transport)?;
    if resume_bytes > 0 {
        println!(
            "Receiver requested resume from sequence {} ({} bytes already present)",
            sequence_no, resume_bytes
        );
        file.seek(SeekFrom::Start(resume_bytes))?;
    }

    let mut sender = SenderWindow::new(WINDOW_SIZE, TIMEOUT_TICKS)?;
    let mut ack_sizes = BTreeMap::<u32, usize>::new();

    let pb = ProgressBar::new(file_size);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
        )
        .unwrap()
        .progress_chars("#>-"),
    );
    pb.set_position(resume_bytes.min(file_size));

    let mut file_done = resume_bytes >= file_size;
    let mut buffer = vec![0u8; CHUNK_SIZE];
    let mut bytes_acked = resume_bytes;

    loop {
        let now = now_ms();

        while !file_done && sender.pending_len() < WINDOW_SIZE * 2 {
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
                    pb.set_position(bytes_acked.min(file_size));
                }
            }
            sender.handle_control_packet(&control)?;
        }

        sender.retransmit_due(&mut transport, now)?;
        sender.send_available(&mut transport, now)?;

        if file_done && sender.is_empty() {
            break;
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    pb.set_position(file_size);
    pb.finish_with_message("Transfer complete");

    let finish_packet = Packet::new(PacketType::Finish, sequence_no, vec![]);
    transport.send(&encode_packet(&finish_packet)?)?;
    transport.close()?;

    Ok(())
}

fn run_receive(
    port: u16,
    output_dir: PathBuf,
    no_discover: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port))?;
    println!("Listening on port {}...", port);

    let token = PairingToken::generate();
    let broadcast_handle = if !no_discover {
        let beacon = Beacon::new();
        let handle = beacon.broadcast(&token, port)?;
        println!(
            "Broadcasting on local network with pairing token: {}",
            token.code()
        );
        println!(
            "Sender can connect using:  cargo run -- send <file> --token {}",
            token.code()
        );
        Some(handle)
    } else {
        None
    };

    let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let broadcast_thread = if let Some(handle) = broadcast_handle {
        let flag = stop_flag.clone();
        Some(std::thread::spawn(move || {
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = handle.send_once();
                std::thread::sleep(handle.interval());
            }
        }))
    } else {
        None
    };

    let mut transport = TcpTransport::accept(&listener)?;
    println!("Connection established.");

    stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(thread) = broadcast_thread {
        let _ = thread.join();
    }

    let mut receiver = ReceiverWindow::new();
    let mut file: Option<File> = None;
    let mut pb: Option<ProgressBar> = None;
    let mut file_size = 0u64;
    let mut bytes_received = 0u64;
    let mut checkpoint: Option<ResumeCheckpoint> = None;
    let mut checkpoint_path: Option<PathBuf> = None;

    loop {
        let frame = match transport.recv() {
            Ok(Some(f)) => f,
            Ok(None) => continue,
            Err(e) => {
                if transport.is_closed() {
                    println!(
                        "Connection closed before transfer completion; checkpoint retained if present."
                    );
                    break;
                }
                return Err(e.into());
            }
        };

        let packet = parse_packet(&frame)?;

        match packet.packet_type {
            PacketType::Start => {
                if packet.payload.len() < 8 {
                    eprintln!("Invalid Start packet.");
                    return Ok(());
                }

                let mut size_bytes = [0u8; 8];
                size_bytes.copy_from_slice(&packet.payload[0..8]);
                file_size = u64::from_be_bytes(size_bytes);

                let file_name = String::from_utf8_lossy(&packet.payload[8..]).into_owned();
                let clean_name = Path::new(&file_name)
                    .file_name()
                    .unwrap_or_else(|| std::ffi::OsStr::new("received_file"))
                    .to_string_lossy()
                    .to_string();
                let out_path = output_dir.join(&clean_name);
                let cp_path = resume_checkpoint_path(&out_path);

                let (resume_sequence, resume_bytes, open_file, cp) =
                    prepare_resume_state(&out_path, &cp_path, &clean_name, file_size)?;

                file = Some(open_file);
                checkpoint = Some(cp);
                checkpoint_path = Some(cp_path);
                receiver = ReceiverWindow::with_next_expected(resume_sequence);
                bytes_received = resume_bytes;

                println!("Receiving '{}' ({} bytes)", out_path.display(), file_size);
                if resume_bytes > 0 {
                    println!(
                        "Resuming existing transfer from sequence {} ({} bytes)",
                        resume_sequence, resume_bytes
                    );
                    let resume_packet = Packet::new(
                        PacketType::Resume,
                        resume_sequence,
                        resume_bytes.to_be_bytes().to_vec(),
                    );
                    transport.send(&encode_packet(&resume_packet)?)?;
                }

                let progress = ProgressBar::new(file_size);
                progress.set_style(
                    ProgressStyle::with_template(
                        "[{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
                    )
                    .unwrap()
                    .progress_chars("#>-"),
                );
                progress.set_position(bytes_received.min(file_size));
                pb = Some(progress);
            }
            PacketType::Data => {
                if file.is_none() {
                    eprintln!("Received Data packet before Start packet!");
                    continue;
                }

                let controls = receiver.receive_data_packet(packet)?;
                for control in controls {
                    transport.send(&encode_packet(&control)?)?;
                }

                let drained = receiver.drain_ordered_packets();
                if !drained.is_empty() {
                    for (_, payload) in drained {
                        bytes_received = bytes_received.saturating_add(payload.len() as u64);
                        if let Some(f) = file.as_mut() {
                            f.write_all(&payload)?;
                        }
                    }

                    if let Some(cp) = checkpoint.as_mut() {
                        cp.update(receiver.next_expected(), bytes_received);
                        if let Some(path) = checkpoint_path.as_ref() {
                            cp.save(path)?;
                        }
                    }

                    if let Some(p) = pb.as_ref() {
                        p.set_position(bytes_received.min(file_size));
                    }
                }
            }
            PacketType::Finish => {
                if let Some(p) = pb.as_ref() {
                    p.set_position(file_size);
                    p.finish_with_message("Transfer complete");
                }
                if let Some(path) = checkpoint_path.as_ref() {
                    ResumeCheckpoint::clear(path)?;
                }
                break;
            }
            PacketType::Close => {
                println!("Connection closed by peer.");
                break;
            }
            PacketType::Resume => {
                // Receiver ignores resume control packets.
            }
            _ => {}
        }
    }

    transport.close()?;
    Ok(())
}

fn run_discover(token: Option<String>, timeout: u64) -> Result<(), Box<dyn std::error::Error>> {
    use aether::discovery::beacon::BeaconConfig;

    let config = BeaconConfig {
        discover_timeout: Duration::from_secs(timeout),
        ..BeaconConfig::default()
    };
    let beacon = Beacon::with_config(config);

    println!("Scanning local network for Aether peers ({timeout}s timeout)...");

    let result = if let Some(ref code) = token {
        println!("Filtering for token: {}", code);
        beacon.discover_with_token(code)
    } else {
        beacon.discover()
    };

    match result {
        Ok(announcement) => {
            println!("\nFound peer:");
            println!("  Hostname:  {}", announcement.hostname);
            println!("  Address:   {}", announcement.tcp_addr());
            println!("  Token:     {}", announcement.token);
            println!(
                "\nTo send a file:  cargo run -- send <file> {}",
                announcement.tcp_addr()
            );
        }
        Err(aether::discovery::DiscoveryError::NoPeersFound) => {
            println!("No peers found on the local network.");
        }
        Err(e) => return Err(e.into()),
    }

    Ok(())
}

fn run_benchmark(
    size_mb: usize,
    iterations: usize,
    latency_ticks: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let size_bytes = size_mb * 1024 * 1024;
    let payload: Vec<u8> = (0..size_bytes).map(|idx| (idx % 251) as u8).collect();

    println!(
        "Benchmarking {} MiB transfer for {} iteration(s) with latency_ticks={}...",
        size_mb, iterations, latency_ticks
    );

    let mut total_secs = 0.0;
    for iteration in 0..iterations {
        let started = Instant::now();
        let packets = chunk_bytes(&payload, CHUNK_SIZE)?;
        let mut sender = SenderWindow::new(WINDOW_SIZE, TIMEOUT_TICKS)?;
        for packet in packets {
            sender.enqueue(packet)?;
        }
        let mut receiver = ReceiverWindow::new();
        let mut data_transport = MemoryTransport::new(MemoryTransportConfig {
            latency_ticks,
            reorder_every: Some(3),
            ..MemoryTransportConfig::default()
        });
        let mut control_transport = MemoryTransport::new(MemoryTransportConfig {
            latency_ticks,
            ..MemoryTransportConfig::default()
        });
        let mut restored = Vec::with_capacity(payload.len());
        let mut peak_sender_bytes = 0usize;
        let mut peak_receiver_bytes = 0usize;

        for tick in 0..200_000_u64 {
            peak_sender_bytes = peak_sender_bytes.max(sender.buffered_payload_bytes());
            peak_receiver_bytes = peak_receiver_bytes.max(receiver.buffered_payload_bytes());

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

        let elapsed = started.elapsed();
        let secs = elapsed.as_secs_f64();
        total_secs += secs;
        let throughput_mib = if secs > 0.0 {
            (size_bytes as f64 / (1024.0 * 1024.0)) / secs
        } else {
            0.0
        };

        println!(
            "Iteration {}: {:.2} MiB/s, peak sender buffer {} KiB, peak receiver buffer {} KiB",
            iteration + 1,
            throughput_mib,
            peak_sender_bytes / 1024,
            peak_receiver_bytes / 1024,
        );
    }

    let avg_secs = total_secs / iterations as f64;
    let avg_throughput_mib = if avg_secs > 0.0 {
        (size_bytes as f64 / (1024.0 * 1024.0)) / avg_secs
    } else {
        0.0
    };
    println!("Average throughput: {:.2} MiB/s", avg_throughput_mib);

    Ok(())
}

fn negotiate_resume(
    transport: &mut TcpTransport,
) -> Result<(u32, u64), Box<dyn std::error::Error>> {
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
            None => std::thread::sleep(Duration::from_millis(5)),
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
        .join(format!("{}.aether.resume.json", file_name))
}

fn prepare_resume_state(
    out_path: &Path,
    checkpoint_path: &Path,
    file_name: &str,
    file_size: u64,
) -> Result<(u32, u64, File, ResumeCheckpoint), Box<dyn std::error::Error>> {
    if checkpoint_path.exists() {
        let checkpoint = ResumeCheckpoint::load(checkpoint_path)?;
        if checkpoint.matches(file_name, file_size, CHUNK_SIZE) {
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
    let checkpoint = ResumeCheckpoint::new(file_name.to_string(), file_size, CHUNK_SIZE);
    checkpoint.save(checkpoint_path)?;
    Ok((0, 0, file, checkpoint))
}
