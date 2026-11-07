# ADR-0961: A number a provider sends reads back as the same number

- Status: accepted
- Date: 2026-11-07
- Spec refs: spec §12.1, §41.4, §41.5, §48.2
- Decided by: agent (autonomous)

## Context

The scheduled coverage-guided campaign (run 42363777026, 2026-11-07, on `main` 25370c63) stopped
`provider-messages` on its round-trip assertion: a `PlanContributeParams` whose action carried
the number `1.04858e+132` decoded, re-encoded and decoded again as `1.0485799999999999e+132`.
The target holds that a message which decodes must re-encode to a message that decodes to the
same thing, "or what the host logs is not what the provider said" (§48.2).

The drift is in the decoder. `serde_json` without its `float_roundtrip` feature parses decimal
numbers with a fast routine that may land one unit in the last place away from the nearest
`f64`: it reads `1.04858e132` as `1.0485799999999999e132`. The encoder then prints that value
exactly, so the host holds, logs and forwards a number the provider never sent. Every crate in
the workspace takes `serde_json` from the workspace dependency, so the same holds wherever JSON
is read: the plugin protocol, the remote protocol, values the shell decodes from JSON, and
persisted history.

## Decision

The workspace dependency enables `serde_json`'s `float_roundtrip` feature. A decimal number in
any JSON the shell reads is parsed to the nearest `f64`, so encoding a decoded value and decoding
it again yields the same value.

The input the campaign found is committed under `fuzz/artifacts/provider-messages/`, where
`corpus.rs` replays it on every gate run (§41.4).

## Consequences

Easy: a number crosses the provider boundary, the record and the log unchanged; the
round-trip assertion holds for every message the target decodes.

Hard:

- Parsing a float is somewhat slower. JSON on the shell's paths is small and dominated by I/O;
  no performance budget measures float parsing on its own.
- A value an earlier build already stored with the drifted digits stays as stored; it is read
  back unchanged now, so it does not drift further.

## Alternatives considered

**Compare floats with a tolerance in the fuzz target.** It would hide a real change of the
provider's data instead of removing it.

**`arbitrary_precision`.** It keeps every number's text, but changes `serde_json::Number`'s
representation and the behaviour of every `as_f64` caller; nearest-`f64` parsing is what the
round trip needs.
