# ADR-0582: A contributed target is queried; a contributed command is invoked

- Status: accepted
- Date: 2026-09-05
- Spec refs: §31.22, §31.23, §31.64, §31.68, §31.80; `docs/contracts/kuang/contributions.v1.yaml`
- Decided by: agent (autonomous)

## Context

A KUANG/11 package can declare two different things it offers. §31.22 is the command
contribution — a verb the user types, invoked through `command.invoke`. §31.23 is the target
contribution — a *noun* the package answers for, queried through `provider.query`, with a schema
the package declares and provenance the host stamps.

Only the first half was wired. `contributions.commands` documents became registry placeholders,
and `provider.query` was protocol-complete, conformance-tested and reachable from
`LoadedPlugin::query` — with no call site anywhere in the shell. `ContributionPaths::targets`
existed as a manifest field that nothing read.

The gap had a workaround that looked like a solution: a package could declare a command with
`verb: get` and `target: pod`, and `get pod` would resolve and run. `crates/ono-cli/tests/plugins.rs`
already fixtures exactly that. But what answers is a command, and it answers whatever it likes —
the example package's `get echo-item` returned a stream of bare integers. There is no declared
schema on the records, no identity, no host-stamped provenance, and nothing the spatial or
provider machinery can attach to. It is a command wearing a target's spelling.

That distinction is not academic for the direction the project has taken. The Kubernetes provider
specification requires resources to be enterable places with relationships, evidence and identity
(§16, §23, §35). None of that can hang off a command.

## Decision

The two contributions are declared separately, registered separately, and routed differently.

- `contributions.targets` documents are read at registry-build time, exactly as command documents
  are (§31.68). Each target becomes a `get <target>` registry entry attributed to the package, so
  the noun is typeable — with help, completion and `explain` — before any package code runs.
- The synthesised entry's id is `<package.id>.target.<name>`, beside the existing
  `<package.id>.command.<kebab>`. §31.5 requires a contributed id to be namespaced under the
  package; the infix says how the entry is routed. Keeping it in the id means `get command`,
  `help` and `explain` show the distinction rather than hiding it behind a heuristic on the verb.
- Invocation branches on that infix. A `.command.` entry is invoked; a `.target.` entry reaches
  `provider.query`, so the records that come back carry the schema the target declared and the
  provenance the host stamps (§31.80).
- A package that declares a target on disk and does not answer for it at the handshake is
  refused with `resolve.target_not_found` naming the disagreement, rather than falling back to
  a command of a similar name.

`ContributedCommand::into_contract` now admits both infixes. Its previous rule — that a
contributed id must be `<package.id>.command.<kebab>` — was the narrower reading of §31.5, and it
is what refused the first working version of this change.

## Consequences

Easy: a provider package contributes nouns. `get echo-item` answers with records that carry
`dev.example.echo.item/1`, an identity of `{seq}` and provenance `plugin:dev.example.echo` — the
same shape a built-in provider's records have, which is what lets everything downstream treat
them alike. A Kubernetes package can now contribute `pod`, `node` and `service` as targets
rather than as commands that happen to be spelled that way.

Hard: two kinds of entry live in one registry and are told apart by an id infix. That is a
convention, and a convention is weaker than a type. It is written into the id rather than a side
table so that it survives being printed, and `into_contract` is the one place that admits it.

Watch: this wires the *read* path only. A contributed target reaches `provider.query` and no
further — `ProviderRegistry` still holds no entry for it, so `resolve`, `subscribe` and `act`,
and with them the spatial model's `enter`, `near` and `follow`, do not yet see contributed
targets. That is the next increment and it is what the Kubernetes provider's §35 needs. Nothing
here should be read as claiming a package is a first-class provider yet; it can answer `get`.

Also watch: the query is issued with no options. Selectors, `where` pushdown and pagination are
applied by the pipeline after the fact, which is correct but not always cheap, and the generic
provider contract's §12.2 wants pushdown where semantics are preserved. That needs the options
half of `provider.query`, which is a separate increment.

## Alternatives considered

**Route on the verb: any contributed `get` becomes a query.** Rejected: it silently changes the
meaning of every existing contributed `get`-shaped command, including the two already fixtured in
the test suite, and it leaves a package no way to contribute a `get` command deliberately.

**A side table mapping contract ids to routes.** Rejected: the registry entry is what `explain`,
`help` and `get command` print, and a routing decision that is invisible there is one a user
cannot discover. Keeping it in the id costs a convention and buys visibility.

**Synthesize the target entry from the handshake instead of from disk.** Rejected: it would make
`get pod` unresolvable until something else caused the package to load, which inverts §31.68's
`installed manifest -> registry placeholders -> first invocation -> runtime load` and would mean
a user has to know which package to load before they can ask a question.

**Implement the full `Provider` wrapper first.** Rejected as an increment, not as a direction: it
is the next step, and doing it in one commit with this one would have mixed the routing decision
with the registry integration and left neither separately reviewable.
