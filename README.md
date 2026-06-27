# Plenum

Plenum is a Rust-based peer-to-peer file transfer engine focused on secure, high-performance transfers with a modular architecture.

The project separates peer discovery, transport, protocol framing, and stream control so each layer can evolve independently. Plenum is designed to support local discovery through mDNS or UDP broadcast, remote discovery through signaling servers, and multiple transport options such as TCP, UDP, WebRTC data channels, or future custom protocols.

At its core, Plenum will implement a custom binary packet format with checksums, chunk reassembly, backpressure, and sliding-window flow control to transfer large files reliably without overwhelming memory or network buffers.

## Initial Focus

The first milestone is to build the protocol layer in Rust:

- Define the binary packet format.
- Encode file chunks into framed packets.
- Parse packets back from raw bytes.
- Verify packet integrity with checksums.
- Reassemble the original data in memory.

Networking and peer discovery will be added after the packet framing layer is correct and well tested.

## License

Plenum is licensed under the [MIT License](./LICENSE).
