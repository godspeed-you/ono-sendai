# ADR-0610: The temporal error family takes free numbers and keeps the names the spec fixed

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §34; v0.4.1 §21.4, §53.1; v0.2 §43; ADR-0006, ADR-0453
- Decided by: agent (autonomous)

## Context

v0.5 §34 reserves the `temporal` error family and tabulates fourteen codes, E1101 through E1114.
Three of those numbers are already in the registry and have been since the v0.4.1 resource
tranche: `Ono-Sendai-E1101` is `resource.item_limit`, `E1102` is `resource.byte_limit` and
`E1103` is `resource.materialization_limit` (ADR-0453). They ship in 0.4.4, scripts branch on
them, and `docs/contracts/errors.yaml` opens by stating the rule that governs the collision:
"a code is never renumbered, never removed and never re-pointed at a different meaning."

Two things are being asked for at once, and only one of them is contested. The *names* —
`temporal.invalid_time`, `temporal.read_only`, and the twelve beside them — are the stable,
machine-readable selectors that `try`/`catch` and `where` predicates match on (ADR-0006), and
nothing in the tree claims them. The *numbers* are a rendering of the same identity, and three
of them are taken.

## Decision

The temporal family keeps every name v0.5 §34 fixes and opens the lowest free hundred-block, E13:

| v0.5 §34 | this registry | name | kind |
|---|---|---|---|
| E1101 | Ono-Sendai-E1301 | temporal.invalid_time | parse |
| E1102 | Ono-Sendai-E1302 | temporal.not_recorded | resolution |
| E1103 | Ono-Sendai-E1303 | temporal.out_of_retention | resolution |
| E1104 | Ono-Sendai-E1304 | temporal.read_only | safety |
| E1105 | Ono-Sendai-E1305 | temporal.present_only | safety |
| E1106 | Ono-Sendai-E1306 | temporal.ambiguous_event | resolution |
| E1107 | Ono-Sendai-E1307 | temporal.store_unavailable | io |
| E1108 | Ono-Sendai-E1308 | temporal.store_corrupt | io |
| E1109 | Ono-Sendai-E1309 | temporal.permission_denied | permission |
| E1110 | Ono-Sendai-E1310 | temporal.unsupported_source | provider |
| E1111 | Ono-Sendai-E1311 | temporal.coverage_gap | provider |
| E1112 | Ono-Sendai-E1312 | temporal.clock_uncertain | provider |
| E1113 | Ono-Sendai-E1313 | temporal.recorder_not_running | conflict |
| E1114 | Ono-Sendai-E1314 | temporal.recorder_already_running | conflict |

The mapping is the identity on the last two digits, so a reader holding §34 open finds each row
by adding two hundred. Unknown cause remains what §34 says it is: no code at all.

## Consequences

- `docs/contracts/errors.yaml` and `ono_core::ErrorCode` gain fourteen rows each, and
  `spec-check` compares them as it compares every other family.
- Nothing already shipped changes meaning. A 0.4.4 script that catches `resource.item_limit`
  catches the same thing on 0.5.
- A reader of v0.5 §34 who greps for `E1101` finds the resource code and this ADR, because the
  deviation heading below is the greppable record AGENTS.md §8 requires.

## Spec deviation

- Section: v0.5 §34
- Text: "| E1101 | `temporal.invalid_time` | Time selector cannot resolve unambiguously. |"
  and the thirteen rows beside it, through E1114.
- Instead: the fourteen names are registered unchanged at codes E1301 through E1314, in the same
  order.
- Why: E1101, E1102 and E1103 were allocated by v0.4.1 §21.4 to the resource family and are in a
  released binary. v0.5 §0.1 requires the implementation to record a deviation where a normative
  requirement cannot be met; renumbering a shipped code to free the number would break the one
  guarantee the registry makes about every code it holds.

## Alternatives considered

- **Renumbering the three resource codes.** It re-points a shipped identity at a different
  meaning, which is the single thing the taxonomy forbids, and it would silently change what a
  0.4.4 script catches.
- **Registering the temporal names with no code.** Every error in the registry renders a code;
  a family that does not would be the only one, and `inspect` and the error renderer both assume
  the code exists.
