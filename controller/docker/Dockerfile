# The end-to-end harness: the whole controller, with no hardware.
#
# Build context is the repository root, so the image can carry the design
# documents the tests cite.
#
#   docker build -f controller/docker/Dockerfile -t kdtv-harness .
#   docker run --rm -t kdtv-harness
#
# The image holds three things the suite needs and a developer machine may not:
# the pinned Rust toolchain, an aarch64 cross-compiler with a sysroot, and
# qemu-user-static so the ARM64 binary can be executed here. Virtual serial ports
# come from openpty(3) inside the harness itself, so no privileged mode, no
# device mapping and no /dev/ttyUSB* are required.
#
# It cannot reach a real bus. Every fixture in the repository is tier [C], and
# the transmit gate refuses to open a real serial port until Phase 1 capture
# promotes them to tier [A]. There is no hardware mode.

FROM docker.io/library/rust:1.94-bookworm AS harness

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_TERM_COLOR=always \
    CARGO_INCREMENTAL=0

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      gcc-aarch64-linux-gnu \
      libc6-dev-arm64-cross \
      qemu-user-static \
      socat \
      jq \
 && rm -rf /var/lib/apt/lists/*

RUN rustup target add aarch64-unknown-linux-gnu \
 && rustup component add clippy rustfmt

WORKDIR /work

# Dependency manifests first, so a source-only change does not refetch the world.
COPY controller/Cargo.toml controller/Cargo.lock* controller/rust-toolchain.toml ./controller/
COPY controller/crates ./controller/crates
RUN cd controller && cargo fetch --locked || cargo fetch

# Then everything the tests read: the fixtures, the scripts, and the design
# documents the requirement ids point at.
COPY controller ./controller
COPY docs ./docs
COPY research ./research
COPY AGENT.md CLAUDE.md DISCLAIMER.md PROTOCOL.md ./

RUN cd controller && cargo build --workspace --all-targets

WORKDIR /work
CMD ["./controller/scripts/e2e.sh"]
