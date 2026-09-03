# ADR-0560: A named version is reached by the command that goes that way

- Status: accepted
- Date: 2026-09-03
- Spec refs: §9.1, §17.1, §31.58, §50; ADR-0422
- Decided by: agent (autonomous)

## Context

ADR-0422 §3 gives `set package <name> --version <v>` one command per family. zypper is given
`--oldpackage` and moves in both directions with it. dnf and yum are given
`install -y <name>-<version>`, and **dnf refuses to install a version older than the one that is
installed**: the documented spelling for that direction is `downgrade`. So the one thing a
version option is most often wanted for on a Red Hat machine — putting a package back — could not
be done, and it surfaced as dnf's own message inside a `failed` row rather than as a wrong
outcome (issue #15).

Knowing which direction a version string points is not a string comparison. `1.0~rc1` is *before*
`1.0`; `1.0^git1` is after it; `10` is after `9` and after any word; every non-alphanumeric
character is a separator, so `1_0` and `1.0` are one version. That ordering is rpm's, it is
specified by rpm's own `lib/rpmvercmp.c`, and nothing else answers it correctly.

## Decision

**The provider knows rpm's ordering, and picks the command that goes the way the caller asked
for.** `version_compare` is `rpmvercmp`, transcribed: separators skipped, `~` before everything
including the end of the string, `^` after the end of the string and before anything else, a run
of digits compared as a number and beating a run of letters, leading zeros dropped.

`is_older(wanted, installed)` splits both sides into `epoch:version-release` and **compares only
the parts the caller wrote**. `--version 8.6.0` against an installed `8.6.0-8.fc40` means "8.6.0,
whatever the release"; comparing the missing release against `8.fc40` would call the installed one
newer and move a package nobody asked to move. An epoch outranks a version, as rpm has it, and is
compared only when both sides carry one.

On Red Hat, `set package --version` and `add package --version` run `downgrade -y
<name>-<version>` where the named version is older than what is installed, and `install -y` in
every other case. The description follows: "move `curl` back to 8.5.0-1.fc40 from 8.6.0-8.fc40",
so `--dry-run` says which direction it is going.

**Nothing about SUSE changes.** `--oldpackage` already does both directions in one command, which
is why this is a Red Hat gap and not a `set package` gap.

`act` reads the installed version *before* it builds the mutation rather than after, so the
reading that decides the direction and the reading the outcome's `changed` compares against are
one reading of one moment.

## Consequences

- `set package <name> --version <older>` on a Red Hat machine runs the command that can do it.
  The exit condition issue #15 states — `changed: true`, and rpm reporting the older version
  afterwards — is reachable; what this repository can prove without a Red Hat machine is the
  decision, and `crates/ono-provider-linux/src/packages_rpm.rs`'s tests hold both halves: rpm's
  ordering against the vectors rpm's own suite uses, and the command each direction produces.
- One `rpm -q` per package action, moved earlier rather than added: the same call that already
  measured `changed` now also decides the direction.
- The ordering is a transcription of a C function, and it is the kind of thing that is wrong in
  the cases nobody tried. So the test carries rpm's own vectors — the tilde, the caret, the
  alpha-versus-numeric rule, leading zeros, separators — rather than the three cases the feature
  needs.
- A version the caller wrote with a release is still compared on the release, so
  `--version 8.6.0-7.fc40` against `8.6.0-8.fc40` downgrades, which is what it says.

## Alternatives considered

- **Run `install` and fall back to `downgrade` when it fails.** No comparison to get wrong, and
  rejected: it runs a package manager twice, it reads dnf's human message to decide, and §50 and
  §31.58 forbid deciding anything by parsing a tool's prose.
- **Always run `downgrade`.** It refuses to go *up*, so it trades one direction for the other.
- **Shell out to `rpmdev-vercmp`.** It is in `rpmdevtools`, which a server does not have, and it
  would make the answer depend on a package the shell does not require.
- **Link `librpm` for `rpmVersionCompare`.** A C dependency, a build requirement on every
  platform including the ones with no rpm at all, for forty lines of documented algorithm.
