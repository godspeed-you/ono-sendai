# ADR-0910: The core build is a feature set with two profiles, full and core

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.2 §24.2, §31, §37; v0.3; v0.4; v0.5; v0.6; ADR-0863 §4 (the user's direction)
- Issues: #127
- Decided by: user (direction), agent (feature set, seams)

## Context

ADR-0863 §4 records the user's direction: a build of the object shell alone, for container
images and embedded Linux targets. It contains the language, the evaluator, typed pipelines, the
native commands and the Linux and network providers, and it is statically linked. It leaves out
the systemd, container, remote, spatial, graph, adapter and KUANG/11 tiers. The default build has
to stay exactly today's product, so no test or acceptance case changes behaviour.

No crate had a `[features]` table. The tiers are wired into `crates/ono-cli` directly, and they
reach into one another. The temporal seam of `providers.rs` asks the spatial session where it
stands. A change plan reads both the spatial index and the temporal ledger. A KUANG/11 package
contributes places, and the storage provider in `ono-provider-linux` speaks the systemd bus for
its mount units.

Two tiers exist today that did not exist when the direction was given on 2026-09-04: temporal
(v0.5, released 2026-09-09) and change (v0.6). The direction names a positive list, and the
issue's title is "the object shell *without its enhancements*". Both are enhancement
specifications layered on the v0.4 spatial model (AGENTS.md §5.2), and neither is on the list.
Both also depend on the spatial tier, and they bring the only C dependency left in the graph once
`ring` has gone: the bundled SQLite of the temporal ledger and the plan store.

## Decision

1. **Two profiles.** `ono-cli` builds as `full` or as `core`:
   - `full` is `default = ["full"]`, the whole product. `cargo build -p ono-cli` builds exactly
     what it built before.
   - `core` is `cargo build -p ono-cli --no-default-features --features core`. The `core` feature
     enables nothing. It exists so the command says which profile it builds.

2. **Nine tier features, one decision.** `adapter`, `change`, `container`, `graph`, `kuang`,
   `remote`, `spatial`, `systemd` and `temporal` each enable their optional dependencies. They
   are named separately so that every `cfg` says which tier it belongs to. `lib.rs` refuses any
   build with some of the nine but not all of them (`compile_error!`). The tiers call into each
   other, so a partial set would be a program nobody has compiled or tested. A feature
   combination nobody builds does not compile.

3. **What the core contains.** The language, the evaluator, the typed pipeline, the native
   commands of the base specification, jobs and the line editor. It also contains the Linux
   providers (process, file, identity, env, storage, device, packages) and the netlink and
   network providers. The hardening of v0.4.1 is not a tier and stays whole.

4. **What it leaves out.**
   - v0.3 adapters.
   - The v0.4 spatial interface.
   - The v0.5 temporal interface.
   - The v0.6 change interface.
   - Remote links and the agent (§21).
   - The graph's `trace` (§22).
   - KUANG/11 (§31).
   - The systemd, logind and journal providers.
   - The container engine provider.

   The temporal and change tiers are left out as the enhancements they are. This decision reads
   the user's positive list as the scope, and the list of seven as its examples.

5. **Seams at module and registration boundaries, not at call sites.**
   - A tier module that the rest of the shell calls into (`spatial`, `temporal`, `change`,
     `remote`, `plugins`, `plugin_registry`) is replaced in the core build by a small inert
     module under `crates/ono-cli/src/absent/`, mounted at the same path with `#[path]`. Each
     inert module holds only what the core path asks of the tier: that no place is active, that
     no stage is claimed, that a session stays in the present. Call sites do not change.
   - Registrations are gated where they are made: the spatial, temporal and change commands in
     one function of `eval/native/mod.rs`, and the providers in `providers.rs`.
   - Types that carry a tier's state are gated where they are declared: the session's link table
     and the KUANG/11 host.
   - The adapter tier is the `ono_adapter::Registry` a session builds. The core build builds it
     empty, so every program runs as bytes, as a program no adapter names always has.
   - The graph tier is the `trace` verb. Its commands are removed from the core registry, and
     nothing binds them.
   - `ono-adapter`, `ono-graph` and `ono-temporal-core` stay dependencies. They are pure Rust,
     their types are part of `ono-command`'s API, and only what the core reaches of them is
     linked.

6. **The storage provider's systemd coupling is a feature of `ono-provider-linux`.**
   `systemd = ["dep:ono-provider-systemd"]` is on by default. The workspace dependency declares
   `default-features = false`, and `ono-cli`'s `systemd` feature enables it. Without it,
   `start`/`stop mount` fail their row with `provider.unavailable` and the help a host without a
   system bus gives. This removes the last user of `zbus` from the core graph. ADR-0863 measured
   that dropping only the systemd registrations saved 0.3 MB for exactly this reason.

## Consequences

- The core dependency graph holds no `wasmtime`, `zbus`, `rustls`, `ring`, `ureq`, `x509-parser`
  or `rusqlite`, and no C code. `cargo tree -p ono-cli --no-default-features --features core`
  shows it.
- The size profile of ADR-0863 gives these stripped sizes (2026-09-24, toolchain 1.94), on top
  of #126, which took wasmtime's compiler out of the full build (ADR-0870):

  | build | size |
  |---|---|
  | core, glibc | 6,913,976 bytes |
  | core, static musl | 7,063,744 bytes |
  | full, glibc | 22,155,744 bytes (28,736,736 before #126) |

  ADR-0912 records the musl build.
- The full build's behaviour is unchanged. A few functions moved without changing behaviour: the
  tier renderers of `sink.rs` are now one gated helper, the link routing of
  `Session::pipeline_context` is its own function, and the spatial, temporal and change
  registrations are one function. The existing suites cover them.
- Every new tier has to say where its seam is. A module the core path calls needs an inert
  counterpart under `absent/`, and a new command family needs an entry in
  `absent::tier_of`. Otherwise the core build either fails to compile, which is the intended
  failure, or advertises the tier.
- CI builds, lints and tests the core profile on every push (ADR-0913). The configuration does
  not rot unseen.

## Alternatives considered

**One feature per tier, freely combinable.** This was rejected: the tiers reach into each other
(temporal ↔ spatial ↔ change, KUANG/11 → spatial contributions). Each combination would need its
own seams and its own CI job, and no user asked for a shell with maps but no ledger.

**A `cfg` at every call site.** This was rejected: the issue counts roughly 250 spatial sites,
and scattered `cfg`s would make both builds harder to read. Seams at module and registration
boundaries keep the call sites identical in both profiles.

**Keeping temporal and change in the core.** This was rejected. They are enhancement
specifications on top of the spatial tier, which the direction excludes. They cannot run without
it, and they bring the bundled SQLite back into a build that otherwise has no C code.

**Moving the tiers out of `ono-cli` into crates of their own.** That is the refactor this issue
explicitly does not ask for. The feature set does not prevent it later.
