# Build and run the ono shell in a clean Linux environment.
#
# The acceptance suite runs against the binary produced here, not against a developer machine:
# a shell that only works where it was written is not a shell (docs/ACCEPTANCE.md).

FROM rust:1.94-slim-bookworm AS builder
WORKDIR /build

# Cache dependency compilation separately from source changes.
COPY Cargo.toml Cargo.lock* rust-toolchain.toml clippy.toml rustfmt.toml ./
COPY crates ./crates
COPY xtask ./xtask
RUN cargo build --release --package ono-cli --locked || cargo build --release --package ono-cli

FROM debian:bookworm-slim AS runtime

# Fixtures the provider phases need: real processes, real services, real sockets, real users.
RUN apt-get update && apt-get install --yes --no-install-recommends \
        ca-certificates procps iproute2 util-linux coreutils bash \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/ono /usr/local/bin/ono

# A non-root user, so privilege boundaries are exercised the way real users meet them.
RUN useradd --create-home --shell /usr/local/bin/ono case
USER case
WORKDIR /home/case

ENV ONO_IN_CONTAINER=1
CMD ["ono"]
