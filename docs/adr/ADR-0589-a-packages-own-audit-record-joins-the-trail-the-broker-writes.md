# ADR-0589: A package's own audit record joins the trail the broker writes

- Status: accepted
- Date: 2026-09-06
- Spec refs: §31.16, §31.33, §31.37, §35.3, §50; `docs/contracts/kuang/protocol.v1.yaml` →
  `audit.event`; `docs/contracts/schemas/plugin-audit-event.v1.yaml`;
  `docs/contracts/commands/kuang.yaml` → `ono.audit.get`;
  §27.6 of `docs/architecture/external-system-provider.md`;
  ADR-0040, ADR-0041, ADR-0107, ADR-0587, ADR-0588
- Decided by: agent (autonomous)

## Context

`audit.event` is the host call by which a package adds to the audit trail. The contract describes
it precisely — *"A package can add to the audit trail but cannot write the host's own records, and
cannot suppress one"* — and the supervisor implemented it as:

```rust
lock(&self.shared).plugin_events.push(params);
```

**Nothing read `plugin_events`.** Not `LoadedPlugin::audit()`, which returns the broker's trail;
not `LoadedPlugin::logs()`, which returns `audit.log` records; not `get audit`; not the persisted
`audit.jsonl`. A package could call `audit.event` for an hour and no operator, no export and no
test could observe a single record.

The consequence is the one that matters for a contract: **a call nothing can observe is a call a
package cannot be held to.** The generic external-system provider contract §27.6 asks a
security-sensitive provider operation to emit an audit record, and the Kubernetes provider
building against it recorded exactly this as the reason it wrote none: under "no test, no code",
an emission no test can assert is not an emission worth writing. That is the correct reading of
the discipline, and it means the missing accessor was blocking a normative requirement in a
downstream repository.

The broker's own records cover what the broker *checked*. They cannot cover what only the package
knows: which credential plugin it invoked, which permission the fronted system refused it, which
change it made to a system the broker sees only as bytes on a connection it already authorised.

## Decision

**`audit.event` records into the same trail `AuditTrail` holds for the broker's own decisions**,
and `LoadedPlugin::audit()` — and therefore `get audit`, the persisted trail and every export —
carries it. There is no second half of the trail and no second accessor.

Everything that could be a claim stays the host's, and the fixture proves each one by trying to
forge it:

| Field | Whose | Why |
|---|---|---|
| `plugin` | the host's `package_id` | a package that could name another package's id could put a record in somebody else's history |
| `at` | the host clock | `trail.rs` already says it: "a package cannot backdate its own trail" |
| `invocation` | the supervisor's label | it is what makes a trail read as a story |
| `capability` | `audit.event` | the call it arrived through, not a capability that was checked |
| `enforcement` | `Advisory` | the contract's own words: "recorded, audited and shown — and labelled advisory on every surface that shows it" |
| `action` | `audit.event:<the package's word>` | readable, and prefixed so no package-supplied word can pass for one of the broker's |
| `target` | the package's whole event | nothing it wrote is lost, and nothing it wrote is believed to be something else |

**`Enforcement::Advisory` rather than `Broker`** is the load-bearing choice. No capability gates
this call and no policy decision was taken, so `Broker` — "the only level that may be presented as
a security boundary" — would assert a check that never happened. §31.16 forbids exactly that shape
of claim.

**The SDK gains `Ctx::audit_event(event)`**, so a package writes one line rather than assembling
`host_call(method::AUDIT_EVENT, json!({"event": …}))` and getting the envelope wrong. Its
documentation carries the one warning the trail needs: a payload put here is a payload published.

**`Shared::plugin_events` is deleted.** A field nothing reads is not a smaller version of a
feature; it is the appearance of one.

## Consequences

- `get audit --plugin <id>` shows what a package recorded beside what the broker checked, and the
  `enforcement` column says which is which. An operator reading the trail of a provider package
  now sees the credential-plugin invocations and the permission refusals that were previously
  invisible.
- §27.6 of the external-system provider contract becomes satisfiable, and the Kubernetes
  provider's §51.6 with it. That was the finding that prompted this.
- A package can add noise to its own trail. It could before; the difference is that the noise is
  now visible, attributed, and filterable by `capability == "audit.event"`. A trail a package
  cannot write to at all would not be a trail of what the package did.
- `plugin-audit-event.v1.yaml` needs no change: every field a package-originated record fills is
  already in the schema, which is what made this the right shape rather than a new one.

## Alternatives considered

**Add a `recorded_events()` accessor beside `audit()` and leave the vector.** Rejected. Two
half-trails is the failure the trail exists to prevent: an operator asking "what did this package
do" would have to know to ask twice, and the export, the persistence and `get audit` would all
have to learn about the second half.

**Record it with `Enforcement::Broker` so it looks like the rest.** Rejected outright, and §31.16
is explicit about why: a scope that cannot be enforced must not be offered as if it were a
security boundary, and an enforcement level asserting a check that did not happen is the same
falsehood one field over.

**Let the package set `action` unprefixed.** Rejected. `action` is what a reader filters and
scans, and `network.connect` written by a package would sit in the trail beside `network.connect`
written by the broker, indistinguishable except by a column the reader has to remember to check.

**Take the package's timestamp when it supplies one, "because it knows better when it happened".**
Rejected. It is the one field an attacker most wants, the trail's ordering rests on it, and a
package that genuinely needs to record when something happened puts that in its event body, where
it reads as the claim it is.
