# relay-server

Self-hosted relay/signaling server for Plenum's internet (NAT-traversal)
transfers. Reuses `plenum::signaling` for the join/offer/answer/ICE-candidate
routing state machine, adds a WebSocket transport (`axum`) and short-lived
TURN credential minting (coturn's `use-auth-secret` REST scheme).

Endpoints:

- `GET /ws` — WebSocket signaling channel. First message must be
  `JoinSession`; see `src/ws.rs` for the full connection lifecycle.
- `GET /turn-credentials?peer_id=<id>` — mints a short-lived (1 hour) coturn
  REST-API credential pair. Returns 404 if `TURN_SHARED_SECRET` isn't set.
- `GET /healthz` — plain-text `200 OK` for container health checks.

## Running locally (no Docker)

```
cargo run -p relay-server
```

Configuration is entirely via environment variables:

| Variable              | Default         | Purpose                                          |
|-----------------------|-----------------|---------------------------------------------------|
| `RELAY_BIND_ADDR`     | `0.0.0.0:8080`  | Socket address to bind                             |
| `RELAY_PUBLIC_URL`    | *(unset)*       | Logged on startup only, for operator convenience   |
| `TURN_SHARED_SECRET`  | *(unset)*       | coturn `static-auth-secret`; unset disables TURN   |
| `TURN_URLS`           | *(empty)*       | Comma-separated TURN server URLs returned to clients |
| `RUST_LOG`            | `info`          | `tracing_subscriber::EnvFilter` string             |

## Deploying with Docker Compose

The repo root `docker-compose.yml` runs three services:

- **relay-server** — this crate, built from the repo root (see below).
- **coturn** — the TURN server, using coturn's official image and the
  `use-auth-secret` REST auth scheme so `relay-server` can mint per-session
  credentials instead of a static shared username/password.
- **caddy** — reverse proxy in front of `relay-server`, handling automatic
  Let's Encrypt TLS so the WebSocket endpoint is reachable at `wss://...`.

### 1. Build context

`relay-server` path-depends on the root `plenum` crate (`plenum = { path =
".." }`), so it cannot be built with `relay-server/` alone as the Docker
build context — the whole workspace (root `Cargo.toml`/`Cargo.lock`, `src/`,
and the other workspace members' manifests) has to be visible to `cargo
build`. `docker-compose.yml` is already configured for this:

```yaml
relay-server:
  build:
    context: .
    dockerfile: relay-server/Dockerfile
```

If you build the image manually (without compose), run it from the repo
root, not from inside `relay-server/`:

```
docker build -f relay-server/Dockerfile -t plenum-relay-server .
```

### 2. Environment setup

Create a gitignored `.env` file at the repo root (compose loads it
automatically):

```
TURN_SHARED_SECRET=$(openssl rand -hex 32)
CADDY_DOMAIN=relay.example.com
TURN_PUBLIC_IP=<this host's public/Elastic IP>
TURN_PRIVATE_IP=<this host's private IP, e.g. from `hostname -I`>
```

- `TURN_SHARED_SECRET`: generate with `openssl rand -hex 32`. Shared between
  `relay-server` (to mint credentials) and `coturn` (to verify them) — both
  read it from the same `.env` via compose variable substitution.
- `CADDY_DOMAIN`: a DNS name that already points at this host's public IP.
  Caddy uses it both to request a Let's Encrypt certificate and to know
  which `Host` header to route.
- `TURN_PUBLIC_IP` / `TURN_PRIVATE_IP`: **required on any host behind NAT
  (EC2, most cloud VMs).** Without these, coturn auto-discovers all local
  addresses and may advertise the private IP as a relay candidate — clients
  outside the VPC can never reach it, so TURN relay fallback silently fails
  for peers on genuinely separate networks (same-LAN or lucky-NAT transfers
  will still appear to work, masking the bug). `TURN_PRIVATE_IP` is what
  coturn binds/relays on internally; `TURN_PUBLIC_IP` is what it reports to
  clients in candidate strings. On EC2 find these with `hostname -I` (private)
  and the console's "Public IPv4 address" / an Elastic IP (public).

`.env` is already covered by the repo's root `.gitignore` (`.env` /
`.env.*`) — do not commit your real secret.

### 3. Start everything

```
docker compose up -d
```

Caddy will occupy ports 80/443 on the host (needed for the ACME HTTP-01
challenge and for serving `https://`/`wss://`). `relay-server` itself is not
published to the host directly — only reachable through Caddy.

### 4. coturn networking: host mode vs. bridge mode

By default `docker-compose.yml` runs `coturn` with `network_mode: host`,
which is the simplest way to expose the full TURN relay UDP port range
(`49152-49452` here) without publishing thousands of individual Docker port
mappings. This requires a host that allows host networking for containers,
which some managed/restricted platforms (e.g. certain PaaS offerings) do
not.

**Fallback: bridge mode.** If your host disallows `network_mode: host`,
replace the `coturn` service's `network_mode: host` line with explicit port
publishing:

```yaml
coturn:
  image: coturn/coturn:latest
  restart: unless-stopped
  ports:
    - "3478:3478/udp"
    - "3478:3478/tcp"
    - "5349:5349/tcp"
    - "49152-49452:49152-49452/udp"
  command:
    - "-n"
    - "--use-auth-secret"
    - "--static-auth-secret=${TURN_SHARED_SECRET}"
    - "--realm=plenum.local"
    - "--listening-ip=${TURN_PRIVATE_IP}"
    - "--relay-ip=${TURN_PRIVATE_IP}"
    - "--external-ip=${TURN_PUBLIC_IP}/${TURN_PRIVATE_IP}"
    - "--min-port=49152"
    - "--max-port=49452"
```

Note the full `min-port`-`max-port` UDP range must be published explicitly
in bridge mode — TURN relay allocations use ports from this range directly,
unlike a typical single-port service.

## Manual end-to-end verification

1. `docker compose up -d` with `TURN_SHARED_SECRET` and `CADDY_DOMAIN` set.
2. Run two desktop app instances on **genuinely separate networks** (not the
   same LAN — e.g. one on a mobile hotspot, one on a different ISP) so NAT
   traversal actually has something to traverse.
3. In Settings on both instances, set the Relay Server URL to
   `wss://<CADDY_DOMAIN>/ws`.
4. On the receiving instance, switch to Internet mode and note the
   generated room code. On the sending instance, switch to Internet mode,
   pick a file, enter the code, and connect.
5. Confirm the transfer completes and checksums match.
6. Repeat with one peer behind a **symmetric NAT** (e.g. carrier-grade
   mobile NAT) to confirm the TURN relay fallback actually engages — check
   coturn's logs for an allocation, not just STUN binding success.
7. Check the `typ relay` candidate strings exchanged in `relay-server`'s
   logs — they must show `TURN_PUBLIC_IP`, never `TURN_PRIVATE_IP`. Same-NAT
   tests (both peers behind the same router/carrier) can complete via a
   direct `srflx`/`host` pair even when the relay candidate is broken and
   unreachable, silently masking this bug — genuinely separate networks are
   the only real test.

## Testing

```
cargo test -p relay-server
```

`tests/ws_routing.rs` spins up the real axum router on an ephemeral loopback
port and drives it with real `tokio-tungstenite` WebSocket clients, covering
the join-order synthesis behavior, Offer/Answer/IceCandidate routing, and
`PeerLeft` delivery on ungraceful disconnect.
