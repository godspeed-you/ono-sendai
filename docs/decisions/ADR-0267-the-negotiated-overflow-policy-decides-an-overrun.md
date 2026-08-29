# ADR-0267: The negotiated overflow policy decides an overrun

- Status: accepted
- Date: 2026-08-29
- Spec refs: §31.15, §31.33, §31.34, §31.63
- Decided by: agent (autonomous, `close-spat`)

## Context

§31.15 gives five overflow policies and says "when a plugin cannot keep up, policy can choose"
between them, with the manifest declaring a preference and the host holding final authority.
`OverflowPolicy` existed with all five variants, `negotiate` clamped the manifest's preference into
the contract, and then **nothing read `contract.overflow`**. Every overrun took one hardcoded path:
an emission beyond credit was a protocol violation, which quarantines the instance.

So a package could declare `overflow: coalesce`, see it echoed back in its negotiated contract,
and be quarantined the first time it coalesced nothing.

## Decision

`host_emit` consults `contract.overflow` when an emission exceeds the stream's remaining credit:

| Policy | What happens |
|---|---|
| `block-upstream` | protocol violation, as before — the producer was told to wait and did not |
| `fail-stream` | the stream ends with `runtime.backpressure_failure` (K11206); the instance keeps running |
| `drop-newest` | the values that fitted are delivered, the rest dropped |
| `drop-oldest` | the *last* values that fit are delivered, the earlier ones dropped |
| `coalesce` | values sharing a record identity collapse to the newest; what still does not fit is dropped oldest-first |

Three things follow from the spec text rather than from convenience:

1. **Only `block-upstream` punishes the package.** §31.34 says plugin failure degrades the plugin.
   An overrun under a policy that permits losing values is not a failure at all, and under
   `fail-stream` §31.15 says the *stream* terminates — quarantining the package instead is a
   different and larger answer than the one the policy names.
2. **`fail-stream` replies to the emit with the error, not with a receipt.** The producer asked to
   emit and the emission did not happen; an OK reply would tell it otherwise.
3. **Coalescing folds by record identity, and a value that declares none is left alone.** §31.15:
   "combine repeated updates by object identity. Requires the schema to declare one." Collapsing
   two values nobody said were the same object would lose data while claiming not to.

**What was dropped is recorded.** Every overrun writes a `warn` line into the package's structured
log with `dropped` and `policy` — §31.33's own example is that line. §2.17's rule applies to data
the shell decided to lose as much as to data it never had.

**`drop-newest` still cannot come from a manifest.** `negotiate` already refuses it as a
preference because §31.15 calls it "explicit only, never a default"; it reaches a contract only
from host policy, and this ADR does not change that.

## Consequences

- The example plugin's `flood` mode now finishes its invocation after the overrun. Under
  `block-upstream` the host has already quarantined it and never reads that; under every other
  policy the stream survives, and a fixture that never ended would hang rather than say so.
- `should_quarantine_a_plugin_that_emits_beyond_credit` is unchanged and still green: the default
  host policy is `block-upstream`.
- Encoded by `ono-kuang-sdk/tests/conformance.rs::should_end_the_stream_and_keep_the_instance_when_the_negotiated_overflow_fails_the_stream`
  and `::should_keep_the_oldest_values_and_drop_the_rest_when_the_overflow_drops_the_newest`.

## Alternatives considered

- **Bounding the host's channel and applying the policy on the consumer side** — the channel is
  unbounded and the credit window is the real bound, so the overrun is observable exactly once, at
  the emission. A second bound would give two places to disagree about the same limit.
- **Leaving the protocol violation and treating the policy as advice to the package** — that is
  what the code did, and it is what §31.15's "host policy has final authority" rules out.
