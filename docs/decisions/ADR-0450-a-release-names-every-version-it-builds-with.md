# ADR-0450: A release names every version it builds with

- Status: accepted
- Date: 2026-09-02
- Spec refs: spec §44.2 (tool versions), §44.3 (Rust toolchain), §44.4 (dependency fetch
  reproducibility), §46.1 (reproducible build definition), §65.11 (mutable release inputs)
- Issues: #102
- Decided by: agent (autonomous)

## Context

ADR-0433 pinned the *actions* and the *images* a release pulls. Between those two layers sits a
third that was still floating: the programs that turn a compiled binary into a package, and the
dependency graph they compile.

`.github/workflows/release.yml` and `.github/workflows/ci.yml` both installed
`cargo-deb,cargo-generate-rpm,cross` through `taiki-e/install-action`, which installs the newest
release of each. `cargo-deb` decides the layout of a `.deb`, its control fields and the order of
its members; `cargo-generate-rpm` does the same for an `.rpm`. Two runners a week apart install
two versions and produce two different packages from one commit — the same failure ADR-0433
removed from images, one layer up, and the one spec §46.1 will measure with a byte-for-byte
comparison.

`docker/Dockerfile` was worse than unpinned. It read:

```dockerfile
cargo build --release --package ono-cli --package ono-kuang-sdk --locked \
  || cargo build --release --package ono-cli --package ono-kuang-sdk \
```

The fallback exists to turn a red build green, and what it turns green is precisely the case
`--locked` is for: a `Cargo.lock` that no longer describes the manifests. The flag was there and
could not fail. Spec §44.4 asks for the opposite in one sentence — "a release build MUST fail if
lockfile resolution would change".

And `scripts/package.sh` checked that `cargo-deb` was *installed*, which is a different question
from which one.

## Decision

**Every version a release depends on is written in the workspace manifest, once, and every place
that installs or requires one names that version.**

```toml
[workspace.metadata.release-tools]
cargo-deb = "3.7.0"
cargo-generate-rpm = "0.21.0"
cross = "0.2.5"
cargo-deny = "0.20.2"
```

`check_tool_versions` in `xtask/src/supply_chain.rs` holds three rules over the release path —
the workflows, the Dockerfiles, and the scripts at the top of `scripts/`:

- an `install-action` `tool:` entry without `@version` fails. `cargo-deb` becomes
  `cargo-deb@3.7.0`;
- any `name@version` mention of a registered tool, anywhere on that path, must be the registered
  version. The register is the one place a bump is made, and a script that disagrees with a
  workflow is a red gate rather than a surprise in six months;
- a register entry nothing installs fails too, so the list cannot outlive the thing it describes.

The Rust toolchain follows the same shape in the file that already owns it: `rust-toolchain.toml`
must name an exact version rather than a channel, and a workflow that asks
`dtolnay/rust-toolchain` for a toolchain must ask for that one (spec §44.3). Both jobs already
passed `toolchain: "1.94"`; nothing now lets the two drift apart.

`check_locked_builds` requires `--locked` on every `cargo build` and `cross build` on the release
path, per line rather than per file — which is what refuses the Dockerfile's fallback, since the
first invocation carried the flag and the second was the whole problem. The fallback is gone.

`scripts/package.sh` no longer asks whether the packaging tools exist; it asks which ones, refuses
a version that is not the registered one, and prints the `cargo install --locked <tool>@<version>`
that fixes it. A developer packaging locally with `cargo-deb` 3.5 was producing an artifact the
release could not reproduce, and finding that out at the byte-comparison of §46.5 would be finding
it out late.

`scripts/demo/` is outside the release path by construction: it builds nothing a user installs,
and it mentions `cargo build` in a hint string rather than running one.

## Consequences

Easy: the release input list is now four lines of TOML plus one toolchain file, and #103's
manifest reads them rather than re-deriving them. A tool bump is a diff a reviewer can judge — one
version, one place, with the workflows and the script following it or the gate going red.

Hard, and the same trade ADR-0433 accepted: **the pinned tool versions age.** `cargo-deb` 3.8 will
not be installed until somebody edits the register, and a fix in it does not arrive on its own.
The alternative is a release whose contents depend on the date, which spec §65.11 names as the
failure this phase exists to remove.

Also hard: `scripts/package.sh` now refuses to run for a developer whose `cargo-deb` is a
different version. That is deliberate — a package built with the wrong tool is not the package
the release publishes — and the message says exactly what to install.

One risk was taken and is worth naming: removing the Dockerfile's unlocked fallback means the
acceptance image build now fails when `Cargo.lock` is stale, instead of quietly re-resolving. That
*is* the requirement of §44.4, and the failure is loud, immediate and fixed by `cargo update -p
<crate>` or by committing the lockfile the manifests imply.

Encoded by:

- `xtask/tests/supply_chain.rs::should_reject_a_tool_installed_without_a_version`,
  `::should_reject_a_tool_installed_at_a_version_the_register_does_not_name`,
  `::should_accept_a_tool_installed_at_the_registered_version`,
  `::should_reject_a_release_job_asking_for_a_toolchain_the_repository_does_not_pin`,
  `::should_reject_a_rust_toolchain_that_follows_a_channel_instead_of_a_version`,
  `::should_reject_a_registered_tool_version_that_disagrees_with_a_script`,
  `::should_report_a_registered_tool_version_nothing_installs`,
  `::should_find_an_exact_version_for_every_release_tool`;
- `::should_reject_a_release_build_that_does_not_lock_the_dependency_graph`,
  `::should_reject_a_fallback_that_builds_again_without_the_lock`,
  `::should_accept_a_developer_script_that_only_mentions_a_build_command`,
  `::should_build_the_release_with_a_locked_dependency_graph`;
- `xtask/tests/packaging.rs::should_refuse_a_release_build_whose_lockfile_would_change`, which
  arranges a stale lockfile from outside cargo and proves the build refuses rather than repairs —
  and proves the same workspace builds without the flag, so the fixture is not vacuous.

## Alternatives considered

**Keep the versions in the workflows and let the register be a comment.** Two workflows, two
scripts and a Dockerfile name these tools. Five copies of a version string is five chances for
four of them to be right.

**Pin with a lockfile-style manifest (`cargo-binstall`, `rust-toolchain`-style tool file).** A
second file format to learn, for four lines. `workspace.metadata` is already where this
repository keeps the supply-chain facts that cargo ignores (ADR-0449).

**Let `scripts/package.sh` warn rather than refuse on a version mismatch.** A warning in a script
that prints twenty lines is a warning nobody reads, and the artifact it produced is
indistinguishable from the right one until §46.5 compares hashes.

**Check `--locked` per file rather than per line.** Simpler, and it would have passed the exact
Dockerfile line this ADR removed.
