# ADR-0264: A grant takes the scope and the duration it declares

- Status: accepted
- Date: 2026-08-29
- Spec refs: §31.16, §31.17, §31.18, §31.19, §31.49
- Decided by: agent (autonomous, `close-spat`)

## Context

`docs/contracts/commands/kuang.yaml` declares `grant capability --scope` and `--duration`, `help grant
capability` prints both in the synopsis, and `crates/ono-cli/src/plugins.rs` read neither: the
scope was taken verbatim from the manifest and every grant was minted with `duration: "session"`
and `expires_at`, `max_uses`, `actions`, `selector` and `condition` all null. A declared safety
control that does nothing is the same defect class as a bulk guard that never fires, and it is
worse than an absent one, because the operator believes the boundary is there.

Two questions had to be answered before the options could be read.

**How is a scope written on a command line?** The field is a record and the manifest writes it as
a YAML mapping of key to list. A shell option is a word.

**Which of §31.18's six durations can this broker hold itself to?** §31.18 says grant duration
SHOULD support `once`, `this command`, `this view`, `this session`, `this link session` and
`always for this exact scope`.

## Decision

**1. `--scope <key>=<value>[,<value>]`, repeatable, one key per occurrence.** A grant that bounds
two keys writes `--scope paths=/var/log/** --scope units=nginx.*`. The values are a list because
every §31.16 scope shape is a list.

**2. A key the capability does not declare is refused**, `type.unknown_field` (E0202), naming the
keys it does declare. §31.16: "A key the capability does not declare is invalid, not ignored." A
capability with no scope keys at all cannot be scoped, and says so rather than recording a scope
nothing will check — §31.16 forbids offering a scope that cannot be enforced as if it were a
security boundary.

**3. The operator's scope outranks the package's, key by key.** §31.19's precedence is "system
deny > user deny > **scoped grant** > plugin request > default deny": a scoped grant is above a
plugin request, so `--scope` replaces the manifest's value for the keys it names and leaves the
rest as the package asked. This is not a widening loophole — it is the operator deciding, at the
prompt §31.18 describes, and the decision is recorded on the grant where it can be read back.

**4. `--duration` takes a word this broker can enforce, or a span.**

| Written | Recorded | Enforced by |
|---|---|---|
| *(omitted)* | `session` | the process ending |
| `session` | `session` | the process ending |
| `always` | `always` | the policy store (ADR-0265) |
| a span, e.g. `1h` | `session` + `expires_at` | the broker, on every call |

A span makes a lease (§31.49): `expires_at` is `granted_at + span`, the record carries it, and
`Policy::evaluate` refuses a call past it. `duration` stays `session` for a lease because the
`capability-grant/1` enum has no word for one and everything but `always` "lives only in the
session"; `expires_at` is what makes a lease narrower, and it is on the same record.

**5. `once`, `command`, `view` and `link-session` are refused by name.** Each is bounded by an
event the broker cannot observe from where the grant is made: nothing tells the host that a use,
a command, a view or a link session has ended, so a grant minted with one of those words would
behave exactly like a session grant while claiming to be narrower. The refusal says so and names
what does work. §31.18 says SHOULD; a control that silently does nothing is not a weaker version
of one that works, it is the defect this ADR removes.

**6. An expired lease is its own answer.** `Evaluation::LeaseExpired` and `capability.lease_expired`
(K11303), not `capability.denied` (K11301): "your window closed" and "you were never allowed" are
different facts, and K11303 existed in the taxonomy with nothing raising it. An expired lease
beside a standing grant for the same family does not take the standing grant with it.

## Consequences

- A `--duration` or `--scope` on the command line now changes what the broker permits. The three
  spec durations that are refused stay open work, and the refusal is the honest interface until
  the host can observe the boundary each of them names.
- `Host::grant` takes the duration and the expiry, so every caller states them; `load plugin
  --grant` states `session`/`None` explicitly rather than by omission.
- `standing_grants` excludes an expired lease, so `get capability` stops calling it an allow the
  moment it stops being one.
- Encoded by `ono-kuang-supervisor/src/policy.rs::should_refuse_a_lease_whose_window_has_closed_rather_than_calling_it_ungranted`,
  `::should_allow_a_lease_that_is_still_inside_its_window`,
  `::should_keep_a_standing_grant_answering_after_a_lease_for_the_same_family_has_expired`,
  `ono-cli/tests/plugins_missing.rs::should_record_the_scope_the_operator_named_on_the_grant`,
  `::should_refuse_a_scope_key_the_capability_does_not_declare`,
  `::should_make_a_lease_that_expires_when_a_grant_is_given_a_span`,
  `::should_refuse_a_duration_the_broker_cannot_enforce`, and acceptance case
  `125-kuang-capability-policy`.

## Alternatives considered

- **`--scope` as a JSON object** — matches the stored shape exactly and is unreadable to type.
  Rejected: the option exists to be used at a prompt.
- **Recording `once`/`command`/`view` and enforcing nothing** — precisely the defect being fixed.
- **Counting uses in the host to enforce `once`** — the host only sees a use after the fact, in
  the audit trail; between the grant and the next pipeline the package could use the capability
  any number of times. A control enforced after the fact is not a control.
