# ADR-0640: A start that cannot be satisfied refuses rather than being absorbed by idempotency

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §10.3, §10.4, §10.8, §33, §34; ADR-0610
- Decided by: agent (autonomous)

## Context

§10.8 says one thing about `start recorder`: it "MUST be idempotent". §34 registers two conflict
errors beside it, `temporal.recorder_not_running` and `temporal.recorder_already_running`, and
§10.8 does not say when the second one fires. Read naively the two are in tension: if a start is
idempotent, no start is ever `already_running`, and the error is unreachable.

`docs/contracts/temporal/recorder.yaml` states the reading the contract expects, and it is the one
that makes both true: "starting a running recorder reports the running recorder; it is not
`temporal.recorder_already_running` unless the caller asked for a start that could not be
satisfied."

What makes a start unsatisfiable is not the recorder's state but the settings the start carries.
`start recorder` is the command that applies §10.4's five figures, and a running recorder holds
them. A second start carrying `temporal.retention.max_size 64MiB` against a recorder retaining
512MiB is asking for something that will not happen, and idempotency would swallow the request in
silence — the operator would see a running recorder and a setting that never took effect.

## Decision

`Recorder::start(settings, now)` compares the settings a running recorder is using with the
settings the start carries, setting by setting, over the seven §33 keys the recorder holds:

- **No difference** — the start is absorbed. The answer is the running recorder's status, with the
  `since` it already had. This is §10.8's idempotency, and it is what a login shell running
  `start recorder` in a profile script gets on every session after the first.
- **Any difference** — `temporal.recorder_already_running` (E1314), whose help names every setting
  that differs with both values, and whose `settings` metadata carries their §33 keys as a list.
  The remedy is in the help: `stop recorder` then `start recorder`.

`RecorderSettings::differences` is the comparison, and it returns the list rather than a boolean
so the refusal can name what it refused.

`stop recorder` on a stopped recorder is `temporal.recorder_not_running` (E1313), because there is
no reading under which stopping nothing is what the caller wanted.

A store that cannot be opened is **not** an error from `start`. §44.3 requires that "if migration
cannot complete safely, current shell operation continues with temporal persistent functionality
disabled and an explicit diagnostic", and the same is true of a store that will not open for any
other reason. `StartOutcome` therefore carries the status, the §44.1 plan and an optional
diagnostic, and a start that could not bring persistence up answers with `health: failed`, no
store, the session ledger still in place and the diagnostic set.

## Consequences

The CLI's `start recorder` has three outcomes to render rather than two, and the third —
`StartOutcome::diagnostic` with a stopped, failed status — is the one that must not be rendered as
success. `get recorder` shows `health: failed` afterwards, so the state is inspectable rather than
only reported once.

An operator who changes a setting and re-runs `start recorder` is told plainly, which is the
behaviour §30.1's intent asks for: what is being retained must never be a surprise.

Tests: `crates/ono-recorder/tests/lifecycle.rs` — the idempotent repeat, the refusal that names
`temporal.retention.max_size`, the `temporal.recorder_not_running` stop, and the store that cannot
be opened.

## Alternatives considered

- **Apply the new settings to the running recorder.** Rejected: retention that changed under a
  running recorder would make §33's "changing retention MUST NOT retroactively resurrect expired
  data" a question about when the change landed, and a checkpoint interval that changed mid-run
  would make the cadence unreconstructable from the ledger.
- **Ignore the settings and report the running recorder.** Rejected: it is the silence §10.8's
  idempotency was never meant to license, and it leaves E1314 unreachable.
- **Refuse every second start.** Rejected: it is `start recorder` in a profile script failing on
  the second login, which is exactly what §10.8 forbids.
