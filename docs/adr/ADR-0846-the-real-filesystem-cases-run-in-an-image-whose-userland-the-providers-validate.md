# ADR-0846: The real-filesystem cases run in an image whose userland the providers validate

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §13, §14, §55.3, §55.4, §63.8, §63.9, §63.19, §66.9, Appendix G.3,
  Appendix G.4; v0.4.1 §38; ADR-0843
- Decided by: agent (autonomous)

## Context

ADR-0843 let the Btrfs cases build their own loop filesystem in a privileged container and left
the ZFS cases 289 and 318 with their declared skips. The second full container run showed that
privilege was not enough: cases 290, 291 and 319 built their filesystem and then found no Btrfs
candidate, because the acceptance image is Debian bookworm, whose btrfs-progs is 6.2, and the Btrfs
provider degrades to unsupported at any series it has not validated (Appendix G.4) — 6.16 and 6.17.
The ZFS provider validates OpenZFS 2.4.1, the module the host this suite runs on has loaded, and
Debian's `contrib` carries 2.1. `scripts/release-check.sh` admits no open box, and the §4.12 boxes
of the live pool, the live filesystem, §63.8, §63.9 and §63.19 have these cases as their proof. The
live suites `real_zfs.rs` and `real_btrfs.rs` pass in a privileged Ubuntu 26.04 container against
the same modules, whose btrfs-progs is 6.17.1 and whose `zfsutils-linux` is 2.4.1.

## Decision

1. `docker/Dockerfile` gains a stage `runtime-filesystems`, from a pinned `ubuntu:26.04`, carrying
   btrfs-progs and `zfsutils-linux`, the same `ono` binary the main image carries and the same
   unprivileged `case` user. It sits before the `runtime` stage, so the main image stays the
   file's last stage and every other build of the file is unchanged.
2. A case selects it with the directive `image: filesystems`. The harness builds that image only
   when a selected case names it, runs the case in it, and removes it afterwards unless
   `--keep-image` is given. Any other value of `image:` is refused as a malformed case.
3. Cases 289, 290, 291, 318 and 319 declare `image: filesystems`, `privileged: true` and
   `user: root`. The two ZFS rows leave `docs/contracts/hardening/expected_test_skips.yaml`,
   because in this environment the cases no longer skip, and the harness fails a declared skip
   that did not happen.

## Consequences

The Btrfs and ZFS boxes of §4.12 are proven by the container, against a real filesystem and a real
pool each case builds and destroys (Appendix G.3). The suite needs a host with the Btrfs module and
the OpenZFS 2.4.1 module loaded. On a host without them a case announces a skip the registry does
not declare and fails; that is deliberate, because a pass that built nothing would prove nothing
about the boxes it ticks. Whether the CI runner is such a host is recorded in `docs/STATE.md` under
*Found, not yet filed*.

## Alternatives considered

Debian's `contrib` or backports userland. Rejected: those are series the providers do not validate,
and a mismatched OpenZFS userland against the 2.4.1 module is unsupported by OpenZFS itself.
Widening the providers' validated series to Debian's. Rejected: Appendix G.4 makes validation a
statement that the semantics were tested, and nobody tested them. Moving the whole acceptance image
to Ubuntu. Rejected: it changes the environment of every other case to fix five. Keeping the skips
as recorded exclusions. Rejected: the release gate admits no open box, and the environment the
boxes need is available here.
