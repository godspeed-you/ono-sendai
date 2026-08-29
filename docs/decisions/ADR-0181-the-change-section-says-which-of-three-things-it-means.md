# ADR-0181: `look --changes` says which of three things it means

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.17, §3.6, §24.3, §25.2, §25.4, §26, §35.2
- Decided by: agent (autonomous, phase S7)

## Context

§24.3 is two sentences: "The `changed` group SHOULD show only changes relevant to the current
place and recent configurable time horizon. No fake change summary may be generated when no event
source or comparison snapshot exists." S4a delivered the second half by answering `unsupported`
with no entries, which was honest while nothing compared anything. §25.4 names the source this
build now has for a still view: "Where event streams are unavailable, Ono MAY build live changes
by comparing successive snapshots. The provenance must identify that the change was inferred from
snapshots."

## Decision

**`look --changes [window]` compares this place's neighborhood against the last time this session
looked at it, and says which of three answers it is giving.**

| `state` | means |
|---|---|
| `unknown` | this session has not looked at this place before. There is nothing to compare to — not "nothing changed". |
| `empty` | there was a snapshot, and nothing differs from it. |
| `available` | there was a snapshot, and these are the differences. |

`source` is `snapshot_comparison` wherever a comparison happened, and null where none did (§25.4).
Entries are `ono.spatial-change/1` values carrying the kind, the place, the §3.7 reason where the
closed vocabulary has one, and when the difference was seen.

**What is compared is the complete neighborhood, not the ranked one.** The ranking is a view
decision: two rankings of one unchanged system differ whenever the budget cuts a tie differently,
and comparing those would report change where nothing moved — the decorative motion §25.2 and
§2.12 forbid. The complete set is computed from the same observation the ranked view was built
from (`view::neighborhood_and_whole`), so nothing is asked of a provider twice.

**The baseline is taken whenever `--changes` is asked**, whether or not it could answer, so the
next ask has something to compare to. §26's landmark recalculation needs no separate trigger: the
landmark engine already runs on every `absorb`, so the reasons a comparison reads are current.

## Consequences

- `spatial_map_missing::should_not_invent_a_change_section_when_no_snapshot_or_event_source_exists`
  stays green, now for a stronger reason: a one-shot script gets `unknown` with no entries, which
  is the answer §24.3 demands rather than the absence it forbids.
- The three answers are proven in the container by `docker/acceptance/cases/108-spatial-live.case`
  (s7r, s7s, s7t).
- A place looked at twice in one session reports what moved between the two looks — on a busy
  collection that is a long list, and it is a true one. Whether §24.3's "relevant to the current
  place" should further rank it is a decision for whoever finds the list too long; the window is
  already `spatial.look.change_window`.
- A session-local baseline means two `ono` processes never share one, which is §29.2's rule for
  the current place applied to the same state.

## Alternatives considered

- **Keeping `unsupported` until a provider event stream exists.** Rejected: §25.4 explicitly
  allows the comparison, and answering `unsupported` while a comparison is available would be as
  much of a false statement as inventing changes would be.
- **Comparing the ranked neighborhood.** Rejected: it reports change that is a ranking artefact.
  Measured on a 300-process collection it produced 52 spurious entries between two immediate
  looks.
- **Reporting only landmark changes.** Rejected: §24.3's example is `worker/1871 cpu +41%`, an
  ordinary neighbour, not a landmark.
