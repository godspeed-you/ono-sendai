# ADR-0121: Packaging tools and layout — `.deb` and `.rpm` from the crate manifest

- Status: accepted
- Date: 2026-08-27
- Spec refs: §37 (delivery), §50; docs/ACCEPTANCE.md §4.5
- Decided by: agent (autonomous)

## Context

The deliverable is a shell someone can set as their login shell and keep (AGENTS.md §15). Until
now the only way to get `ono` onto a machine was `cargo build --release` and a hand `install`,
plus a bare binary attached to a release. A shell that has to be built from source by every
user is not installed; it is demonstrated. The spec is silent on packaging; this ADR fixes the
tools, the package layout and the file names, so that every later increment (the build script,
the referee, CI) works against one decision.

## Decision

1. **Tools.** Packages are produced by `cargo-deb` and `cargo-generate-rpm`, both driven from
   `[package.metadata.deb]` and `[package.metadata.generate-rpm]` in `crates/ono-cli/Cargo.toml`.
   No `debian/` source tree, no `.spec` file, no `rpmbuild`: the crate manifest is the single
   description of the package, and both tools read it. Both are `cargo install --locked`ed at user
   level on a developer machine and fetched as prebuilt binaries in CI.

2. **Package identity.** The package is named `ono` in both formats (the crate stays `ono-cli`).
   The version is the workspace version; the Debian revision is empty and the RPM release is `1`,
   so the files are named

   ```
   dist/ono_<version>_<amd64|arm64>.deb
   dist/ono-<version>-1.<x86_64|aarch64>.rpm
   ```

   `dist/` is ignored by git. The workspace version is the user's decision and is never bumped by
   packaging work.

3. **Layout.** Both packages install exactly this:

   | Path | Content |
   |---|---|
   | `/usr/bin/ono` (0755) | the binary, stripped by the release profile |
   | `/usr/share/doc/ono/README.md` | the repository README |
   | `/usr/share/doc/ono/reference/*.md`, `…/reference/adapters/*.md` | the generated reference of `docs/reference/` (ADR-0018) |
   | `/usr/share/doc/ono/copyright` (deb) / `/usr/share/licenses/ono/LICENSE` (rpm) | the MIT licence |

   No conffiles: `ono` is configured per user under `~/.config/ono/`. No plugin, no fixture, no
   example is packaged — KUANG/11 packages are their own deliverable (spec §31).

4. **Dependencies are computed, not guessed.** The `.deb` declares `$auto` (dpkg-shlibdeps over
   the binary) plus `debianutils` for `add-shell`/`remove-shell` (ADR-0122). The `.rpm` uses
   cargo-generate-rpm's automatic dependency discovery, so `libc.so.6` and friends are required
   at the versions the binary actually links.

5. **Login-shell registration** is a maintainer-script concern, decided in ADR-0122; the scripts
   live in `crates/ono-cli/packaging/deb/` (deb) and inline in the manifest (rpm).

6. **Two referees.** `xtask/tests/packaging.rs` runs in the gate and proves the *shape* against a
   stand-in binary: both tools accept the manifest, the control data names the package, section,
   architecture and dependencies, the file list is the table above, the scripts register and
   unregister the shell. It parses the RPM header itself because the gate machine has no `rpm`.
   `scripts/package-check.sh` is the referee for the *product*: it installs the real packages in
   fresh `debian:bookworm` and `fedora:latest` containers, runs `ono` there as root and as an
   unprivileged user whose login shell is `/usr/bin/ono`, and checks `/etc/shells` before and
   after removal. How the binaries are built for each architecture is ADR-0123.

## Consequences

- `scripts/package.sh [--target <triple>]` writes `dist/`; `scripts/package-check.sh` verifies
  it; `scripts/release-check.sh` runs both after the acceptance suite, so a release cannot be
  declared without an installable package for the host architecture.
- The gate now needs `cargo-deb`, `cargo-generate-rpm` and `dpkg-deb` on the machine; the test
  fails with the tool's name when one is missing, per AGENTS.md §10 ("create the gate tool").
- Adding a file to the package is one line in the manifest and one line in the test.
- The CI release workflow publishes `dist/*` of both architectures on a `v*` tag
  (`.github/workflows/release.yml`).

## Alternatives considered

- **A `debian/` directory and `dpkg-buildpackage`** — the canonical Debian way, but it builds the
  binary itself with the distribution's toolchain, needs a `debian/changelog`, `rules`,
  `control` and `copyright` kept in sync with the manifest, and gives nothing for RPM. Rejected
  for a shell that is not (yet) in any distribution.
- **`rpmbuild` with a `.spec`** — the same duplication on the RPM side and a build dependency
  that is not on the developer machine. Rejected.
- **Only a tarball / a bare binary** — does not register the shell, cannot be removed cleanly,
  and is what the README already offered. Kept as a fallback in the release, not as the product.
- **`cargo-dist`** — builds installers for many platforms, but its Linux packages are a thin
  layer over the same two tools and it wants to own the release workflow. Rejected as a
  framework where two manifest tables suffice (AGENTS.md §4).
