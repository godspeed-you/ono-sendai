# syntax=docker/dockerfile:1.7
# Build and run the ono shell in a clean Linux environment.
#
# The acceptance suite runs against the binary produced here, not against a developer machine:
# a shell that only works where it was written is not a shell (docs/ACCEPTANCE.md).

FROM rust:1.94-slim-bookworm@sha256:cf9dd0ec73e75f827fe59123fff9dc65af1a1c8363c3c31ee8d7f8ad0b6a5fb2 AS builder
WORKDIR /build

# Registry and target caches survive between builds, so the acceptance suite stays cheap enough
# to run on every increment rather than only at the end (AUTONOMOUS_IMPLEMENTATION.md section 20).
ENV CARGO_TERM_COLOR=never CARGO_INCREMENTAL=0
COPY Cargo.toml Cargo.lock* rust-toolchain.toml clippy.toml rustfmt.toml ./
COPY crates ./crates
COPY xtask ./xtask
# The fuzz targets are a workspace member (ADR-0313). The image never builds them — it builds
# `ono-cli` and `ono-kuang-sdk` — but cargo loads every member's manifest before it builds
# anything, so the directory has to be here. Its corpus is a few hundred kilobytes.
COPY fuzz ./fuzz
# The machine-readable contracts are compile-time inputs, not documentation: `ono-value` and
# `ono-command` embed them with `include_str!`, so there is exactly one definition of a schema or
# a command in the process (spec section 27, section 36.4). Copying them here is what makes the
# container prove that the contracts ship with the binary rather than sitting beside it.
COPY docs/spec ./docs/spec
# `target/` survives in a cache mount, and cargo decides what to rebuild from *mtimes*. `COPY`
# preserves the host's mtimes, so a source file edited before the cached artifacts were written
# looks older than them and cargo declares the crate fresh — the image then ships the previous
# binary while carrying the new source, and the acceptance suite silently grades yesterday's
# code. Stamping the workspace sources with the build's own clock is what makes the referee
# grade what was copied. Only the workspace is stamped; the registry's sources keep their
# mtimes, so dependencies stay cached and only `crates/` and `xtask/` are recompiled.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    find crates xtask fuzz docs/spec Cargo.toml Cargo.lock -type f -exec touch {} + && \
    cargo build --release --package ono-cli --package ono-kuang-sdk --package xtask --locked \
    && install -m 0755 target/release/ono /usr/local/bin/ono \
    && install -m 0755 target/release/kuang-example-plugin /usr/local/bin/kuang-example-plugin \
    && install -m 0755 target/release/kuang-sign /usr/local/bin/kuang-sign \
    && install -m 0755 target/release/xtask /usr/local/bin/ono-release-tool

FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171 AS runtime

# Fixtures the provider phases need: real processes, real services, real sockets, real users,
# and the ordinary programs a shell must be able to run.
RUN apt-get update && apt-get install --yes --no-install-recommends \
        ca-certificates bash coreutils procps iproute2 util-linux \
        findutils grep sed gawk diffutils less file psmisc \
        git curl lsof \
        ncurses-base ncurses-bin \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/ono /usr/local/bin/ono
COPY --from=builder /usr/local/bin/kuang-example-plugin /usr/local/bin/kuang-example-plugin
# The signing tool the SDK ships, so a case can be a package author (spec §31.36, ADR-0311).
COPY --from=builder /usr/local/bin/kuang-sign /usr/local/bin/kuang-sign
# The release tooling of §47, and the verification script a reader runs. Case 199 makes a release
# out of the binary this image built and checks it the way a reader would — with no network, as
# an unprivileged user, against real bytes (ADR-0530).
COPY --from=builder /usr/local/bin/ono-release-tool /usr/local/bin/ono-release-tool
COPY scripts/verify-release.sh /usr/local/bin/ono-verify-release

# A non-root user, so privilege boundaries are exercised the way real users meet them.
RUN useradd --create-home --shell /usr/local/bin/ono case
COPY docker/acceptance/fixtures /opt/ono-fixtures
COPY crates/ono-kuang-sdk/examples /opt/ono-examples
RUN chown -R case:case /opt/ono-fixtures

USER case
WORKDIR /home/case

ENV ONO_IN_CONTAINER=1
CMD ["ono"]
