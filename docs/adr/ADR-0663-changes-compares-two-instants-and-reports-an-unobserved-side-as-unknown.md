# ADR-0663: `changes` compares two instants and reports an unobserved side as unknown

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §13.1, §13.2, §13.4, §13.5, §18.7, §6.2, §8.5, §32.3; v0.2 §10.5
- Decided by: agent (autonomous)

## Context

§13.1 makes `changes` a comparison between two instants, and §13.2 fixes the answer at five
classes. §13.4 fixes the hard case: "if one side lacks enough evidence, the field MUST be reported
as unknown rather than fabricated", with the coverage that explains it. The specification does not
say what the engine does with an object that appeared *and* went away inside the window, nor what
"lacks enough evidence" is in terms the code can test.

§13.5 adds a second requirement that shapes the API rather than the algorithm: `look`'s
recent-change section must be backed by this engine and must not keep a second ad-hoc snapshot
comparison. §18.7's return-to-now summary is a third caller of the same thing.

## Decision

**One function answers all three, over the ledger, comparing endpoints.**

1. **`changes(ledger, request, coordinate)`.** `--until` omitted takes `coordinate`, which is the
   active temporal coordinate the caller passes; nothing here reads a clock (§39.2). `look` and
   the return-to-now summary call this function, and `summarise` is a projection of its output
   rather than a second computation, so the list and the summary cannot disagree (§13.5).
2. **The class comes from the last lifecycle event in the window.** An object whose last lifecycle
   event is `object.appeared` is `added`; `object.disappeared` is `removed`; a subject with
   neither and at least one changed field is `changed`. An object that appeared and went away
   inside the window therefore reads as `removed`, because that is what a reader standing at
   `--until` sees.
3. **A field's two sides are the first and the last observation of it.** Repeated movement folds:
   `active -> reloading -> failed` is one change from `active` to `failed`. §13.1 asks for a
   comparison rather than a replay, and the replay is what `timeline` is.
4. **§13.4's unknown is "nothing observed the earlier side", and that is a null.** Where the
   earliest in-window change carries no `before`, the folded change has `before: None` and
   `ChangeCertainty::Unknown`. Never a zero, never an empty string, never "changed to nothing"
   (v0.2 §10.5 reads that null back as unknown at field access). Where the source *did* report the
   transition, the value it reported is evidence for the earlier side and is kept, at the
   certainty the source claimed.
5. **The coverage travels with the change.** Every change carries the `CoverageSummary` composed
   over the window, so a partial before-state comes with the gaps that explain it, which is what
   §13.4's example prints. §8.5's per-capability composition is what the summary already is.
6. **`change_id` is a digest of the answer.** A change is a computed answer over a window rather
   than a row in the ledger, so its identity is FNV-1a over the class, the subject, both instants
   and the folded fields — stable across two runs of one comparison, different for two different
   ones. The ledger's own identities stay SHA-256 in `ono-temporal-core`; nothing here is one.

## Consequences

- `changes --since 30m | group subject.object_type` works because each change is an
  `ono.temporal-change/1` record (§28.1).
- The engine is bounded: one event query and one coverage query over the window, both pushed down,
  and a ceiling on the answer (§32.3, §43.1).
- An object that appeared and went away in the window is reported once. A reader who wants both
  transitions asks `timeline`, which is the command that replays.
- `ChangeSummary` gives §18.7 its `+3 processes / -1 connection` as net counts per object type and
  its `nginx.service active -> failed` as typed highlights. The formatting is the renderer's.
- Encoded by `crates/ono-temporal-query/tests/changes.rs`.

## Alternatives considered

- **Reconstructing both endpoints and diffing them.** The honest general answer, and it needs
  `ono-temporal-reconstruct`'s replay for every subject in the scope, which §32.3's 150 ms budget
  for an hour cannot pay. It is also strictly weaker on §13.4: a reconstruction that filled a
  field from an interval reports a value where the ledger reports nothing observed. Folding the
  events keeps the evidence visible. This decision should be revisited once reconstruction can
  answer a scope at an instant within budget.
- **Reporting a zero for an unobserved side.** What §13.4 exists to forbid.
- **A second summary implementation for `look`.** What §13.5 exists to forbid.
