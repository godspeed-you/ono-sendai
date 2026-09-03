# ADR-0495: Two words for one edge reach the same place

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4 §6.4, §12, §14.3, §29.3, §32.1, §32.2, §35.2, §41.2, §44.2; v0.4.1 §34.2, §34.3,
  §53.1; ADR-0149, ADR-0421, ADR-0494; issues #25, #86
- Decided by: agent (autonomous)

## Context

Issue #25, reproduced at HEAD before this change:

```
$ ono -c 'enter 127.0.0.1:36173; follow owner'
ono: Ono-Sendai-E1009 spatial.unsupported the `owner` of this place is not answered here:
available on request

$ ono -c 'enter 127.0.0.1:36173; follow process'          # same edge, same end
$ echo $?
0
```

`owner` is the `inverse_label` of `process.owns_socket` and `process` is its `inverse_group`. They
are the same edge seen from the same end, and §6.4 lets `follow` take either word — the
declaration says so in a comment: *"Both words reach it: the label is what the edge is called, the
group is what a socket place prints."*

The cause is a string comparison. `Interest::wants` decided whether to ask a broad relationship
provider by testing whether the word the caller typed appeared in `labels_of(provider)`, and
`labels_of("linux.socket-owners")` is `["process"]` — the *group* words, because that is what a
refusal has to land on for a reader to recognise it (§35.2). `owner` is not in that list, so the
provider was skipped, `process` was recorded as declined, and the decline became
`Unknown / "available on request"`.

v0.4.1 §34.3 names that exact state and forbids it:

> If a relationship is described as "available on request", there MUST actually be a request path.
> v0.4.1 MUST resolve any state where `follow <relation>` refuses because the relation is
> expensive while another unrelated command can acquire it but the user has no way to request it
> through `follow`.

## Decision

**Words are resolved to relations before they are compared, and `follow` gains the flag §34.3
names for the relations that naming alone will not buy.**

### 1. `wants` compares relations, not spellings

`Interest::wants` now also resolves the caller's word and each of the provider's group words
against the subject's type — `relation::resolve_label`, which already accepts both label and group
— and asks the provider when any of them is the same relation. `follow owner` and `follow process`
therefore reach the same place, which is what §6.4 promised and what a test now asserts by
comparing the two spatial identities rather than the two spellings.

This is a `fix`: no vocabulary changed, no exit changed, and the only behaviour that changed is
that a word which always named the relation now also asks for it.

### 2. Naming the relation is the request path

§32.2 makes an exit nobody asked about a discoverable, unloaded one, and §32.1 forbids a default
`look` from spending a whole-target enumeration on it. That is why `look` prints
*"process — available on request"* at a socket, and that sentence is now true: `follow owner` is
the request. `look`'s own output is left saying it, because it is the honest thing for `look` to
say about an exit nothing has paid for.

### 3. `--resolve` is the path for a relation that even a named `follow` declines

Two different mechanisms produce "available on request", and only one of them is the decline above.
The other is `SpatialIndex::relation_summary`, which reports a `CostClass::Expensive` relation with
no members that way — `openers` on a file, which is every process on the host (ADR-0149). Naming
that relation is not enough, because the cost is the point.

So `follow <relation> --resolve` sets the interest complete, and §34.3's canonical spelling is the
one implemented: `follow owner --resolve`. It is declared in `docs/spec/commands/spatial.yaml` as
§34.3 requires — *"The exact flag MUST follow existing command option conventions and be recorded
in the command contract"* — beside `near --all`, which is the same idea for a neighbourhood.

### 4. The refusal that remains says which of the two it is

`follow` collapsed `Unknown` and `Unsupported` into one E1009 message. §35.2 keeps them apart —
one is "nobody has paid for it yet" and the other is "nothing serves it" — and §34.3 makes the
difference actionable, so they are now two messages. The `Unknown` one names `--resolve` and says
the relation is classified expensive or external (§34.2, ADR-0494). The code stays `E1009` in both
cases: §53.1 makes a stable code part of automation, and splitting one condition into two codes
because its message got better would break every script that catches it.

## Consequences

Easy: the README figure that opened issue #25 works, `near --type process` and `follow owner` no
longer disagree about whether a socket has an owner, and any future relation whose label and group
differ is covered by the same resolution rather than by a second entry in a table.

Hard: `wants` now resolves labels on every broad provider it considers, which is a linear scan of
the relation table per provider per observation. The table is thirty-odd rows and the scan happens
once per `look`, so it is nothing beside the enumeration it decides about — but it is work done to
decide not to do work, and if the table ever grows it should be indexed.

Also hard: `--resolve` is a flag on `follow` alone. `look` and `near` have `--all`, which is the
same permission with a different name, and a user has to know two words for one idea. Unifying them
would rename a v0.4 option that scripts already use, which §4.1's compatibility contract does not
allow inside a hardening release.

Encoded by
`crates/ono-cli/tests/spatial_relationships.rs::should_follow_the_owner_relation_when_it_is_requested_explicitly`,
`::should_follow_the_owner_relation_whether_or_not_a_look_came_first` (issue #25's own "with and
without a preceding `look`"), and
`::should_pay_for_an_expensive_relation_when_follow_is_asked_to_resolve_it`.

## Alternatives considered

**Add `owner` to `labels_of("linux.socket-owners")`.** One line, and it fixes this socket. It
leaves every other label/group pair to be discovered the same way, one issue at a time, and it
would put follow-labels into a list whose documented job is naming the exits a refusal lands on.

**Make `follow` translate the word to the group before building the interest.** It would work and
it puts the knowledge in the caller, so `near`, the interactive view and anything else that builds
an `Interest` would each need the same translation. `wants` is where the comparison happens, so it
is where the comparison is fixed.

**Give the `Unknown` refusal its own error code from the reserved range.** It is a different
condition and it would be cleaner in the taxonomy. §53.1's stability argument won: a script
catching `spatial.unsupported` for "this place will not tell me" would stop catching half of it.

**Drop the `--resolve` flag and let a named `follow` pay for everything.** `follow openers` on a
file would then walk every process on the host without the user having said anything beyond a
relation name, which is precisely the surprise §32.1 exists to prevent.
