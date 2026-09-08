# ADR-0710: A provider that says nothing about time claims nothing, and refuses the past

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §7.4, §21.1, §21.4, §21.5, §22.1, §34, §39.2; v0.2 §31.14; ADR-0610
- Decided by: agent (autonomous)

## Context

`ono_provider_api::Provider` had three primitives and its module documentation said so:
`snapshot` (state now), `subscribe` (changes over time) and, built on them, the runtime-managed
`watch`. v0.5 §21 adds a fourth responsibility that none of the three covers — answering about
the past — and, ahead of it, a harder one: saying *honestly* what may be answered about the past
before anyone asks.

The two have to arrive together. A `history` call with no capability advertisement forces every
consumer to discover a provider's limits by failing against them, and §7.4 forbids exactly that
kind of discovery: absence is a claim, and a claim needs coverage that says the source could have
seen the thing. A capability advertisement with no call is a promise nothing keeps.

Twenty-odd providers implement this trait. A fourth required method would break all of them for a
capability nineteen of them do not have.

## Decision

### 1. Two defaulted methods, in the shape `subscribe` already established

```rust
fn temporal(&self) -> TemporalCapabilities { TemporalCapabilities::none() }
fn history(&self, query: &Query, window: &TimeWindow) -> Result<ValueStream, ErrorValue> {
    Err(unsupported_history(self.id()))
}
```

Every existing provider keeps compiling, and every one of them keeps telling the truth without
being edited: it claims nothing and it refuses.

### 2. The default claims nothing rather than something plausible

`TemporalCapabilities::none()` is all-false with `retained_history: None`, and `Default` agrees
with it. The tempting alternative — defaulting `current_snapshot` to `true`, since every v0.2
provider does have one — was rejected. A capability that is true of every provider today is still
a claim the trait would be making on a provider's behalf, and §21.5's rule that a provider "MUST
NOT advertise" a capability it does not have cannot be enforced by a trait that advertises for it.
So the claim is the provider's to make, and `snapshot_only()` is a named constructor rather than a
default, so that making it is one line and forgetting to make it is visible as silence.

### 3. The default refusal is `temporal.unsupported_source`, not an empty stream

`Ono-Sendai-E1310` (ADR-0610). A provider that answered a historical query with its current
snapshot would present now as then, which §21.4 and §55.9 exist to prevent; one that answered with
an empty stream would be indistinguishable from "there was nothing there", which is a claim about
the past that no snapshot source can make. Refusing is the only answer that leaves the shell free
to look for the answer where it was recorded.

The refusal is built by one public function, `unsupported_history(provider_id)`, so that a
provider whose history depends on something that may be missing — journald with no journal —
refuses in the same words as one that never had any.

### 4. `TimeWindow` lives in `ono-provider-api`

`ono-temporal-core` owns `TimeRange`, and it is the same shape. It cannot be used here:
`ono-provider-api` is a `capability` crate that `ono-temporal-core` sits beside, and a provider
depending on the temporal core would invert the layering
(`docs/contracts/hardening/module_architecture.yaml`). `TimeWindow` is therefore declared in
`ono-provider-api` over `jiff::Timestamp`, which the crate already depends on, and converting one
to the other is a two-field move for whichever crate needs both.

Neither end of a window is ever filled in from a clock reading: §39.2 keeps the clock out of
everything below the recorder and the CLI, and an absent end means "as far as the source goes".

### 5. The registry routes the question and never widens the answer

`ProviderRegistry::history` mirrors `subscribe`, and `temporal_of(target)` reports the claim of
whichever provider would answer. It reports; it does not compose, cache or improve. A registry
that added a capability its provider withheld would be the shell inventing coverage, and the
contract test `should_report_a_providers_own_temporal_claim_without_widening_it` is what holds
that line.

## Consequences

- Every provider compiles unchanged and answers honestly unchanged. The four that now claim more
  than nothing say so in their own code: `systemd` (causal tokens, from the job identity of
  ADR-0712), `systemd-journal` (historical query), `systemd-logind` and `linux.procfs`
  (snapshot and checkpoint only, as §22.1 requires).
- `JournalProvider::history` is a real implementation rather than a claim: the window becomes
  `--since` and `--until` on the query the snapshot path already maps, so the one source in this
  tree that genuinely keeps the past answers about it through the primitive.
- The refusal's help line does **not** name `inspect provider`. There is no `provider` target in
  `docs/contracts/targets.yaml` and no such command, so naming it would be help that points at
  nothing. When a command for inspecting a provider's temporal claims lands — `ono.temporal-source/1`
  is its schema — the line should name it.
- `TemporalCapabilities` is `Copy` and has no builder. Seven booleans and an optional duration do
  not need one, and struct-update syntax against `none()` or `snapshot_only()` reads as the list
  of claims a provider is making.
- Encoded by `crates/ono-provider-api/tests/contract.rs`: a minimal provider that overrides
  nothing refuses with `E1310`; a provider advertising only `current_snapshot` refuses too; a
  provider claiming `historical_query` answers; the registry routes both; and an unclaimed target
  is `resolve.target_not_found` rather than an empty claim.

## Alternatives considered

- **A fifth required trait method.** Breaks every provider for a capability nineteen of them lack,
  and buys nothing a defaulted method does not: the default is already the honest answer.
- **`history` returning `Option<ValueStream>`.** Loses the reason. "This source cannot answer
  about the past" and "this source failed to answer" are different sentences and a user needs
  both.
- **Deriving capabilities from `docs/contracts/providers/*.yaml` at runtime.** The contract is
  checked against the implementation, never substituted for it; a provider that reads its own
  claims from a document could not be wrong about itself, which is precisely the drift the gate
  exists to catch.
