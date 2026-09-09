# ADR-0775: The change section reports absence only where coverage supports it

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §8.2, §8.3, §13.5, §24.3 (v0.4), §25.4 (v0.4), §55.5
- Decided by: agent (autonomous)

## Context

v0.5 §13.5 makes `look`'s v0.4 `changed` section the temporal engine's answer and prohibits the
implementation it used to have: *"It MUST NOT retain a separate ad-hoc snapshot comparison
implementation once the temporal engine is available."* The section was duly rewired onto
`ono_temporal_query::changes`, and the rewiring introduced a claim nobody had checked: a ledger
that had observed nothing over the window answered with an empty list, and the section reported
that as `state: empty` with `ono.temporal-ledger` as the source.

That is a statement that nothing changed, made on the evidence of a source that was not watching.
v0.4 §24.3 forbids it in as many words — *"No fake change summary may be generated when no event
source or comparison snapshot exists"* — and v0.5 §55.5 names the same shape as the failure mode
that destroys operator trust. §8.2 says which fact settles it: an absence is a finding only where
coverage is complete over the interval, and §8.3 fixes the session's own observation at `partial`,
because a session sees every action taken through the shell and only what a provider happened to
report besides.

Two v0.4 acceptance proofs read the old answers and failed against the new ones:
`102-spatial-look-near.case::s4q` (a look with nothing to compare to says so) and
`108-spatial-live.case::s7r`, `s7t`, `s7s`.

## Decision

The change section asks about coverage before it reports an absence, and only before that:

- **`unsupported`** — no temporal evidence is installed. Nothing was watching.
- **`available`** — the ledger found differences. Reported whatever the coverage is: a change that
  was observed is evidence of itself, and withholding it because the *rest* of the window is
  unknown would lose a fact the shell holds.
- **`empty`** — the ledger found nothing **and** `CoverageSummary::headline()` over the window is
  `Complete`. This is the only state that asserts that nothing happened.
- **`unknown`**, with a `null` source — the ledger found nothing and the window is not covered end
  to end.

Coverage is composed by the temporal session rather than by the spatial layer, through a third
method on `crate::spatial::TemporalEvidence`. §55.7 keeps temporal logic out of the spatial code,
and the session's observation origin is temporal state: the spatial side asks the question and the
temporal side answers it.

The historical `look --changes` follows the same rule against the same helper.

`s7s` is rewritten rather than deleted. It asserted the v0.4 source name `snapshot_comparison`,
which §13.5 makes unreachable; what it proves now is the rule that replaced it — a second look
invents no comparison it has no source for, and nothing in the run names the prohibited engine.

## Consequences

- With recording disabled, `look --changes` is `unknown` rather than `empty`, always: a session
  ledger is `partial` by §8.3 and can never prove an absence. That is the honest reading and it is
  what §24.3 asks for. It also means `empty` is a fact about a *recorder*, which is the right place
  for it.
- `empty` becomes reachable when a source has written complete coverage over the window, which is
  the recorder's job (§10.4). **As first written this consequence did not hold**, and the reason is
  worth keeping: `SessionEvidence::coverage` appends the session's own `session.events` interval at
  `Partial` for every window, and `CoverageSummary::headline()` is `Complete` only when *every*
  capability is, so one always-partial capability made the other branch unreachable however
  complete a recorder's coverage was. The composition now asks whether the capabilities a change
  would have come from can prove an absence, rather than asking the union to be complete. The
  safety direction was never in doubt — what was wrong was that the honest branch could not be
  reached from the other side. It is reachable now, and
  `crates/ono-cli/tests/spatial_look_changes.rs::should_report_nothing_changed_only_when_a_source_covered_the_whole_window`
  is the proof — a test that previously asserted `unknown` over complete coverage, which is to say
  it had encoded the defect as the contract.
- A change is still reported under partial coverage, so enabling the recorder adds absence claims
  rather than adding findings.
- Encoded by `docker/acceptance/cases/102-spatial-look-near.case::s4q`,
  `108-spatial-live.case::s7r`, `s7s`, `s7t` and
  `247-look-changes-are-temporal.case`.

## Spec deviation

- Section: v0.4 §25.4
- Text: "What the changes were observed through — an event stream or a snapshot comparison"
- Instead: a snapshot comparison is never the source of `look`'s change section. The sources are
  the temporal ledger, or none.
- Why: v0.5 §13.5 prohibits retaining the ad-hoc snapshot comparison implementation once the
  temporal engine exists, and §5.2 of `AGENTS.md` gives the later specification precedence where
  two overlap. The `ChangeSource::SnapshotComparison` variant survives in `ono-spatial-events` for
  the live-map path that legitimately compares snapshots (§25.1); what is gone is `look` answering
  §24.3 from one.

## Alternatives considered

- **Report `empty` and let the coverage line beside it carry the caveat.** Rejected: §24.3's rule
  is about the summary itself, and a reader who sees "nothing changed" has been told something
  false whatever the next line says.
- **Withhold changes found under partial coverage too.** Rejected: it would hide observed facts to
  protect a rule that is only about absence, and §35.3's "unknown is null, never fabricated" does
  not ask for the reverse.
