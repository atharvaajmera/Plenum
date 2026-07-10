# Multi-stage build for relay-server.
#
# IMPORTANT: this crate path-depends on the root `plenum` crate (`plenum =
# { path = ".." }`), so it cannot be built from the `relay-server/` directory
# alone. The Docker *build context* must be the repository root, and this
# Dockerfile must be invoked accordingly, e.g. from the repo root:
#
#   docker build -f relay-server/Dockerfile -t plenum-relay-server .
#
# docker-compose.yml at the repo root is already configured with
# `context: .` / `dockerfile: relay-server/Dockerfile` for this reason.

# ---- Stage 1: build ---------------------------------------------------
FROM rust:1-slim AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

# Copy the whole repository. relay-server path-deps on the root `plenum`
# crate, and the workspace also lists desktop/src-tauri and mobile/rust as
# members, so Cargo needs their full manifests (and, since some are
# tauri-build/flutter_rust_bridge codegen crates with build scripts that
# read other project files, their real source trees too) to resolve the
# workspace even though we only compile the relay-server package below.
# Copying everything is simpler and more robust than trying to stub out
# just enough of the other members.
COPY . .

RUN cargo build --release -p relay-server

# ---- Stage 2: runtime ---------------------------------------------------
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /workspace/target/release/relay-server /usr/local/bin/relay-server

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/relay-server"]
