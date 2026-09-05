# ADR-0421: A withheld exit is forgotten when something asks again

- Status: accepted
- Date: 2026-08-31
- Spec refs: v0.4 §2.17, §6.2, §32.1, §32.2, §35.2, §42.4
- Decided by: agent (autonomous)

## Context

At `72aea1e`, on any host, with a listener the caller owns:

```text
ono -c 'enter 127.0.0.1:18080; near --type process'         → owner  python3  Process  available
ono -c 'enter 127.0.0.1:18080; look; near --type process'   → nothing at all, status 0, empty stderr
```

Found by `readme-demo` against the demo image and recorded on the board as "`look` empties the
neighbourhood it just described"; it reproduces on an ordinary host too. A reader who looks before
they walk is told the place is empty.

The mechanism is three correct decisions composing into a wrong answer.

1. Joining a socket to the process holding it is a scan of every `/proc/<pid>/fd` on the host —
   `CostClass::Expensive` — and §32.1 forbids a default `look` from spending it. So `look`
   declines, and `relations::observe` writes `record_withheld(id, "process", Unknown, "available
   on request")`. That is §32.2 exactly: a discoverable but unloaded exit.
2. `SpatialIndex::relation_summary` puts a refusal ahead of a count — §35.2 and §42.4, so that
   "files — permission denied for 14 process FDs" can never become "files — 0". It reads the
   withheld record first and `continue`s.
3. `near --type process` *does* ask for the scan. `observe` runs it and records the owner edge.

Then `relation_summary` reads the withheld record from step 1, and never reaches the edge from
step 3. The marker was written once and had nothing that ever removed it.

So the record outlived the statement it recorded. "Available on request" is a claim about what
this session has **paid for**, and the session had just paid.

## Decision

**A withheld record is forgotten when something asks that question again.**

`SpatialIndex::clear_withheld(id, label)` removes the record for one exit.
`relations::observe` calls it for the labels of a relationship provider **this interest asked for
by name**, immediately before consulting it. Whatever that attempt learns is then recorded in the
cleared place: edges, a fresh refusal via the existing failure path, or a fresh decline.

Where the clearing sits is the decision, and it is deliberate on three counts:

- **Only for a provider the interest asked for.** Only a *broad* provider — one that answers about
  an object by enumerating a whole target, §32.1's expensive class — can be declined, so only a
  broad provider can be asked for. `near --type process`, `follow <relation>` and `--all` are the
  three spellings that ask; a default observation asks for none of them, and what its decline said
  stays true until something does ask.
- **Including `--all`.** Verified rather than assumed: under this rule
  `enter <socket>; look; look --all` answers `process 1`, and without `complete` in the asking set
  it would answer `unknown — available on request` — the original defect, one spelling over.
- **Before the attempt, not after.** A provider that answers with a refusal re-records it in the
  same run; a provider that answers with edges leaves the exit clean. Clearing afterwards would
  have to distinguish "answered with nothing" from "refused" a second time, in a second place.

**The first version of this decision cleared for every provider `observe` consulted, and it was a
regression.** On a host with 780 processes and 556 services,
`spatial_interactive_missing::should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open`
— a liveness bound on a full-screen COMPUTE map — went from a pre-existing flake to a frequent
failure. Measured, A/B, on the same tree:

| variant | failures |
|---|---|
| no clearing at all | 0/12 on a quiet machine, 1/10 on a loaded one |
| clearing for every consulted provider | 9/23 |
| clearing only where the interest asked | 1/12 on a quiet machine |

The statistics were noisy enough not to decide it, so the decision rests on a count instead:
instrumented, **the whole interactive suite makes zero `clear_withheld` calls under the rule as
written, and five under the first version**, one of which removes a record. The narrowed rule
cannot reach that code path, so it cannot be what fails there; the residual 1/12 is the flake the
board tracks under B-spat-6.

*Why* one effective removal per suite cost that test its liveness bound is **not** established
here, and this ADR does not claim it. What is established is which rule touches the path and which
does not. It is the reason the rule reads "asked for" and not "consulted": a decline is cheap to
keep and evidently not cheap to lift, so it is lifted only where someone paid for the answer.

`relation_summary` is untouched. Its rule — a refusal outranks a count — is right, and the fault
was never that it honoured a withheld record; it was that the record was stale.

## Consequences

- `look; near --type process` answers what `near --type process` answers. Proven by
  `spatial_relationships_missing.rs::should_answer_the_same_neighbours_whether_or_not_a_look_came_first`,
  which compares the two answers rather than asserting either one, so it cannot be satisfied by
  both being empty.
- `should_not_report_the_owner_of_a_socket_nobody_looked_up_as_no_owner` still passes unchanged,
  including its third part — a socket reached *through* its owner arrives with the edge already
  observed. §32.1's decline is unaffected: what changes is only how long a decline is remembered.
- An exit refused for permission is re-read on the next command that asks about it, rather than
  being answered from the first refusal. That is a read the session used to save, and it is the
  right trade: a refusal that is cached forever is indistinguishable from one that is still true,
  and §35.4 wants a place to say what is the case now.
- **Two neighbouring defects this does not fix**, both reproduced while fixing this one and both
  now on the board:
  - `follow owner` on a socket place answers `Ono-Sendai-E1009 … available on request` **with or
    without** a preceding `look`, so an expensive relation has no `follow` spelling that pays for
    it. The board's note that a `look` changed this answer is a demo-image observation; on an
    ordinary host the two are identical, and the defect is that neither works.
  - `near --type X` prints nothing with status 0 when the only matching group is withheld. ADR-0271
    fixed exactly this for `near <relation>` and did not reach the `--type` spelling, so §42.4's
    false empty survives on one of the two.

## Alternatives considered

- **Let members outrank a withheld record in `relation_summary`.** One condition, no new index
  method — rejected: every other reader of the index (`spatial/map.rs`, `spatial/find.rs`,
  `view.rs`) would have to learn the same trick, and the stale record would still be there for the
  next reader who does not.
- **Clear on `record_edge`.** Closer to the data, but an edge's group label is not the edge's
  relation — one relation has two ends with two groups — so the clearing would have to re-derive
  the direction the recording already knows about, and it would not fire at all for a provider that
  honestly answers "no such neighbour".
- **Give the record an expiry.** Rejected: it makes the answer depend on how long the reader
  waited, and there is no honest interval — the question is whether it was asked again, not when.
- **Never record a decline as withheld, and derive "available on request" from the cost class at
  read time.** The cleanest shape, and a larger change: `relation_summary` already does exactly
  this for a place with no edges (`members.is_empty() && cost_class == Expensive`), so the two
  paths would merge. Not taken in a fix increment (AGENTS.md §4); it is a refactor, and it needs
  the failure-recording path to keep working through it.
