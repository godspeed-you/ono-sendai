# ADR-0928: The toolchain moves to 1.95 so the component runtime can take wasmtime's fixes

- Status: accepted
- Date: 2026-10-04
- Spec refs: v0.4.1 §44.3, §45.1
- Decided by: agent (autonomous)

## Context

The nightly `audit` workflow has been red since 2026-09-29 on an unchanged `main`: the advisory
database gained nine advisories against the component runtime of ADR-0569, five against
`wasmtime` 47.0.4 (RUSTSEC-2026-0315, -0316, -0325, -0326, -0327) and four against
`wasmtime-wasi` 47.0.4 (RUSTSEC-2026-0314, -0321, -0322, -0323, -0324). They cover fuel
accounting a guest can escape, GC heap corruption, a native stack buffer overflow in
async-lifted callbacks, uninitialised padding copied into guest memory and host panics a guest
can provoke: the boundary KUANG/11 relies on to confine a component.

No 47.x release carries the fixes. The patched lines are 36.0.17 (LTS), 48.0.4 and 49.0.2, and
the two current ones raise their minimum Rust: 48.0.4 needs 1.95, 49.0.2 needs 1.96. The
toolchain has been pinned to 1.94 since ADR-0001.

§45.1 lets a known vulnerability past the gate only with a reason its path is unreachable and an
expiry date. The supervisor runs untrusted components through exactly the paths these advisories
name, so no honest reason exists.

## Decision

**`rust-toolchain.toml` pins 1.95, and the workspace depends on `wasmtime`/`wasmtime-wasi` 48.**
The lockfile resolves 48.0.5, the newest 48 release. Everything that names the release toolchain
moves with it in the same commit, because `xtask` already refuses a workflow that asks for a
toolchain other than the pinned one (spec §44.3): `Cargo.toml`'s `rust-version`, the
`dtolnay/rust-toolchain` steps of `ci.yml`, `release.yml` and `audit.yml`, the
`rust:1.95-slim-bookworm` builder image (pinned by its index digest, ADR-0433) and the documents
that state the version.

The smallest step that closes all nine advisories wins: one toolchain minor and one wasmtime
major.

`docs/contracts/hardening/performance_environment.yaml` keeps `rust_toolchain: "1.94"`: it states
the environment the current baseline was measured in (ADR-0926), and the baselines under
`docs/baselines/` record what their releases were built with. Both describe the past.

## Consequences

- `cargo deny` passes again; the nightly `audit` turns green.
- `WASMTIME_VERSION` in the supervisor becomes `48.0.5`; the test that holds it to the lockfile
  (ADR-0916) is what makes the constant move with the dependency.
- An artifact `kuang-compile` wrote before carries `wasmtime-47.0.4` and is refused at load, as
  after any wasmtime release since ADR-0916; running `kuang-compile` again (or `install plugin`)
  writes a current one (ADR-0870, acceptance case 351).
- The release built next names toolchain 1.95 in `dist/build-inputs.json`; reproducing an
  earlier release still needs that release's toolchain, which its manifest names.
- Contributors need Rust 1.95; rustup fetches it from `rust-toolchain.toml`.

## Alternatives considered

- **wasmtime 49.0.2 on toolchain 1.96.** Also closes every advisory, at the price of two
  toolchain minors and two wasmtime majors in one step. Rejected as the larger change for the same
  security result; the next move is open once 48 stops getting fixes.
- **wasmtime 36.0.17 LTS on toolchain 1.94.** Closes the advisories without a toolchain change,
  but is eleven majors back from the API the supervisor was written against. Rejected.
- **A waiver in `deny.toml`.** §45.1 requires the vulnerable path to be unreachable; here it is
  the plugin boundary itself. Rejected.
