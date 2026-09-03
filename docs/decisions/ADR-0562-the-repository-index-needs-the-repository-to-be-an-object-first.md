# ADR-0562: The repository index needs the repository to be an object first

- Status: accepted
- Date: 2026-09-03
- Spec refs: §7, §7.1, §9.1, §11.5, §17.1, §40.1, §50; ADR-0115, ADR-0422
- Decided by: agent (autonomous)

## Context

`apt update`, `dnf makecache` and `zypper refresh` have no spelling in this shell. Spec §9.1 names
`get`, `find`, `add`, `remove` and `set package`, and no verb for the index, so `sudo apt update`
stays an external command on every distribution (issue #17). Both package providers have the
front end that would run it.

The issue's exit condition is "an `ono` spelling refreshes the index and reports what changed, or
an ADR records why the shell deliberately has none". **This ADR is neither.** The shell should
have the spelling; what this records is what the spelling costs, because it is more than it
looks and finding that out twice is waste.

## What was tried, and why each spelling is wrong

**Reuse a verb.** The registry of §7.1 is deliberately small and §7 asks a new command to reuse an
existing verb whenever the semantics match. None matches. `set` modifies properties of an object,
and the index is not a property of a package. `find package --refresh` reads well and is the worst
of them: `find` is a producer with `privilege: none`, and hiding a privileged mutation of local
state behind a read verb is precisely what a controlled vocabulary exists to prevent.

**Add the verb and hang it on `package`.** `refresh package` needs no new schema, and it does not
work: a mutation in this shell is an `Action` on an `ObjectId` (§11.5, §17.1 — "act on the object
you named"), and a refresh has no package to name. Giving it the identity of a package it is not
about, or a synthetic one, is the fabrication §35.3 forbids one layer down.

**Hang it on the provider.** `ono.provider/1` exists, and its own contract says "no producer
enumerates providers yet": there is no `provider` target, so `refresh provider linux.packages`
names an object nothing can produce or resolve.

## Decision

**The object of a refresh is the repository, and the repository has to exist first.** The work is,
in order:

1. `ono.repository/1` — id, name, source URL or path, enabled, and when its index was last
   refreshed — and a `repo` target. §7.1's `add` row already names `repo` among its typical
   targets, so the vocabulary anticipated it.
2. A producer for it in both package providers. **This is the part that is not small**: apt's
   repositories live in `/etc/apt/sources.list` and `/etc/apt/sources.list.d/*.{list,sources}` in
   two syntaxes, dnf's in `.repo` INI files, zypper's in `/etc/zypp/repos.d`. §50 and §31.58
   forbid parsing a tool's human output, and these are configuration files rather than output —
   but `apt-get indextargets` and `dnf repolist --json` are the machine interfaces to prefer where
   they exist, and deciding that per front end is the bulk of the tranche.
3. `refresh`, added to `docs/spec/verbs.yaml` under the §40.1 review this ADR is: *bring a local
   copy of remote metadata up to date*; targets `repo`; a mutation. It is a new word because
   there is no existing one, and it is `refresh` rather than `update` because `update` is what
   every package manager calls upgrading the packages themselves, and a word that means the other
   thing everywhere else is a word that will be typed by mistake.
4. `refresh repo` over all repositories or one, `apt-get update` / `dnf makecache` /
   `zypper --non-interactive refresh` behind it, one `ono.action-result/1` per repository, and
   `changed` decided by whether the index moved rather than by whether the command exited zero.
5. An acceptance case. The image has no network, so what it proves is that a refusal is a
   structured row rather than a hang — which is worth proving and is not the same as proving the
   refresh.

**So issue #17 stays open**, with this as its shape. Half of it — a verb with a target nothing
produces — would be worse than none: it would put a word in the vocabulary that answers
`resolve.target_not_found` on every machine.

## Consequences

- The next agent to pick #17 starts from the sequence above rather than from the four spellings
  that do not work.
- `refresh` is reserved by this ADR but not added: a verb in the registry with no command behind
  it is drift `spec-check` would rightly report.
- Making the repository an object is worth more than the refresh alone. `get repo` answers "where
  would this package come from", which is the question behind most reasons to refresh, and
  `ono.package/1` gaining a `repository` reference is what would let `find package` say where a
  hit lives.

## Alternatives considered

- **Declare that the shell deliberately has none**, which the issue offers. Rejected: there is no
  principled reason for it. The index is local state a user changes deliberately, the providers
  already run the front ends that change it, and every other package operation has a spelling.
- **`refresh package` with the target's provider as the implicit object.** It is the smallest
  thing that could work, and it makes `refresh package` mean "refresh the indexes of whichever
  provider answers `package`" — an action whose object is a provider, addressed through a target
  that is not it. Rejected: the same confusion `ProviderRegistry::act` was just fixed for
  (ADR-0559), reintroduced at the other end.
