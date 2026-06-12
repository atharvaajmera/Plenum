use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};

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
        /// Address of the peer (e.g., 127.0.0.1:8080)
        address: String,
    },
    /// Receive a file from a peer
    Receive {
        /// Port to listen on
        port: u16,
        /// Directory to save the received file
        #[arg(short, long, default_value = ".")]
        output_dir: PathBuf,
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
        Commands::Send { file_path, address } => run_send(file_path, address),
        Commands::Receive { port, output_dir } => run_receive(port, output_dir),
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

fn run_receive(port: u16, output_dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port))?;
    println!("Listening on port {}...", port);

    let mut transport = TcpTransport::accept(&listener)?;
    println!("Connection established.");

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
                let out_path = output_dir.join(Path::new(&file_name).file_name().unwrap_or_else(|| std::ffi::OsStr::new("received_file")));
                
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
