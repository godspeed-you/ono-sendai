# ADR-0531: The baseline is the floor the binary actually has

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §48.1 (existing real-install checks remain), §48.2 (new validation), §48.3
  (dependency floor), §48.4 (artifact identity), §62.6 (package identity), §44.1 (image digests)
- Issues: #109 — extends ADR-0121/ADR-0122's `scripts/package-check.sh`, feeds #110
- Decided by: agent (autonomous)

## Context

`scripts/package-check.sh` already installed both packages in fresh networkless containers and
proved they run as root and as a login shell. §48.2 adds nine checks to that, and §48.3 adds a
second distribution:

> Package validation MUST run on the oldest supported glibc/distribution baseline as well as one
> current representative distribution. The oldest supported baseline is the binding compatibility
> proof.

"Oldest supported baseline" is the phrase that needed a decision. A distribution tag is not a
compatibility proof: `debian:bookworm` is only a floor if the binary genuinely runs on its glibc,
and nothing had ever measured that.

## Decision

**The baseline is `debian:bookworm`, because it is the base of the image the release compiles in,
and the script holds the binary to its glibc rather than to its name.** `debian:trixie` is the
current representative, and the whole `.deb` half — structure and install — runs in both.

```text
DEBIAN_BASELINE  debian:bookworm@sha256:6ebd97fa…   glibc 2.36, the base of the build image
DEBIAN_CURRENT   debian:trixie@sha256:f324c7ff…     one current representative
FEDORA_IMAGE     fedora:latest@sha256:43b29f65…     the .rpm's referee
GLIBC_FLOOR      2.36
```

The floor is checked, not assumed: the highest `GLIBC_2.x` symbol version in the packaged binary
is read straight out of the ELF — the version strings are plain text in `.gnu.version_r`, so no
binutils are needed in the container — and compared against `GLIBC_FLOOR`. The first run reported
`requires at most GLIBC_2.34`, which is a measurement and now a regression guard: a dependency
that raises it fails validation on the day it lands rather than on a user's machine.

### The nine checks of §48.2, and where each lives

Structural, so they also hold for a package built for the other architecture:

- **package metadata matches the artifact filename.** The version and architecture are parsed out
  of the *filename* and compared against the package's own metadata, rather than both being
  restated from `cargo pkgid`. A file named for one version holding another is how a release ships
  the wrong package under the right name.
- **no private build paths are embedded.** `/home/<somebody>/` or `/Users/<somebody>/` inside the
  binary means it was built on a workstation and carries that workstation's directory layout to
  everyone who installs it. The release compiles at `/project` inside a container, so the check
  should always pass — which is exactly why it has to run.

After installation, in both Debian images and in Fedora:

- **binary version equals release version** — `ono --version` against the release version;
- **the expected path exists** — `/usr/bin/ono`, executable;
- **ownership and mode are correct** — `root root 755`, read with `stat`;
- **uninstall leaves user configuration** — the check writes
  `/home/probe/.config/ono/config.ono` as the user before removing the package, and requires it to
  be there afterwards, unchanged. Removing a shell must not remove what the person using it wrote;
- **reinstall works**, and does not duplicate the `/etc/shells` entry, and does not lose the
  configuration;
- **login-shell smoke behaviour**, which was already there and stays (§48.1);
- **the checksum manifest matches the file** — compared when a manifest is already beside the
  packages, and otherwise recorded.

### What was validated is recorded by digest

`scripts/package-check.sh` writes the SHA-256 of every package it installed to
`target/package-check.sha256`. Not into `dist/`: that directory is what two builds are compared
byte for byte (ADR-0527), and a record of the check is not an artifact of the build. #110 uses it
to prove the published asset is the artifact that was tested rather than a rebuild of it (§48.4,
§62.6).

## Consequences

Easy: the compatibility claim is measured. A .deb is proven on two distributions eleven glibc
releases apart. #110 has a record to compare against.

Hard: validation now pulls a third image and runs the `.deb` half twice, which roughly doubles
that part of the release check. That is what §48.3 asks for, and both images are pinned by digest
so neither can change under the check (§44.1).

Encoded by `xtask/tests/packaging.rs::should_run_every_new_package_check_the_specification_lists`
— each of the nine named beside the machinery that carries it out, so a comment cannot stand in
for a check — and `::should_run_package_validation_on_the_oldest_supported_baseline_as_well_as_a_
current_one`.

## Alternatives considered

**Use an older baseline than the build image — Rocky 9, glibc 2.34.** It would be a wider
compatibility claim and a false one: the binary is compiled against bookworm's glibc, so nothing
older is supported no matter which image the check runs in. §48.3 calls the baseline "the binding
compatibility proof", and a proof of something untrue is worse than a narrower true one.

**Add a second RPM distribution as well.** §48.3 asks for a floor and a current representative,
not for one of each per format. bookworm is the floor for both packages because it is the floor of
the binary they share, and Fedora is a current representative for the format that needs one.

**Check the build paths with `strings`.** Not in the base images; `grep -a` over the binary finds
the same thing with what is already there.

**Record the tested digests in `dist/`.** It would put a file describing the check next to the
artifacts the check is about, and then two builds of one commit would differ by it.
