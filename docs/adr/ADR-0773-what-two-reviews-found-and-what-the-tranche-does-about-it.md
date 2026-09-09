# ADR-0773: What two independent reviews found, and what the tranche does about it

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §7.5, §9.1, §17.5, §30.3, §30.6, §31.8, §34, §55.5; v0.2 §49; ADR-0015 T1,
  ADR-0624, ADR-0634
- Decided by: agent (autonomous)

## Context

Two review agents audited the temporal engine with no assumption that it was correct — one
against the specification section by section, one against the privacy and security boundaries
§30 draws. Between them they returned twenty findings. Most were real. This ADR records the four
that changed a contract; the rest were fixed against the rule they already broke and needed no
decision.

## Decision

### 1. Every string a temporal or spatial renderer emits is sanitised

`ono-temporal-render` and `ono-spatial-render` both depended on `ono-value` alone, so
`ono_render::sanitise` was unreachable from either, and neither implemented a substitute. A
reviewer built against the crate and produced a timeline row that retitled the terminal window
and a second row forged out of a newline in a subject label.

The strings that reach those renderers are a service's name, a container's label, a file's path,
a systemd unit, a KUANG/11 package's own namespaced rule id and a remote host's event — data the
machine was given rather than data Ono chose. v0.2 §49 assumes it is hostile and ADR-0015 T1 puts
one implementation of the rule in the tree. Both crates now depend on `ono-render` for that one
function and sanitise in the accessors every line goes through.

`ono-render` is in the same layer, so the dependency changes nothing about §39.3: that rule is
about providers, ledgers and networks, and a text sanitiser is none of them.

### 2. A secret is redacted by the sequence, not by the word

ADR-0624's rule read `name=value` and redacted where the name looked secret. `--token abc` is two
words and neither contains `=`, so `set credential registry --token abc` was persisted whole. A
connection string's password rode through the command's target untouched.

`RedactedCommandSummary::of` now walks the sequence: a word that names a secret and carries no
value redacts the next bare word, and a following flag is not a value. Any word whose shape is a
URL with userinfo keeps its scheme, its user and its host and loses its password. §30.6's
"command-line arguments known to contain secret values" is about the argument, and an argument
is sometimes two words.

### 3. Retention keeps the checkpoint reconstruction is built on

ADR-0634 §3 deleted every checkpoint older than the oldest surviving event, reasoning that "a
checkpoint older than every event is a projection nothing can be replayed from". That is
backwards. §9.1 selects the nearest checkpoint *at or before* the requested instant and applies
events *forward* from it, so the newest checkpoint below the boundary is exactly the base state
for the earliest instants still retained. Deleting it left the store unable to answer inside its
own retention window: every field of every object fell to unknown.

The sweep now removes a checkpoint below the boundary only where a newer one below the boundary
has superseded it. ADR-0634 §3's reasoning is superseded; its other four decisions stand.

### 4. An expired window says it expired

Retention deleted coverage rows and left nothing behind. A window that had been recorded, had
been complete, and had been swept then composed to `not_recorded` — which renders as
`recorder not running`, an affirmative claim that nothing was watching a period the recorder was
in fact watching. §55.5 names a silent gap as trust-destroying and this is the articulate version
of one.

Worse, the sweep computed its boundary as the oldest *surviving* event, so a sweep that emptied
the store skipped the coverage deletion entirely and left rows claiming `complete` for a window
whose events were gone. A reconstruction reading those concludes that nothing happened — a
confident wrong answer where §7.5 has a word for the truth.

The sweep now falls back to the age horizon when it has emptied the store, and leaves one
`unavailable` interval per scope that lost coverage, spanning what was removed. §34's
`temporal.out_of_retention` is what a query against that window then answers, and it already
names the earliest instant still held.

## Consequences

- Two tests changed, and both had encoded a defect: one asserted that a checkpoint below the
  boundary goes, and one asserted that a sweep leaves no coverage at all. Each now asserts what
  the corrected rule requires, and each says why in its own body.
- A renderer test greps its own output for escape bytes and for a forged row, so the sanitiser
  cannot be removed without a failure that names the reason.
- Three redaction tests cover the separated secret, the flag that follows a secret-named flag and
  is not its value, and the URL password.
- The findings not recorded here were fixed against rules they already broke: an unguarded
  `append_links`, a `truncated` flag computed after filtering, a session ledger bounded only in
  its events, and a handful of vacuous tests. They are ordinary defects, not decisions.

## Alternatives considered

- **Sanitising in the sink rather than in the renderers.** The sink receives finished lines; by
  then a forged row is a row. The renderer is the last place that still knows which bytes came
  from the machine.
- **A `Secret` value type instead of a name-shaped rule.** It is the better answer and it is a
  v0.2 change: nothing in the value model carries secrecy today, and inventing it inside the
  temporal tranche would put the one typed secret in the tree behind a feature that is off by
  default. Recorded here as the thing to do when the value model next moves.
- **Leaving the boundary interval store-wide rather than per scope.** A store-wide row decodes
  into no scope and is dropped on the way back out, so it would have been a comment.
