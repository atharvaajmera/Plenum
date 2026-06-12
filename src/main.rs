use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};

use aether::discovery::{Beacon, PairingToken};
use aether::flow::{ReceiverWindow, SenderWindow};
use aether::protocol::{Packet, PacketType, encode_packet, parse_packet};
use aether::transport::{TcpTransport, Transport};

const CHUNK_SIZE: usize = 32 * 1024;
const WINDOW_SIZE: usize = 128;
const TIMEOUT_TICKS: u64 = 1000;

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
                            eprintln!("Make sure a receiver is running:  cargo run -- receive <port>");
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

    // Send Start packet: 8 bytes size + filename string
    let mut start_payload = Vec::new();
    start_payload.extend_from_slice(&file_size.to_be_bytes());
    start_payload.extend_from_slice(file_name.as_bytes());
    let start_packet = Packet::new(PacketType::Start, 0, start_payload);
    transport.send(&encode_packet(&start_packet)?)?;

    let mut sender = SenderWindow::new(WINDOW_SIZE, TIMEOUT_TICKS)?;

    let pb = ProgressBar::new(file_size);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
        )
        .unwrap()
        .progress_chars("#>-"),
    );

    let mut sequence_no = 0u32;
    let mut file_done = false;
    let mut buffer = vec![0u8; CHUNK_SIZE];
    let mut bytes_acked = 0u64;

    loop {
        let now = now_ms();

        // Feed packets into the sender window until pending queue is large enough, or EOF
        while !file_done && sender.pending_len() < WINDOW_SIZE * 2 {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                file_done = true;
                break;
            }
            let packet = Packet::new(PacketType::Data, sequence_no, buffer[..n].to_vec());
            sender.enqueue(packet)?;
            sequence_no += 1;
        }

        // Process incoming ACKs/NACKs
        while let Some(frame) = transport.recv()? {
            let control = parse_packet(&frame)?;
            if control.packet_type == PacketType::Ack {
                bytes_acked += CHUNK_SIZE as u64; // Approximation for progress bar
                pb.set_position(bytes_acked.min(file_size));
            }
            sender.handle_control_packet(&control)?;
        }

        // Retransmit if needed
        sender.retransmit_due(&mut transport, now)?;

        // Send new packets
        sender.send_available(&mut transport, now)?;

        if file_done && sender.is_empty() {
            break;
        }

        // Small sleep to prevent busy-looping if transport recv would block
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    pb.set_position(file_size);
    pb.finish_with_message("Transfer complete");

    // Send Finish packet
    let finish_packet = Packet::new(PacketType::Finish, sequence_no, vec![]);
    transport.send(&encode_packet(&finish_packet)?)?;

    // Close transport
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

    // Start broadcasting discovery announcement unless disabled
    let token = PairingToken::generate();
    let broadcast_handle = if !no_discover {
        let beacon = Beacon::new();
        let handle = beacon.broadcast(&token, port)?;
        println!("Broadcasting on local network with pairing token: {}", token.code());
        println!("Sender can connect using:  cargo run -- send <file> --token {}", token.code());
        Some(handle)
    } else {
        None
    };

    // Broadcast in a background thread while waiting for a connection
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

    // Stop broadcasting once a connection is accepted
    stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(thread) = broadcast_thread {
        let _ = thread.join();
    }

    let mut receiver = ReceiverWindow::new();
    let mut file: Option<File> = None;
    let mut pb: Option<ProgressBar> = None;
    let mut file_size = 0u64;
    let mut bytes_received = 0u64;

    loop {
        let frame = match transport.recv() {
            Ok(Some(f)) => f,
            Ok(None) => continue,
            Err(e) => {
                // If the stream was closed gracefully or dropped, we can just break
                if transport.is_closed() {
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
                let out_path = output_dir.join(
                    Path::new(&file_name)
                        .file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new("received_file")),
                );

                println!("Receiving '{}' ({} bytes)", out_path.display(), file_size);

                file = Some(File::create(out_path)?);

                let progress = ProgressBar::new(file_size);
                progress.set_style(
                    ProgressStyle::with_template(
                        "[{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
                    )
                    .unwrap()
                    .progress_chars("#>-"),
                );
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

                for payload in receiver.drain_ordered() {
                    bytes_received += payload.len() as u64;
                    if let Some(f) = file.as_mut() {
                        f.write_all(&payload)?;
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
                break;
            }
            PacketType::Close => {
                println!("Connection closed by peer.");
                break;
            }
            _ => {
                // Ignore other packets or log
            }
        }
    }

    transport.close()?;
    Ok(())
}

fn run_discover(
    token: Option<String>,
    timeout: u64,
) -> Result<(), Box<dyn std::error::Error>> {
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
