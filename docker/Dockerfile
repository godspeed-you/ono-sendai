# syntax=docker/dockerfile:1.7
# Build and run the ono shell in a clean Linux environment.
#
# The acceptance suite runs against the binary produced here, not against a developer machine:
# a shell that only works where it was written is not a shell (docs/ACCEPTANCE.md).

FROM rust:1.94-slim-bookworm AS builder
WORKDIR /build

# Registry and target caches survive between builds, so the acceptance suite stays cheap enough
# to run on every increment rather than only at the end (AUTONOMOUS_IMPLEMENTATION.md section 20).
ENV CARGO_TERM_COLOR=never CARGO_INCREMENTAL=0
COPY Cargo.toml Cargo.lock* rust-toolchain.toml clippy.toml rustfmt.toml ./
COPY crates ./crates
COPY xtask ./xtask
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    cargo build --release --package ono-cli --locked \
      || cargo build --release --package ono-cli \
    && install -m 0755 target/release/ono /usr/local/bin/ono

FROM debian:bookworm-slim AS runtime

# Fixtures the provider phases need: real processes, real services, real sockets, real users,
# and the ordinary programs a shell must be able to run.
RUN apt-get update && apt-get install --yes --no-install-recommends \
        ca-certificates bash coreutils procps iproute2 util-linux \
        findutils grep sed gawk diffutils less file psmisc \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/ono /usr/local/bin/ono

# A non-root user, so privilege boundaries are exercised the way real users meet them.
RUN useradd --create-home --shell /usr/local/bin/ono case
COPY docker/acceptance/fixtures /opt/ono-fixtures
RUN chown -R case:case /opt/ono-fixtures

USER case
WORKDIR /home/case

ENV ONO_IN_CONTAINER=1
CMD ["ono"]
