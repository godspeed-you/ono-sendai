# ADR-0474: The audit record is written where the decision is made, and has nowhere to put a payload

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §14.1, §14.2, §53.3, §54.1, §54.2, §56.1, §56.2; ADR-0472, ADR-0473
- Decided by: agent (autonomous)

## Context

§14.1 lists eight event classes a listening agent must produce, §14.2 lists the fields, and §14.2
ends with the constraint that shapes everything: events "MUST NOT include private keys, full secret
environment values or unredacted credentials from provider payloads."

Seven of the eight things §14.1 names happen inside `ono_protocol::serve` — the accept, the two
refusals, the denial, the protocol mismatch, the disconnect, the action. One happens above it: a
TLS handshake that fails never reaches the protocol at all.

## Decision

**`AuditEvent` and `AuditSink` live in `ono-protocol`, beside the loop that makes the decisions;
the sink that writes to stderr lives in `ono-remote`.** The decision and its record cannot drift
apart if they are produced in the same statement, and the refusal an operator reads in the log
carries the same `ErrorCode` the client received. Where the events *go* is the listening agent's
business, which is §56.1's, so `StderrAudit` is there and `ServerConfig::with_audit` is how they
meet.

**`AuditKind` is a closed enum of §14.1's eight, in §14.1's order.** An agent that meets a ninth
kind of event has met something §14.1 did not anticipate, and the honest response is to add a
variant rather than to file it under a neighbour.

**The record has no field a payload could occupy.** `connection_id`, `peer_fingerprint`,
`peer_label`, `source_address`, `protocol_version`, `requested_capability`, `result`, `error_code`,
`timestamp` — §14.2's list, and nothing else. There is no message, no detail and no value. That is
what makes the redaction test meaningful: it greps the whole audit stream for a credential the
caller sent in an action argument and for the values the provider produced, and neither could have
got there, because there is nowhere for either to go. A record that *could* quote a value would
eventually quote a password.

**One line, fixed shape, `key=value`, on stderr.** A field with nothing in it is written `-` rather
than omitted, so the shape of a line does not depend on what happened and `cut -d' ' -f4` means the
same thing on every line. Stderr rather than stdout because stdout is the wire when the agent is
carried over stdio, and §54.2 forbids making an operator enable debug logging to see a policy
decision — so this is on by default for `--agent --listen`, not behind a flag.

**`ConnectionLimitDenied` is declared and never emitted in H2.** The connection semaphore is phase
H3 (§12.1, §12.3), so the class exists in the closed set with nothing to raise it yet. Declaring
it now rather than later keeps the set matching §14.1, and H3 raises it without touching the type.

**`source_address` is reported by the socket and grants nothing.** §65.2 forbids granting on a
source address; it is here so an operator correlating a log has it, and the authorization context
has no field for it at all (ADR-0470).

## Consequences

Easy: `ono --agent --listen … 2>&1 | grep event=authorization.denied` is an operator's answer to
"who was refused what", with the fingerprint to add and the capability to grant on the same line.

Hard: writing an audit line happens on the connection's own task, so a slow sink slows a
connection. `StderrAudit` writes and never blocks on anything else, and a failure to write is
swallowed — the operator loses a line, and the decision the line described has already been
enforced. A sink that ships events somewhere is a later problem and would want a queue.

Also: seven classes are proved end to end and the eighth, `ClientVerificationFailed`, is emitted by
the listener when `accept` fails as well as by `serve` when a policed connection proved no key.
Driving a genuine TLS verification failure through the acceptance container needs a client that
presents a malformed certificate, which `crates/ono-remote/tests/client_authentication.rs` already
does at unit level (ADR-0437); the acceptance case drives the classes a shell can reach.

Encoded by: `crates/ono-remote/tests/audit.rs::should_emit_a_structured_event_for_every_connection_lifecycle_step`,
`::should_record_the_refusal_of_a_client_nobody_authorized`,
`::should_record_a_client_that_proved_no_key_as_a_verification_failure`,
`::should_carry_the_fingerprint_and_the_decision_on_every_authorization_event`,
`::should_never_write_key_material_or_payload_bytes_into_an_audit_event`,
`::should_name_every_event_class_the_specification_lists`,
`crates/ono-cli/tests/authenticated_link.rs::should_write_a_structured_audit_line_for_every_decision_the_agent_makes`,
case `184`.

## Alternatives considered

**A `tracing` subscriber.** The ecosystem's answer, and it would give an operator filtering for
free. Rejected: it would make the audit trail a logging concern with a level, and §54.2 is
explicit that a user "must not need `RUST_LOG=debug` to understand why a security policy denied
them". An audit event is a product output, not a diagnostic.

**JSON lines.** Easier to parse, harder to read over a shoulder. Rejected for now because the
fields are flat, fixed and never contain a value that could need quoting — which is exactly the
property that makes `key=value` safe here and would stop being true the moment a field carried a
payload, which is the thing this record must never do.
