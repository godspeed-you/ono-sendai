# ADR-0565: The package source is the object a refresh acts on

- Status: accepted
- Date: 2026-09-03
- Spec refs: §7, §7.1, §8.1, §9.1, §11.5, §16.5, §17.2, §31.58, §35.3, §40.1, §40.2; ADR-0115, ADR-0422, ADR-0559, ADR-0562
- Decided by: agent (autonomous)

## Context

Issue #17: `apt update`, `dnf makecache` and `zypper refresh` had no spelling. ADR-0562 found
that none of the existing verbs fits, that a refresh has no package to name, and that the object
of a refresh is the repository — so the repository has to be an object first. It laid out the
sequence and reserved the verb `refresh`. This ADR is that sequence, delivered, with the three
decisions ADR-0562 left open or got wrong.

## Decision

**1. The target is `package-source`, not `repo`.** ADR-0562 pointed at `repo` because §7.1's
`add` row names it. `docs/contracts/targets.yaml` already defines `repo` as *a source repository* —
the development target of §8.2, beside `branch`, `commit` and `worktree` — and a package
repository is a different object with a different provider, so reusing the word would have put
two objects behind one noun. `package-source` is what apt calls them (`sources.list`), the
compound form follows `host-key` and `client-key`, and it reads as the domain object §40.2 asks
for: `get package-source`, `refresh package-source`. It is the third entry of `targets.yaml`
marked as not in §8, in the system category, delivered in phase C with the rest of the package
family.

**2. `ono.package-source/1`** is `provider + id` — a machine can carry both databases, as
`ono.package/1` already knows — with `name`, `url`, `enabled` and `refreshed`, where `refreshed`
is read from the index file the manager keeps, never from its prose. The id is the manager's own
where it has one (dnf's and zypper's repository id) and, for apt, the repository root, suite and
component joined by `/`: `archive.ubuntu.com/ubuntu/noble/main`. What is read, per manager:

| manager | listing | refresh |
| --- | --- | --- |
| apt | `apt-get update --print-uris` (apt-get(8)), one line per index file an update would fetch, grouped by root, suite and component; the destination directory from `apt-config shell Dir::State::lists`; labels from `apt-get indextargets` where an index has been fetched | `apt-get update` |
| dnf, yum | the `.repo` files of `/etc/yum.repos.d` (dnf.conf(5)); `dnf repolist` is a table for people and is not read | `dnf -y makecache --refresh --disablerepo=* --enablerepo=<id>` |
| zypper | `zypper --xmlout lr` (zypper(8)) | `zypper --non-interactive refresh <id>` |

`indextargets` alone was the first choice and is not enough: it enumerates fetched indexes, so on
a machine that never ran `apt update` — the acceptance image, a fresh install — it lists nothing,
and a source is a source before its first update. `--print-uris` is apt's own answer to "what
would an update fetch", documented, and it answers unprivileged.

**3. `refresh`** is added to `docs/contracts/verbs.yaml` under the §40.1 review ADR-0562 wrote: no
existing verb means *bring a local copy of remote metadata up to date*; it is a mutation whose
targets are package sources; it composes as every mutation does — `get package-source | refresh
package-source` — and its inverse is none, because an index cannot be un-fetched. It is
`refresh` rather than `update` because `update` is what every package manager calls upgrading the
packages themselves.

**4. apt refreshes every source in one run, and the results of one pipeline share it.** apt has
no way to refresh one source. `get package-source | refresh package-source` asks for one result
per source, and running `apt-get update` once per source would hit the network once per source
for the same answer. So the first result makes the run, records the index times of every source
as they were before it, and the results that follow within five seconds of its finishing read
their own index against that record. The window is measured from the moment the run finished;
the actions of one pipeline follow each other within milliseconds, and a second pipeline typed
later runs it again. dnf and zypper refresh one repository natively, so nothing is shared there.

**5. `changed` is the index's to say.** A result is `changed` when the index file's modification
time moved — apt's list file, dnf's and zypper's `repomd.xml` — not when the command exited zero.
A first refresh of a dnf repository creates its cache directory, which is named after the
repository and looked for again afterwards; where the shell finds no cache at all, the index's
own word is that it was written.

**6. A refresh needs root and says so before it runs** (spec §17.2), exactly as the package
mutations do: one failed `ono.action-result/1` row with `E0302`, and `--dry-run` answers
`skipped` with the command it would have run.

## Consequences

- `sudo apt update` has a spelling on every distribution: `refresh package-source <id>`, or the
  pipeline over all of them. `get package-source` answers "where would this package come from",
  which is the question behind most reasons to refresh.
- Both package providers answer for two targets. A provider resolves a selector without being
  told the target, and the two objects are told apart by what the selector names — a package by
  `name`, a source by `id` or by its identity's schema.
- The acceptance image has no network, so case 210 proves what it can: the sources are listed
  from the real apt, the dry run names the command, the unprivileged refresh is a structured
  refusal rather than a hang, and `explain` shows the privilege first.
- ADR-0562's sequence is closed; its point 1 is corrected by decision 1 above.

## Alternatives considered

- **`repo` as the target**, as ADR-0562 proposed. Rejected: the word is taken by §8.2's source
  repository, and one noun for two objects is the confusion §8.4's design rule exists to avoid.
- **`refresh package-source` with no selector refreshing everything.** A mutation acts on what it
  names or on what is piped in (§11.5, ADR-0082); every other mutation refuses a bare spelling,
  and this one does too. The spelling for everything is the pipeline.
- **Running `apt-get update` once per apt result, with no shared run.** Honest and slow: a
  pipeline over six sources would fetch the same indexes six times. Rejected for the shared run
  of decision 4, whose window is short enough that a run it reuses is one the same pipeline made.
- **Reading `changed` from the manager's output.** `apt-get update` prints `Hit`/`Get` lines and
  dnf prints `Metadata cache created.` either way — prose, and §31.58 forbids parsing it.
