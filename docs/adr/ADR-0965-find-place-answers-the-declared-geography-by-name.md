# ADR-0965: `find place` answers the declared geography by name

- Status: accepted
- Date: 2026-11-09
- Spec refs: v0.4 §4.1, §6.8, §9.3, §19.2, §27.1; ADR-0124, ADR-0140
- Decided by: agent (autonomous)

## Context

`find place <query>` searched the index of observed objects, and a canonical space — `compute`,
`storage.devices`, `network/sockets` — is declared geography, not an observed object, so
`find place compute` answered `[]` (#205). The issue asked whether the search should answer the
domain at all; nobody had decided.

§6.8 describes discovery "without prior names": the user does not know what is there and asks.
Where things are kept is part of what is there, and a user who types `find place compute` or
`find place services` means the place by that name, which the geography declares and `enter`
already resolves by the same spelling (§27.1's canonical identifier). An empty answer about a
place the shell itself declares is a false negative.

## Decision

**A named `find place` search also answers the canonical spaces whose id, label or last id
segment match the text**, ranked with the observed objects by the same rule — a pin first, an
exact name before a partial one, a place under the anchor before a distant one — and rendered as
the same `ono.spatial-place/1` record `look` and `near` emit (ADR-0140), so a found space is a
place `enter` and `jump` take.

The declared geography takes part only where a name was given. A search with no text enumerates
observed objects, as before: there is nothing to match a space against, and listing the whole
geography beside every object would make `find place --type process` and its kin answer about
places nobody asked for.

The other filters keep their meaning. `--where` evaluates a predicate against providers' records
and its subjects are objects, so a space is never among them; a space carries no role; `--type`
is honoured — the compute domain is a `Compute` place, not a process — and `--near` bounds a
space by its place path as it bounds an object.

A canonical space is a place of some host (§19.2), so the search needs a scope to name it in.
The session's own host comes through `FindRequest::in_scope`; a remote session standing on a
linked host uses that host's scope (`space::standing_in`) as resolution does. A library caller
giving neither gets observed objects only — the geography is not invented for a host nobody
named.

A found space is `Freshness::Live` with the spatial layer's own provenance (`ono.spatial`),
because the geography is declared by the layer and is as current as the build (§4.1); it is not
observed and no provider asserted it.

## Consequences

Easy: `find place compute`, `find place services`, `find place compute.services` answer the
place; `find place compute | take 1 | enter` works.

Hard: a name a space and an object share — a process named `compute`, a unit `services` — now
answers both, ranked by match quality and then by name; §27.1 prefers local orientation, and the
observed object under the anchor still answers first.

Encoded by `crates/ono-cli/tests/spatial_navigation.rs::should_answer_a_canonical_space_when_find_place_names_it`
and acceptance case 090 proof 44.1f2.

## Alternatives considered

**Leave `find place` to observed objects, and document it.** A search that cannot find the
compute domain while `enter compute` resolves it is two answers to one spelling (§27.1).

**Answer spaces for a textless search too.** It would put the whole geography into every
unfiltered `find place` and change what `--type` filters answer; the spaces are already listed
by `look` at the root and by `map`.

**A separate `find domain`.** A second verb for the same question, which §6.8's one search is
there to avoid.
