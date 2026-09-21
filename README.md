<div align="center">
  <img src="desktop/public/plenum-logo.png" width="128" alt="Plenum Logo">
  <h1>Plenum</h1>
  <p><strong>Secure, High-Performance Peer-to-Peer File Transfer</strong></p>
</div>

<br>

Plenum is a blazingly fast, cross-platform file transfer application. Built on a shared Rust core, it provides seamless peer-to-peer file sharing over local networks and the internet using WebRTC.

## Features

- **High Performance:** Powered by a custom Rust-based binary packet engine with sliding-window flow control and backpressure handling.
- **Cross-Platform:** Beautiful, native clients for both Mobile (Flutter) and Desktop (Tauri).
- **Dual Transfer Modes:**
  - *Local Network:* Automatic peer discovery via UDP Broadcast / mDNS.
  - *Internet Relay:* Secure WebRTC data channels using Room Codes.
- **Secure transfers:** LAN payloads use a fresh X25519 key exchange and
  ChaCha20-Poly1305 encryption for every transfer. An optional PIN authenticates
  the LAN handshake. Internet transfers use WebRTC's encrypted data channel.
- **Large File Support:** Engineered specifically for massive files, ensuring your transfers won't overwhelm memory or network buffers.

## Screenshots

## Architecture

Plenum embraces a highly modular architecture that shares a singular core network and protocol engine written in **Rust**.

* **Core Engine (`/src`)**: Rust backend handling framing, flow control, transport (TCP/WebRTC), and signaling.
* **Mobile App (`/mobile`)**: A Flutter app utilizing `flutter_rust_bridge` to bind the UI to the Rust core.
* **Desktop App (`/desktop`)**: A React + Vite frontend packaged as a lightweight Tauri application.

## Getting Started

### Prerequisites

Ensure you have the following installed:
- [Rust](https://rustup.rs/) (latest stable)
- [Flutter](https://flutter.dev/docs/get-started/install) (for mobile)
- [Node.js & npm](https://nodejs.org/) (for desktop)
- [Tauri CLI](https://tauri.app/v1/guides/getting-started/setup/) (for desktop)

### Building the Mobile App

```bash
cd mobile
flutter pub get
# Generate the rust bindings if modifying the core API
flutter_rust_bridge_codegen generate
# Run the app
flutter run
```

### Building the Desktop App

```bash
cd desktop
npm install
# Run the development server
npm run tauri dev
```

### Building the Core Engine CLI

```bash
cargo build --release
./target/release/plenum --help
```

## License

Distributed under the MIT License. See [`LICENSE`](./LICENSE) for more information.

<div align="center">
  <p>Built with love using Rust</p>
</div>
