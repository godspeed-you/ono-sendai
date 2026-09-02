# ADR-0422: The rpm database is one provider with three front ends

- Status: superseded by ADR-0465 (in part: the sentence about a plugin running sandboxed)
- Date: 2026-08-31
- Spec refs: §9.1 (Package), §16.5, §17.2, §27, §31.58, §35.3, §43, §50; AGENTS.md §4, §6;
  ADR-0006, ADR-0068, ADR-0112, ADR-0115
- Decided by: agent (autonomous)

## Context

ADR-0115 delivered `get/find/add/remove/set package` for dpkg and apt and said so about
everything else: where no `dpkg-query` is on `PATH` the provider reported itself unavailable with
"rpm-based systems are not served by this build", and `docs/STATE.md` carried the rpm/dnf
provider as open. On Fedora, RHEL, their rebuilds, openSUSE and SLES the whole package family
therefore answered E0401 — an honest refusal, and still a shell that cannot say what is
installed on half of the Linux machines in the world.

Nothing in the spec or in the contracts fixes how a second package manager arrives. `§9.1` names
the commands and not the managers; `docs/spec/providers/` allows any number of providers per
target; `ProviderRegistry::provider_for` answers with the first provider that is *available*, and
`registry.rs` already documents that a later registration "extends rather than replaces".

## Decision

### 1. One provider per package database, not per distribution

`crates/ono-provider-linux/src/packages_rpm.rs` is `linux.packages.rpm`, registered after
`linux.packages` and claiming the same `package` target. Which one answers is the registry's
existing rule and needs no new mechanism: on a Debian machine dpkg is available and rpm is not,
on a Red Hat or SUSE machine the reverse, and where neither database is present each provider
says what it looked for, so `get package` names both `dpkg` and `rpm` in one refusal.

The split is by *database*, not by distribution, because that is what a package identity belongs
to. `ono.package/1` is identified by `provider + name` (ADR-0115), and the `provider` field of
every record this provider makes is `rpm` on Red Hat and on SUSE alike: it is one database, one
namespace of names, one answer to "is curl installed". A provider per distribution would have
put the same object under two identities.

### 2. The front end is discovered, and zypper decides

The database is asked with `rpm`; the repositories and every change go through whichever of
`zypper`, `dnf` and `yum` is on `PATH`, looked up in that order and never at an absolute path.

zypper comes first deliberately. Fedora and RHEL do not ship zypper, while dnf installs
anywhere, so a machine that carries zypper is a SUSE machine whatever else is on it; the reverse
inference does not hold. `yum` is last because on every supported Red Hat release it is the
compatibility name for dnf and takes the same subcommands — where it is the old Python 2 yum
instead, its own message reaches the user as the outcome of the action rather than a guess made
here.

`rpm` alone is enough for the provider to be **available**: the listing is what an operator asks
for most, and it is complete without a front end. `find package` and every mutation then refuse
with `provider.unsupported` (E0402) naming the three programs that were looked for. That is the
same shape ADR-0115 gave a dpkg system with no `apt-cache`.

### 3. The machine formats, and nothing else (spec §31.58, AGENTS.md §6)

- `rpm -qa --queryformat '%{NAME}\t%{VERSION}-%{RELEASE}\n'` for the installed set, and
  `rpm -q --queryformat … <name>` for one package. Everything the database answers for is
  installed, so `installed` is `true` for every record it produces.
- `rpm -q <name>` exits non-zero for a package that is not installed and says so on *stdout*.
  That is rpm's documented answer to the question and is read here as an empty result — but only
  when nothing reached stderr, because a database that failed to open is not an absence.
- `dnf repoquery --queryformat '%{name}\t%{summary}\n' '*term*'` for the repositories. dnf's own
  `search` prints a human report with match-quality headings and is never read.
- `zypper --xmlout --non-interactive --no-refresh search --type package <term>`: `--xmlout` is
  the machine interface zypper(8) documents, and the `solvable` elements are the answer. A
  `solvable` of kind `srcpackage` is a source package and not a package. Exit status 104 is
  zypper's "nothing matched" and is an empty result, not a failure.
- `dnf install -y` / `dnf remove -y` / `dnf versionlock add|delete`, and
  `zypper --non-interactive install|remove|addlock|removelock`. Both are given their documented
  non-interactive mode on every invocation: a package manager waiting for an answer on a pipe
  nobody is behind is a hung shell.

A listing outside the declared format is `provider.schema_violation` (E0403) for the whole
answer, and no record is made from it — including a zypper document with no `stream` element,
which is what a human-readable table would be.

### 4. One name is one object

`rpm -qa` lists a package once per installed architecture, and `dnf repoquery` once per version
and architecture the repositories carry. Both are deduplicated by name, keeping the first line
the manager printed, because `provider + name` is the identity: emitting both would put two
objects with one identity into the pipeline, and `remove package` would then act on it twice.
The architecture is not a field of `ono.package/1` and is not invented into one (spec §35.3).

### 5. Elevation is refused before the manager runs, as it is for dpkg

Unchanged from ADR-0115 §5: the uid decides before anything is started, the refusal is one
`failed` row carrying `io.permission_denied` (E0302), and `changed` is decided by comparing what
rpm reports as the installed version before and after — never by reading a front end's prose.

### 6. `--purge` is refused rather than silently downgraded

rpm has no purge. A removal leaves a configuration file the administrator changed behind as
`.rpmsave`, and there is no manager invocation that does what `apt-get purge` does. `remove
package <name> --purge true` is therefore `provider.unsupported` (E0402) on an rpm system.
Accepting the flag and performing an ordinary removal would be a lie about what happened.

### 7. `explain` names the provider that would answer

`explain` reported the first provider *claiming* a target. With two providers claiming `package`
that is a plan for a different machine, so it now reports the first *available* one and falls
back to the first claiming one when none can answer. No existing behaviour changes: until this
ADR every target was claimed by exactly one provider, and the two rules agreed.

### 8. `quick-xml` joins the workspace

zypper's only documented machine format is XML, so the workspace gains one XML reader
(`quick-xml`, ADR-0005's rules apply: it is small, has one optional dependency, and is used in
exactly one module). Reading `--xmlout` is exactly what AGENTS.md §6 asks for: XML is the format
zypper declares for programs, as `--queryformat` is rpm's.

## Consequences

- On Red Hat and SUSE machines `get`, `find`, `add`, `remove` and `set package` answer;
  everywhere else the two package providers together name both databases in one refusal.
- The records of both providers share `ono.package/1`, so a pipeline written against one works
  against the other, and `explain` names which one will run.
- Tests: `crates/ono-cli/tests/packages_rpm.rs` — fifteen outcome tests over fake managers on a
  scratch `PATH` (listing, one-by-name, absence, per-architecture duplicates, E0403 on garbage,
  dnf search, yum search, zypper search, zypper winning over dnf, the front-end-less refusal
  with the listing still working, E0302 on add and remove, E0402 on purge and with no front end,
  and `explain` naming `linux.packages.rpm`); seven unit tests over the three decoders in
  `packages_rpm.rs`; the generated `provider_conformance.rs` surface and schema cases; and the
  acceptance case `docker/acceptance/cases/046-rpm-packages.case`, which drives all of it
  through the real binary on three faked machines and then proves dpkg still answers on the
  Debian container.
- What is left open, and is recorded under *Next up* in `docs/STATE.md`: a `set package
  --version` that moves *backwards* is dnf's refusal rather than a downgrade (zypper is given
  `--oldpackage` and does it); a record from one package provider piped into a mutation is
  routed to the first available provider rather than to the one its identity names; and there is
  still no command for refreshing a repository index (`apt update`, `dnf makecache`), which
  §9.1 does not name either.

## Alternatives considered

- **A provider per distribution family** (`linux.packages.dnf`, `linux.packages.zypper`).
  Rejected: it puts one object under two identities, and forces each provider to require its own
  front end for availability, so a machine with rpm and no front end would report "no package
  manager" while holding a complete database.
- **A KUANG/11 plugin.** Rejected, and not possible today: `provider.query` is dispatched only
  for targets the package itself contributes (`ono-kuang-supervisor/src/supervisor.rs`), there
  is no host→plugin call for an action at all, and package mutations need root while a plugin
  runs sandboxed under the shell's uid. A plugin could only have contributed a second vocabulary
  beside `add package`, which is what the provider registry exists to avoid.
- **Parsing `dnf search` or `zypper search`.** Rejected by AGENTS.md §6: both are human reports
  whose columns and headings change with the terminal and the locale.
- **Deciding the front end from `/etc/os-release`.** Rejected: what can answer is what is
  installed, and `PATH` is the answer the machine itself gives (ADR-0115).
