# ADR-0123: Building the packages — in a container, cross for a foreign target, native in CI

- Status: accepted
- Date: 2026-08-27
- Spec refs: §37 (delivery), §34 (nothing here may slow the shell); docs/ACCEPTANCE.md §4.5
- Decided by: agent (autonomous)

## Context

ADR-0121 fixes what the packages contain; this ADR fixes how the binary inside them is built,
and for which processors. Two facts forced it:

1. **The host toolchain is the wrong toolchain.** A binary built on the developer machine
   (Ubuntu 26.04, glibc 2.43) references `GLIBC_2.39` symbols, so its `.deb` says
   `libc6 (>= 2.39)` and refuses to install on `debian:bookworm` (2.36) — the very container
   `scripts/package-check.sh` installs into. The same happens on a GitHub runner
   (ubuntu-24.04, 2.39). The glibc a package requires must come from a chosen build image,
   not from whoever ran the script.
2. **This machine cannot run aarch64 code.** There is no binfmt/qemu registration and, by
   the user's instruction, none is to be installed system-wide. `docker run --platform
   linux/arm64` fails with an exec-format error. An aarch64 binary can be produced here but
   not executed here.

## Decision

1. **Every build runs inside a container, through `cross`.** `scripts/package.sh` never calls
   `cargo build` on the host. `cross` mounts the pinned toolchain of `rust-toolchain.toml` into
   an image and builds there, so the linked glibc is the image's.

2. **The host's own target builds in the acceptance image base.** For `<host triple>`, the
   image is `rust:1.94-slim-bookworm` — the same base `docker/Dockerfile` builds the acceptance
   binary from, selected by `CROSS_TARGET_<TRIPLE>_IMAGE`. Result: a glibc floor of 2.34
   (`__libc_start_main` and friends), which installs on Debian 12, Ubuntu 22.04, and every
   Fedora and RHEL 9+ derivative. The image is multi-arch, so the same rule holds unchanged on
   an aarch64 host.

3. **A foreign target builds in cross's own toolchain image** (`ghcr.io/cross-rs/<triple>`),
   which carries the cross GCC and an old sysroot (glibc 2.18 on aarch64). This is what
   `scripts/package.sh --target aarch64-unknown-linux-gnu` does on the x86_64 developer
   machine. What it proves here: the manifest, the maintainer scripts, the architecture fields
   and the ELF inside the packages are right for aarch64 (`scripts/package-check.sh --target
   aarch64-unknown-linux-gnu` reads them in containers without executing anything). What it
   cannot prove here: that the binary runs, installs and serves as a login shell. And what it
   cannot produce: computed library dependencies — `dpkg-shlibdeps` and `ldd` do not read a
   foreign ELF, so a cross-built package declares `debianutils` only. The script says so.

4. **Released packages are built natively, both architectures.** `.github/workflows/release.yml`
   runs `scripts/package.sh` and `scripts/package-check.sh` on `ubuntu-24.04` and on
   `ubuntu-24.04-arm`, so on each runner the target *is* the host: rule 2 applies, the
   dependencies are computed, and the full install/run/login-shell/remove proof runs on real
   hardware for both `amd64`/`x86_64` and `arm64`/`aarch64`. `package-check.sh` refuses a
   native package whose `libc6`/`libc.so.6` dependency is missing, so a cross-built package can
   never slip into a release by accident.

5. **The referee is the same script everywhere.** Locally, in the non-tag `packaging` job of
   `ci.yml` (x86_64, on every push) and in the release workflow (both architectures), the
   proof is `scripts/package-check.sh`: install in a fresh `debian:bookworm` / `fedora:latest`
   with networking disabled, `ono --version`, a `get process` pipeline as root, the same from
   an unprivileged user whose login shell is `/usr/bin/ono` (via `su -`), `/etc/shells` present
   after install, absent after removal, one entry after a reinstall. The Fedora image is
   prepared once with `util-linux` because the stock image ships no `su`.

## Consequences

- `cross` (user-level `cargo install --locked cross`) and a container runtime are prerequisites
  of `scripts/package.sh`; `cargo-deb` and `cargo-generate-rpm` of the packaging step. The
  scripts name the missing tool and exit 127.
- Two build trees appear under `target/<triple>/release/`; `dist/` is the only output.
- Both packagers honour `SOURCE_DATE_EPOCH` (set to the commit time), so repackaging the same
  build yields the same bytes — checked by hand for this ADR, not by a test, because a byte
  comparison of two builds in different containers is not a property this project needs.
- The local aarch64 package is a developer artefact: right shape, unverified runtime,
  undeclared library dependencies. The README points people at the release assets.
- If `cross` ever stops working with a new toolchain, rule 1 can be met with a plain
  `docker run … cargo build` in the same image; the ADR's contract is "built in a pinned
  container", not "built by cross".

## Alternatives considered

- **Build on the host, declare the host's glibc** — honest but useless: packages that install
  nowhere older than the developer machine. Rejected (fact 1).
- **`rustup target add aarch64-unknown-linux-gnu` plus `gcc-aarch64-linux-gnu`** — needs a
  system-wide package the user did not authorise, and links against the host's multiarch
  sysroot, whose glibc is again 2.43. Rejected.
- **Emulate aarch64 with qemu/binfmt to run the check here** — explicitly excluded by the
  user; also slow enough to be skipped in practice. Native CI runners are free and real.
- **Declare a fixed `libc6 (>= 2.34)` in the manifest instead of `$auto`** — identical across
  hosts, but a claim rather than a measurement; the moment std uses a newer symbol the claim
  is a lie. Computed on native builds, and a refusal to release a package where it was not
  computed, is the safer rule.
