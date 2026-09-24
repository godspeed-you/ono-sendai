# ADR-0902: An RPM member carries the epoch, whatever age its source file has

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §46.1, §46.2, §46.4, §46.5; ADR-0526, ADR-0527; issue #146
- Decided by: agent (autonomous)

## Context

`xtask/tests/packaging.rs::should_produce_identical_hashes_for_two_clean_builds_of_one_commit`
once produced two RPMs of one commit that differed by 18 bytes, while the `.deb` came out
identical. The cause was not known. It is now:

cargo-generate-rpm 0.21.0 passes `SOURCE_DATE_EPOCH` to the `rpm` crate (0.23.2). The crate
**clamps** each file's mtime to that value rather than setting it (`builder.rs`:
`Some(d) if d < entry.modified_at => d, _ => entry.modified_at`). A source file younger than
the commit gets the epoch. A file older than the commit keeps its own mtime in
`RPMTAG_FILEMTIMES` and in its cpio header. A tree checked out before its HEAD commit was made
has such files: that is the usual state of a workstation, where most files a release ships are
not the ones the last commit touched. cargo-deb sets every member to the epoch, which is why the
`.deb` never differed.

This was reproduced directly: one RPM built, `README.md` and `docs/reference/commands.md`
touched to 2020-01-01, a second RPM built. The size moved by a byte, and `rpm -qp --dump`
(run in `fedora:latest`, since the host has no `rpm`) showed exactly those two files with
different mtimes:

```text
< /usr/share/doc/ono/README.md 27072 1789473202 …
> /usr/share/doc/ono/README.md 27072 1577836800 …
```

The comparison in ADR-0527 could not see this. Both of its builds packaged one shared tree, so
the two builds only differed if a file's mtime changed in the minutes between them. On
2026-09-09 several processes were working in the same checkouts, and that is the most likely
explanation for the one observation. It was not reproduced in that form.

## Decision

1. **`scripts/package.sh` builds the RPM from a staged copy whose every mtime is the epoch.** It
   copies what git would track, read through the ignore files with
   `tar --exclude-vcs --exclude-vcs-ignores` so no repository is required, plus the binary, into
   a temporary directory. It then runs `touch -h -d @$SOURCE_DATE_EPOCH` over the copy and runs
   cargo-generate-rpm there. Since every mtime equals the epoch, clamping gives the epoch
   whatever the checkout's history. The `.deb` path is unchanged.
2. **`scripts/rebuild-check.sh` gives each build its own checkout, and the two checkouts differ
   in file age.** Build `a` packages a fresh copy, where every file is younger than the commit,
   like a runner's clone. Build `b` packages a copy dated one day before the commit, like a
   workstation's tree. This adds the checkout's file ages to ADR-0527's list of things a build
   may see and must not embed.

## Consequences

- The existing reproducibility test now covers the failure it once hit by chance. With decision
  2 alone, `rebuild-check.sh` fails every time with the RPM `header`, `signature header` and
  `payload` differing. With decision 1 added, it passes.
- `package.sh` copies about 34 MB more for each RPM build and needs GNU `tar` and `touch`,
  which the Linux release hosts already have.
- The mtime of a packaged binary is pinned too. It was already younger than the commit in every
  real build, but now that is guaranteed rather than assumed.

## Alternatives considered

- **Touch the checkout's own files up to the epoch.** This rewrites the mtimes of the user's
  working tree, which cargo and editors read, to fix one tool.
- **Stage only the files the manifest's `assets` name.** That means parsing the manifest in
  bash and a second list to keep in step with it. A glob that matched nothing would drop an
  asset silently rather than fail.
- **Pass an older `--source-date`.** Clamping would then give each file the older of the two
  times, so the checkout's history would still decide the result.
