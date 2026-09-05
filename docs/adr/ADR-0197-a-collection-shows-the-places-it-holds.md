# ADR-0197: A canonical collection shows the places it holds, whatever observed them

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.17, §7.2, §35.2, §37.1, §37.2, §42.4, §53; ADR-0140, ADR-0143, ADR-0193
- Decided by: agent (autonomous), delivering v0.4 §50 Phase S11

## Context

ADR-0193 settled how an adapted object becomes a place: the spatial layer never runs a tool, so
an adapted observation exists only where a user's own command line produced one, and §37.1's
second sentence keeps it — "the adapted record is all there is and it appears with its own
provenance".

The canonical *collections* did not honour that. `observe_space` asked the providers that serve a
space and built each exit from their answers alone. Where a provider refused, or where the target
has no provider at all, the exit was `unsupported` with a reason — even when the index was holding
places whose canonical parent is exactly that space. So

```text
ip addr | count | to text
home; enter network; enter addresses
```

reported `addresses  unsupported  no provider answers for addresses` while `find place` found
twenty-three addresses and `enter` reached each of them, and

```text
systemctl list-units | count | to text
home; enter compute; enter services
```

did the same for a host whose service manager only an adapter can read — the situation
`docker/acceptance/cases/091-spatial-unknown-web-service.case` puts the shell in.

That is the claim §2.17 forbids. `unsupported` means "nobody could answer for this"; a group that
can name its members has an answer.

## Decision

**An exit's members are the union of what the providers answered and what the index already holds
for that space.** `observe_space` builds one parent → members map from the session index and
`exit_of` reads it:

- a target with no provider at all, or one whose providers refused, is `unsupported` /
  `permission_denied` only while the index holds nothing for the space — the refusal may still
  replace a count nobody could take (§35.2, §42.4), never one the shell can produce;
- where the index holds members, the exit is open and lists them;
- where a provider also answered, the provider's places come first and the index adds only what
  the provider did not report, so nothing is listed twice and the provider keeps the ranking.

A place's space is its canonical parent as `resolve::parent_of` computes it — the stored
hierarchical edge where there is one, the derived parent otherwise — so this reads the same
hierarchy `up` walks (§11.3) and invents none of its own.

**A tombstone is not a member.** An entry whose lifetime has ended stays in the index as the
record §10.3 requires and is not offered as something to enter.

This adds no source of truth: everything listed was observed by a provider or by an adapter and
carries that provenance (§2.16, §37.2). Raw text still never becomes a place.

## Consequences

- The six domains and their collections are honest on a host where the canonical provider for a
  target is missing: SERVICES on a container without systemd shows the units the operator's own
  `systemctl` reported, and says `unsupported` only while nothing has reported any.
- `look` costs one pass over the session index per view. It is built once per `observe_space`
  call rather than once per exit, so the cost is linear in the index and inside the §34 budget;
  `should_answer_repeated_looks_far_inside_the_look_budget` still passes.
- Encoded by `spatial_contracts_missing::should_show_a_place_only_an_adapter_observed_when_standing_in_the_collection_that_holds_it`
  and `::should_not_call_a_collection_unsupported_while_it_holds_a_place_the_shell_observed`,
  and by `docker/acceptance/cases/091-spatial-unknown-web-service.case`.

## Alternatives considered

- **Have the spatial layer run the adapter when its provider refuses.** Rejected: ADR-0193 fixed
  that the spatial layer never runs a tool, and §2.16 keeps facts with the providers. A shell that
  shells out to `systemctl` behind a `look` is the text-scraping provider AGENTS.md §6 forbids.
- **Show the held places only when the provider refused.** Rejected: an adapter can observe an
  object no canonical provider serves on a host where the provider answers for others — §37.1's
  "both objects may appear" is not conditional on total failure.
- **Report a distinct state, such as `partial`, for a collection assembled this way.** Rejected:
  §35.2's six states are the vocabulary, `partial` is not one of them, and the neighborhood
  already carries `completeness` for the question of whether everything was shown.
