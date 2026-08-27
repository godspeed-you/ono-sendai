# ADR-0053: The adapter error family

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.65, §1.16, §1.18, §1.57; v0.2 §16.1, §43; ADR-0006
- Decided by: agent (autonomous)

## Context

Spec v0.3 §1.65 lists eleven adapter errors by screaming-case name (`ADAPTER_DECODE_FAILED`,
…) and says they "should carry" the adapter identity, the executable identity, the original
invocation, whether raw fallback is safe, and a recovery. Spec v0.2 §43 fixes the shape of every
error — `Ono-Sendai-ENNNN`, a dotted selector, one of twelve kinds (ADR-0006) — and the
taxonomy is closed and additive. The two have to be reconciled once, before the first adapter
emits anything.

## Decision

1. The eleven errors join the taxonomy as the **E09xx block**, named `adapter.<detail>` in the
   dotted form every other family uses. `ADAPTER_DECODE_FAILED` is `adapter.decode_failed`,
   `Ono-Sendai-E0907`. Numbers follow the order of v0.3 §1.65 and, like every other code, are
   never reused.

2. Each code maps onto an existing kind; no thirteenth kind is added:

   | Code | Selector | Kind | Why |
   |---|---|---|---|
   | E0901 | `adapter.not_available` | resolution | no adapter answers to the invocation |
   | E0902 | `adapter.disabled` | permission | understood, but switched off for this context |
   | E0903 | `adapter.unsupported_invocation` | provider | the adapter cannot answer this option set |
   | E0904 | `adapter.version_incompatible` | provider | the adapter cannot answer for this version |
   | E0905 | `adapter.executable_mismatch` | resolution | the resolved binary is not the one the contract names |
   | E0906 | `adapter.rewrite_failed` | provider | the adapter could not produce its own invocation |
   | E0907 | `adapter.decode_failed` | provider | the tool's output was not what the adapter promised to read |
   | E0908 | `adapter.schema_violation` | provider | decoded values fell outside the advertised schema |
   | E0909 | `adapter.capability_denied` | permission | `process.exec` refused the executable |
   | E0910 | `adapter.conflict` | conflict | two adapters claim the invocation and the rules cannot separate them |
   | E0911 | `adapter.required_for_structured_pipeline` | type | bytes where a consumer demanded objects (v0.3 §1.18) |

   The `provider` kind is right for §1.16/§1.57's "the tool answered, the adapter could not":
   an adapter is a provider of values in the sense of spec §16.1, and scripts that already
   `catch provider` for a provider that answered outside its schema get the same branch.

3. The payload v0.3 §1.65 asks for travels in the error's metadata under fixed keys —
   `adapter`, `adapter_version`, `executable`, `executable_version`, `invocation`,
   `raw_fallback_safe`, `recovery` — set by whichever increment emits the error. The keys are
   defined here so every emitter uses the same ones; ADAPT-002 adds the constructor.

## Consequences

- `docs/spec/errors.yaml` and `ono_core::ErrorCode` grow eleven entries in lockstep, checked
  by `cargo xtask spec-check` as before; `docs/reference/errors.md` is regenerated.
- Test: `crates/ono-core/tests/error_taxonomy.rs::should_carry_the_adapter_family_of_the_v03_specification_when_enumerated`.

## Alternatives considered

- A new `adapter` kind — rejected: spec §16.1 fixes the kinds, ADR-0006 already added the two
  §43 needs, and scripts branch on kinds; the eleven codes each have an honest existing kind.
- Keeping the screaming-case spellings — rejected: spec §43's dotted selectors are what
  `try`/`catch` and `where` match on.
