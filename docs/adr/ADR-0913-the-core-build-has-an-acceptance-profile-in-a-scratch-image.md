# ADR-0913: The core build has an acceptance profile, run in a `FROM scratch` image

- Status: accepted
- Date: 2026-09-24
- Spec refs: AGENTS.md §10, §14; docs/ACCEPTANCE.md §2; v0.4.1 §44.1; ADR-0851, ADR-0846,
  ADR-0910, ADR-0911, ADR-0912
- Issues: #127
- Decided by: agent (autonomous)

## Context

The exit test of #127 requires the acceptance cases tagged for the core profile to pass against
the core binary in a `FROM scratch` image. The harness runs every case as
`bash -lc 'eval "$ONO_CASE_SCRIPT"'` inside the image, as the unprivileged user `case` whose login
shell is `ono`. Under `pty: true` it runs through `script(1)`. A scratch image has no bash, no
`script` and no userland at all.

A case is a shell script that observes `ono` from outside: it runs `ono`, pipes its output
through `grep` or `head`, and creates files. The binary under test has to run in the scratch
image. The harness may run beside it.

## Decision

1. **Profiles are a case directive.** `profile: full` and `profile: core` may be repeated. A case
   runs in each profile it names, and in `full` alone when it names none, so no existing case
   changes. The directive belongs to the case file, like `image:` and `privileged:`, and a
   profile name other than those two fails the run.

2. **`scripts/acceptance.sh --profile core`.**
   - The harness runs the cases that name `core`, from whatever selection was made: fragments,
     `--group`.
   - It builds the core image instead of the full one. `scripts/build-core.sh --stage` runs, and
     then `docker/core/Dockerfile` is built at its `core-acceptance` target.
   - Its tag is `${ONO_ACCEPTANCE_CORE_IMAGE:-${IMAGE}-core}`, named after the full image the way
     the filesystem image of ADR-0846 is.
   - `--build-only`, `--no-build` and `--keep-image` mean what they mean for the full profile.
   - A selection with no case left in the requested profile says so and exits 0. `--group
     verification` in the full profile is an example: that group holds core-only cases. A group
     is a range of numbers, not a profile.
   - The full profile's default image tag and its build are untouched, and so is every existing
     path through the script.

3. **The image.** `docker/core/Dockerfile` has two stages.
   - `core` is the deliverable, built `FROM scratch`. It contains:
     - the static binary at `/usr/local/bin/ono`;
     - `/etc/passwd` and `/etc/group` for `root` and `case`, whose login shell is `ono`;
     - `/home/case`, owned by `case`;
     - `/tmp` with mode 1777.

     It has no loader, no libc and no shell. `ono` is the only program in the image.
   - `core-acceptance` is `core` plus the harness's own interpreter: a static busybox
     (`busybox:1.37.0-musl`, pinned by digest, v0.4.1 §44.1) under `/opt/harness/bin`. The
     harness runs a case as `/opt/harness/bin/sh -c 'eval "$ONO_CASE_SCRIPT"'` with
     `PATH=/usr/local/bin:/opt/harness/bin`. Busybox is the harness beside `ono`, never a
     substitute for it: nothing of it is on a path `ono` was installed on, and case 355 proves
     that the image has no loader, no libc and no `/bin/sh`.
   - The build context is a staging directory `scripts/build-core.sh` lays out, never the
     repository. The image contains exactly what that directory names, with the modes it gives
     them.

4. **What a core case may use.** A core case is a POSIX shell script for busybox `sh`, not bash.
   It uses busybox's tools, not Debian's, and it cannot use `pty:`, `image: filesystems`,
   `privileged:`, `capability:` or `user:` in the core profile. The harness refuses the first
   two, and the rest have not been needed. The terminal cases stay full-only.

5. **Which cases the core profile claims.**
   - The existing cases that exercise the object shell alone, and pass unchanged in both images,
     are tagged `profile: full` and `profile: core`: 000, 001, 002, 021, 022, 023,
     024-quoting-and-expansion, 027, 028, 033, 035, 036, 037, 040-object-pipeline, 041-typed-values,
     042-inspection,
     043-discoverable, 045-context-and-reuse, 048, 120, 153 and 193.
   - Four other candidates were run in the core image and left full-only, because each needs
     something of the Debian image rather than of `ono`:
     - 020 runs `/bin/echo`.
     - 030 needs `ps` behaviour busybox does not have.
     - 032 expects `ls` at `/usr/bin/ls`.
     - 151 needs `perl`.
   - Five new cases cover the core's own behaviour. They take numbers 355–359 of the new
     `verification` group (350–369):
     - 355: one static binary, no loader or libc, `ono` the login shell, `--version` names the
       profile.
     - 356: every compiled-out tier refuses by name with `resolve.not_in_build` and status 126.
     - 357: `get service` refuses as `provider.unavailable`. It runs in both profiles, which
       proves the exit test's "same error a full build gives on a host without systemd".
     - 358: help, the registry and the settings list only what the build carries.
     - 359: a mutation acts through its provider without a change plan.

6. **CI.**
   - A `core-build` job in `.github/workflows/ci.yml` runs on every push. It lints the core
     profile (`cargo clippy -p ono-cli --no-default-features --features core --all-targets -D
     warnings`), which compiles every test target in that configuration. It runs the core suites
     (`crates/ono-cli/tests/core_build.rs`, and `ono-provider-linux`'s storage tests without
     systemd). It builds the static binary against its budget (ADR-0912) and runs `--profile
     core`.
   - `xtask spec-check`'s image-digest rule accepts `FROM scratch`: it is Docker's name for no
     image, so nothing is pulled and there is nothing to pin.
   - The integration test files of `ono-cli` that exercise a compiled-out tier carry
     `#![cfg(feature = "<tier>")]`, so the core configuration compiles them away rather than
     failing on them. The full build compiles and runs every one of them, as before.

## Consequences

- The exit test is a CI job, not a claim: static, under 8 MB, 27 core cases green in a scratch
  image.
- A case added to the core profile has to be POSIX and busybox-clean. The harness does not
  translate bash into sh.
- The core profile proves no terminal behaviour. The line editor and job control in a
  pseudo-terminal are proven by the full profile's cases only. Busybox has a `script` applet, but
  its `--return` semantics differ from util-linux's, and proving the harness on it is left for
  the day a core terminal case is needed.

## Alternatives considered

**Running the harness in a sidecar container sharing the scratch container's namespaces.** This
was rejected: a case drives `ono` through its argv, stdin and files, so the harness would need
the scratch container's filesystem as well as its PID namespace. Mounting a static shell beside
the binary is the same separation with less machinery.

**Putting busybox on the image's own `/bin`.** This was rejected: the image would stop being a
test of the scratch deliverable, and a case could pass because busybox answered where `ono`
should have.

**A separate case directory for the core profile.** This was rejected: a case that holds in both
builds would have to be written twice, and case 357 exists to be one case in two images.
