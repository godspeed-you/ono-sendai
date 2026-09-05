# ADR-0193: An adapter observes; it does not name

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §37, §37.1, §37.2 (integration with the v0.3 adapters), §10.1, §10.2 (identity
  tiers, process identity), §42.1 (one object, one place), §33.1, §33.4 (the index and what
  `inspect` reveals), §2.16 (providers own facts), §29.4, §34 (streams and budgets);
  v0.3 §1.47 (adapted provenance), ADR-0057
- Decided by: agent (autonomous), delivering v0.4 §50 Phase S10

## Context

v0.3 turns `ip link`, `ps`, `lsblk` and the rest into typed canonical objects. v0.4 §37 lets those
objects contribute to the spatial model and §37.1 sets the condition: "Objects from adapters MUST
be reconciled with canonical provider identities before appearing as duplicate map nodes."

Two facts make that harder than a schema comparison:

- **An adapter carries what its tool prints.** `ip link` prints an interface's index, which *is*
  `ono.interface/1`'s identity, so the adapted `lo` already reduces to the same [`SpatialId`] the
  netlink provider's `lo` reduces to. `ps` prints a start time rounded to the second and no pid
  namespace, where §10.2 composes a process's identity from the boot, the pid, the start time and
  the namespace — so the adapted process reduces to a *different* identity for the same process.
- **Nothing indexed an adapter's answer at all.** The spatial layer asks its providers when it
  needs them; it never runs a tool. An adapted observation exists only where a user's own command
  line produced it, so the shell had no record that `ip link` had ever agreed about `lo`, and
  §37.1's "keeps both sources" had nothing to keep them on.

## Decision

1. **An adapted record never mints a spatial identity the canonical provider would not.** Where an
   adapted object is about to become a place through `enter`, the shell asks the provider that
   serves the target for the same object, by the handle the `enter` contract names it with, and
   the provider's record is the place (`context::canonical_twin`). `ps … | enter process` therefore
   stands in exactly the place `enter process <pid>` stands in. Where no provider answers, the
   adapted record is all there is and it appears with its own provenance — §37.1's second sentence.
2. **A place keeps every source that observed it.** `IndexEntry` accumulates the provider of every
   observation, and `ono.spatial-place/1` and `ono.spatial-neighbor/1` carry them as `sources`. So
   `inspect` on `lo` shows `linux.netlink` *and*
   `adapter:org.ono.compat.iproute2.ip-link`, which is what §37.1's "both objects may appear with
   provenance" asks for once the two have reconciled into one place (§33.4).
3. **An adapter observes; it does not own.** A canonical provider's record always replaces the
   record an entry holds. An adapted observation of a place a provider has already described
   refreshes it and adds itself to the sources — §2.16 keeps the facts with the provider.
4. **A batch an adapter decoded is offered to the index; a stream is not.** The whole-document
   decoders (`ip -j`, and every other `kind: json`) hand the shell a batch, and that is where the
   offer happens (`spatial::observe_adapted`). A streaming decoder hands its records straight to
   the consumer, and the shell does not buffer a stream in order to index it (§29.4, §34); one of
   its records becomes a place when something places it, through point 1.
5. **Only a record carrying the identity in full is offered.** `carries_full_identity` admits an
   adapted record whose schema identity fields are all present — and, for a process, §10.2's four
   parts. Those reduce to the identity the canonical record reduces to, so they reconcile. The rest
   stay typed values in the pipeline rather than becoming the duplicate node §37.1 forbids.
6. **Bytes are not a place.** `… | enter` where something arrived and none of it was an object is
   `spatial.not_enterable` (§40), naming what arrived. §37.2 admits "only canonical typed adapter
   output or explicit plugin schemas" into the index, so reading a place out of `raw ip link`'s
   bytes is the table heuristic that section forbids, and the refusal says so rather than
   reporting a place it could not find.

## Consequences

- `get interface`, `ip link` and `near --type interface` all answer for one `lo`, and the place
  says both saw it. Two nodes for one interface cannot arise from observing it twice.
- `enter` costs one targeted provider query more when the object arrived from an adapter. That is
  the same query the `enter <target> <identity>` grammar already makes, and it is what buys the
  identity.
- The asymmetry in point 4 is real and deliberate: an adapter that streams contributes to the
  index only through a place. Buffering `ps` to index it would break §29.4 for every user of it,
  and indexing a process from `ps` is exactly the case point 5 would reject anyway.
- Encoded by `spatial_contracts_missing::should_reconcile_an_adapted_object_with_its_native_twin_into_one_place`,
  `::should_never_let_raw_command_output_become_a_place`,
  `spatial_identity_missing::should_resolve_the_adapter_view_and_the_native_view_of_one_process_to_one_spatial_id`,
  and `docker/acceptance/cases/110-spatial-contributions.case` (s10-a … s10-f).

## Alternatives considered

- **Reconcile by loosening process identity** — treat two observations as one object when the pid
  and boot match and the start times are consistent to the second. Rejected: the start time is
  precisely what makes the identity safe against pid reuse (§10.2), and a rounding tolerance is a
  window in which the wrong process is the same place.
- **Index every record every pipeline produces.** It removes the asymmetry of point 4 and it makes
  `get process | where …` quietly re-file the whole process table on every command. §34's budgets
  and §2.16 both argue against making the shell's every answer an index write.
- **Give an adapted object an `unresolved_equivalence` edge to its twin** rather than merging, as
  §37.1's second sentence allows. Kept in reserve for the case point 1 cannot resolve; used as the
  first answer it would put two nodes on every map for every object seen twice, which is what the
  first sentence forbids.
