# ADR-0682: `look`'s change section is the changes engine, and the snapshot comparison is gone

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §13.5, §13.4, §18.7, §2.17; v0.4 §24.3, §25.4, §35.2
- Decided by: agent (autonomous)

## Context

v0.4 §24.3 gave `look` a `changed` section and one source for it: the difference between two
observations the session took, kept as a `PlaceSnapshot` baseline per place (§25.4). v0.5 §13.5
replaces that: "`look`'s v0.4 `changed` section SHOULD be backed by the same `TemporalChange`
engine when v0.5 evidence exists", and "it MUST NOT retain a separate ad-hoc snapshot comparison
implementation once the temporal engine is available".

Two implementations of "what changed" can disagree, and the one a reader sees depends on which
command they typed. §18.7's return-to-now summary is the third caller of the same question.

## Decision

**`look --changes` is `ono_temporal_query::changes::changes`, and the snapshot comparison is
deleted.**

- `SpatialSessionState::rebase` and the `baselines` map are removed. Nothing else used them.
- The change section is composed from `ono.temporal-change/1` records, which
  `ono.change-summary/1` accepts because its `entries` field is `list<record>`.
- Three answers stay apart, which is what §24.3 and §2.17 both require:
  - **`unsupported`** — this session holds no temporal evidence, so nothing was watching. It is
    never rendered as "nothing changed".
  - **`empty`** — the ledger was asked over the window and nothing differs.
  - **`available`** — the ledger was asked and these are the differences, each a typed value whose
    unknown side stays unknown (§13.4).
- `source` is `ono.temporal-ledger`, so a reader can see which engine answered.
- The scope is the current place and the objects directly around it, which is §24.3's "changes
  relevant to the current place"; the window is `--changes`'s duration back from the coordinate,
  which is the present in the present and the historical instant in the past.
- §18.7's map summary and §13.5's `look` section call the same function. The renderer for the
  summary is `ono_temporal_render::return_to_now`, which reads the same records.

## Consequences

- A session with no ledger reports `unsupported` where v0.4 reported `unknown` on the first look
  and a comparison on the second. That is a truthful downgrade: without v0.5 evidence nothing was
  watching, and the honest word for it is the one §35.2 already has.
- `ono_spatial_events::{PlaceSnapshot, compare_places}` lose their only caller in the shell. They
  stay in their crate; pruning a library's API is not part of this change (AGENTS.md §4).
- `crates/ono-cli/tests/spatial_map.rs::should_not_invent_a_change_section_when_no_snapshot_or_event_source_exists`
  keeps passing: it already admits `unsupported` as one of §35.2's states.

## Alternatives considered

- **Keeping the snapshot path as a fallback where no ledger exists.** Rejected by the MUST NOT in
  §13.5, and by the fact that a fallback is a second implementation whichever way it is spelled.
- **Emitting `ono.spatial-change/1` values converted from `TemporalChange`.** Rejected: the
  conversion would be a third vocabulary between two canonical ones, and §13.2 fixes the change
  classes in the temporal contract.
