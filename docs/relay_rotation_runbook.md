# Plenum Relay IP Rotation Runbook

## Overview

Plenum uses WebRTC and a signaling/TURN relay (`relay.plenumonline.me`) for internet transfers. In restricted environments (such as university campuses or corporate networks), DNS queries for unknown or dynamic domains are frequently sinkholed or blocked.

To ensure high transfer reliability, Plenum implements a **multi-tier fallback addressing system**:
1. **Direct DNS**: First attempts standard resolution of `wss://relay.plenumonline.me/ws`.
2. **Fallback IP Candidates**: If DNS fails or times out, Plenum sequentially connects directly to fallback IP candidates via TCP port 443, while preserving full TLS Server Name Indication (SNI) and certificate validation against `relay.plenumonline.me`.
3. **Targeted ICE Server Rewriting**: Once a fallback IP successfully connects, STUN and TURN server URLs (`stun:relay.plenumonline.me:3478`, `turn:relay.plenumonline.me:3478`) are dynamically rewritten to target that specific working IP.

This runbook describes how to rotate, migrate, or add relay server instances **without requiring a new client release**.

---

## Fallback IP Resolution Priority

When fallback applies, Plenum resolves IP candidates in this strict priority order:

| Priority | Source | How to Use | Scope |
| :--- | :--- | :--- | :--- |
| **1** | **User Settings / CLI Override** | `--relay-fallback-ips <ips>` or GUI in Desktop/Mobile Settings | Local client |
| **2** | **Runtime Environment** | `export PLENUM_RELAY_FALLBACK_IPS="1.2.3.4,5.6.7.8"` | Environment |
| **3** | **Remote Dynamic Config** | Fetched non-blocking at startup from `relay-config.json` | All active clients |
| **4** | **Build-time Environment** | `PLENUM_RELAY_FALLBACK_IPS` passed to `cargo build` | Binary build |
| **5** | **Embedded Fallback** | Hardcoded default constant `RELAY_FALLBACK_IPS` | Built-in baseline |

---

## Step-by-Step Server Move Procedure

When moving from an existing relay instance (e.g. `13.232.226.146`) to a new instance (e.g. `NEW_IP`):

### Step 1: Provision and Configure New Instance
1. Launch new Linux instance (Ubuntu 22.04 LTS or newer).
2. Configure security group / firewall rules:
   - `TCP 80, 443` (HTTP/HTTPS/WSS - Caddy)
   - `UDP 3478, 5349` (STUN/TURN signaling - coturn)
   - `UDP 49152:65535` (TURN media relay ports)
3. Clone deployment configuration (`docker-compose.yml`, `Caddyfile`, `.env`).
4. Update `.env`:
   ```bash
   CADDY_DOMAIN=relay.plenumonline.me
   TURN_PUBLIC_IP=<NEW_IP>
   TURN_SHARED_SECRET=<GENERATE_OR_COPY_SECRET>
   ```
5. Start services:
   ```bash
   docker compose up -d
   ```

### Step 2: Verify TLS SNI and Direct IP Connectivity
Before modifying DNS or client configuration, verify that the new IP responds to TLS handshakes targeting `relay.plenumonline.me`:
```bash
# Test HTTPS / WSS handshake with SNI resolution override:
curl -v --resolve relay.plenumonline.me:443:<NEW_IP> https://relay.plenumonline.me/health

# Test TURN credentials endpoint:
curl -v --resolve relay.plenumonline.me:443:<NEW_IP> https://relay.plenumonline.me/turn-credentials?peer_id=test_peer
```

### Step 3: Zero-Downtime Multi-IP Update via `relay-config.json`
To allow clients running with DNS sinkholes to reach both old and new instances during migration:
1. Edit `relay-config.json` in the root repository:
   ```json
   {
     "version": 2,
     "relay_host": "relay.plenumonline.me",
     "relay_server_url": "wss://relay.plenumonline.me/ws",
     "fallback_ips": [
       "<NEW_IP>",
       "<OLD_IP>"
     ],
     "updated_at": "2026-10-19T00:00:00Z"
   }
   ```
2. Commit and push to `main`.
3. Plenum clients automatically fetch this configuration in the background on startup (3-second timeout, non-blocking).

### Step 4: Update DNS Record
1. In Cloudflare / Route53, update the A record for `relay.plenumonline.me`:
   - Set value to `<NEW_IP>`.
   - Set TTL to 300 seconds (5 minutes) initially.
2. Wait for DNS propagation (~5–15 minutes).

### Step 5: Verify DNS Resolution
```bash
nslookup relay.plenumonline.me
dig +short relay.plenumonline.me
```
Verify clients with healthy DNS connect via domain name without triggering fallback.

### Step 6: Verify Fallback Operation
Simulate a DNS sinkhole on a test client by pointing `relay.plenumonline.me` to `127.0.0.1` in `/etc/hosts` or `C:\Windows\System32\drivers\etc\hosts`:
```text
127.0.0.1 relay.plenumonline.me
```
Run a transfer or start the client:
- The client detects that `127.0.0.1` fails or times out.
- The client queries candidate fallback IPs in order (`NEW_IP`, then `OLD_IP`).
- The client connects successfully and outputs diagnostic log:
  ```text
  DIAG offerer: connected to relay via fallback IP <NEW_IP> (DNS sinkhole workaround active)
  ```

### Step 7: Decommission Old Instance
1. Monitor traffic and connection logs on the old instance:
   ```bash
   docker compose logs -f
   ```
2. Once active connections have dropped to zero (typically 24–48 hours grace period), stop services and terminate the instance:
   ```bash
   docker compose down -v
   ```
3. Update `relay-config.json` to remove `<OLD_IP>`, leaving only `<NEW_IP>`.

---

## Client User Override (Emergency / Manual)

If an end-user needs to connect to a specific relay IP immediately without updating the application:

### Desktop App
1. Open **Settings** (gear icon).
2. Scroll to **Network & Relay**.
3. Under **Relay Fallback IPs**, enter the new IP address(es) (e.g. `13.232.226.146` or comma-separated `1.2.3.4, 5.6.7.8`).
4. Press Enter or click outside the field. Changes take effect immediately.

### Mobile App (Android / iOS)
1. Navigate to the **Settings** tab.
2. Under **Network & Relay**, tap **Relay Fallback IPs**.
3. Enter the IP address and tap **Save**.
4. To revert to defaults at any time, tap the clear (X) icon or **Reset Default**.

### CLI
Pass the `--relay-fallback-ips` argument or set the environment variable:
```bash
plenum send --file-path photo.jpg --target 7K9-X2M-4P1 --relay-fallback-ips 1.2.3.4,5.6.7.8
```
or:
```bash
export PLENUM_RELAY_FALLBACK_IPS="1.2.3.4,5.6.7.8"
plenum receive
```
