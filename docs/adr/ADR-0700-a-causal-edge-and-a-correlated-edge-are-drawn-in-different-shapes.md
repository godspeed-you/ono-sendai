# ADR-0700: A causal edge and a correlated edge are drawn in different shapes

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §15.1, §15.5, §15.6, §15.7, §15.8, §16.5, §16.6, §35.5, §45.2, §45.3;
  `docs/contracts/temporal/causality.yaml`
- Decided by: agent (autonomous)

## Context

v0.5 §45.3 ends with a sentence that is a constraint on a renderer rather than on a data model:

> A renderer MUST never use the same edge style/label for both.

§15.6 adds four words — `because`, `therefore`, `led to`, `caused` — that `preceded_by` MUST NOT be
rendered with, and §15.8 puts the whole causal vocabulary in one registry with the line "No
renderer may create causal language outside this registry."

Two ways of honouring that were open. The renderer could carry its own list of forbidden words and
its own idea of which relation classes are causal, or it could read both from the contract. The
first drifts the moment somebody adds a class or renames a word in `causality.yaml`, and the drift
is silent: the renderer keeps compiling and starts lying.

`ono.causal-explanation/1` already does half the work by keeping `cause`, `causal_chain`,
`correlations` and `preceding` in four fields, which §35.5 requires precisely so that a renderer
has no ranking step in which a correlation could be promoted into a cause.

## Decision

**1. The distinction is read, never derived.** An edge is causal when its record's `is_causal`
field says so. `ono.causal-link/1` declares that field for exactly this reason, so a renderer that
re-derived the predicate from the class name would be one rename away from calling a correlation a
cause.

**2. Two shapes, both visible without colour** (§45.2). A causal edge is a directed connector
labelled with the relation's own inverse label and the rule that emitted it:

```text
14:03:16.812  process/1827  disappeared
        |
        | caused  ono.systemd-job-result
        v
nginx.service failed
```

A non-causal edge is a non-directional dotted connector carrying the class's own word:

```text
14:03:06.000  /etc/nginx/nginx.conf  changed  .... correlated ....  nginx.service failed
```

The glyphs and the labels carry the difference, so it survives a monochrome terminal, a pipe and a
screen reader. This crate emits no escape sequence at all.

**3. Four sections out of four fields.** `causal_explanation` renders `known cause`, `chain` and
`evidence` from `cause` and `causal_chain`; `correlated`, `preceded by` and `coverage gap` from
`correlations`, `preceding` and `gaps`. There is no step at which an entry moves between them,
which is how §16.6's "MUST NOT move the config change into the `known cause` section because it
appears plausible" is honoured structurally rather than carefully.

**4. The link's fields are read wherever they travel.** `ono-temporal-query`'s `CausalStep` wraps
a `CausalLink` beside a depth and a summary, and its `CausalNode` and `TemporalAssociation` carry
an event *id* and words rather than an event record (§16.4). So an entry's `relation`,
`is_causal`, `rule`, `source` and `strength` are read from a nested `link` where there is one and
from the entry itself where there is not; a node is drawn from an embedded event record where one
came along and from the producer's own `summary` and reference where it did not. A renderer may
not resolve an id (§39.3), so the summary is what stands in an event record's place, and the
reference stays typeable.

**5. The test reads the prohibition from the registry.**
`crates/ono-temporal-render/tests/causal.rs::wording` parses
`docs/contracts/temporal/causality.yaml`, takes `renderer_wording.forbidden_for_non_causal` and
`renderer_wording.connector_style.{causal,non_causal}.style`, and asserts that an explanation whose
edges are all non-causal contains none of those words and that the two declared styles differ. A
word added to the registry therefore tightens the test with no code change, and a renderer that
started writing "led to" about a correlation fails the gate rather than a review.

Matching is on word boundaries for the single words and on substring for `led to`, so the section
heading `cause` and the strength `correlated` are unaffected while `caused` is caught.

## Consequences

- `ono.causal-explanation/1` has no producer yet: `CausalExplanation` in `ono-temporal-query`
  carries no `to_record`. When it grows one, the two shapes above are what this renderer already
  reads, and §16.5's `failed at 14:03:17.004` needs one field the schema does not declare — see
  the report to the lead.
- Adding a fifth relation class needs no renderer change: the connector follows `is_causal` and the
  label follows the class's own `inverse`.
- The forbidden-word test is a text search over rendered output, which is exactly the kind of
  assertion AGENTS.md §11 allows for a renderer and refuses for a data contract.
- The registry becomes load-bearing for a test in another crate. `xtask/src/temporal.rs::check`
  already holds `causality.yaml` against itself, so a malformed registry fails before this test
  has to explain itself.
- A causal chain is drawn origin-first while `causal_chain` is stored nearest-first (§16.7). The
  reversal is in the renderer, where the reading order is a presentation decision.

## Alternatives considered

- **A `Style` enum in Rust with the words as constants.** Rejected: it is the drift §15.8's last
  sentence exists to prevent, and it puts causal language somewhere other than the registry.
- **One ranked list of associations with a `kind` column.** Rejected: §35.5 requires the arrays to
  stay separate, and a ranked list is the shape in which a plausible correlation floats to the top.
- **Colour for the distinction, words as a fallback.** Rejected by §45.2 and by the fact that this
  crate returns `Vec<String>` to a caller that may be a pipe.
