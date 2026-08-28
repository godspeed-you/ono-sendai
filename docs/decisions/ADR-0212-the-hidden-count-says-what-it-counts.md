# ADR-0212: The hidden count says what it counts

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.4 §3.6, §24.1, §24.2, §39.3, §2.9
- Decided by: agent (autonomous, `S11c`)

## Context

`docs/dogfood/v0.4-2026-08-28.md` finding 1. At the root, `look` printed

```text
 exits
   COMPUTE        4
   NETWORK        7
   STORAGE        4
   CONTAINERS     7
   IDENTITY       3
   DEVICES        215

 199 more not shown
```

The 199 are not domains. They are the neighbourhood's own `hidden_count` (§3.6) — the places
*behind* the exits that the view budget left out — and the line stands directly under the exits
with nothing to say otherwise, so a reader concludes the root has 205 exits. §24.2 forbids the
renderer from implying an exit that is not one, and this is that, in the one place a reader is
least equipped to doubt it: a count printed under a list is a statement about the list.

## Decision

The line names what it counts, and points at the command that shows them:

```text
 199 more neighbours not shown — `near` lists them
```

The alternative §24.1 leaves open — moving the count under a section of its own — was rejected:
`look` does not print a neighbours section (it prints exits, landmarks and changes), so the count
would arrive under a heading with nothing beneath it, which is a worse sentence than the one
being fixed.

Where the terminal is too narrow for the second half, the line drops it and keeps the count:
`199 more neighbours not shown`. §39.3 requires the view to stay usable at 40 columns, and a
sentence truncated mid-word is not usable. Nothing else about the view changes, and a place whose
neighbourhood fitted still says nothing — a disclosure that always appears is not a disclosure.

## Consequences

- `docker/acceptance/cases/109-spatial-storage.case` `s4d-r` matched the old wording; it now
  matches the new one, in this commit.
- The renderer gains a width decision, which `crates/ono-spatial-render/tests/place_view.rs`
  holds at 40, 80 and 120 columns along with the two halves of the rule:
  `::should_say_what_the_hidden_count_counts_when_the_view_bounded_the_neighborhood`,
  `::should_leave_the_disclosure_out_when_the_view_hid_nothing`,
  `::should_keep_the_disclosure_inside_a_narrow_terminal`. That file is also the first test
  coverage `place_view` had of its own.
- The spelling is `neighbours`, which is what `docs/spec/commands/spatial.yaml` and
  `docs/spec/spatial/spaces.yaml` already write in their prose. The identifiers stay
  `Neighborhood`/`hidden_count`; only the sentence a person reads changes.

## Alternatives considered

- **Print the count inside the exits block, as a seventh row.** Rejected: it would make the
  implication explicit instead of removing it.
- **Drop the line where `near` would show the same thing.** Rejected: §2.9 bounds the horizon and
  §2.17 makes what was left out visible; silence about a bound is the failure both forbid.
