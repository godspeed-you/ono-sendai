# ADR-0265: The broker keeps its decisions and its trail

- Status: accepted
- Date: 2026-08-29
- Spec refs: §31.18, §31.19, §31.31, §31.33, §31.37
- Decided by: agent (autonomous, `close-spat`)

## Context

`Host.grants` and `Host.retained_audit` were in-process only. Nothing but package enablement
reached disk, so §31.18's "'Always' grants MUST be inspectable and revocable" was inspectable for
one session and revocable for the same one, §31.19's policy store did not exist, and §31.37's
audit trail answered "what did that package do?" only for as long as nobody had left the shell.

## Decision

**1. The policy store is `<config>/kuang/policy.yaml`** — §31.19's own suggested location, under
the config directory rather than the state directory, because it is a decision the operator makes
and may edit, and because §31.19 wants it "stored separately from plugin packages so package
updates cannot rewrite them".

**2. Only `always` grants are stored.** Everything else "lives only in the session"
(`capability-grant.v1`), and a grant that outlived the session it was made in would be an
`always` grant the operator never asked for.

**3. A stored grant enters a later session as `source: user-policy`.** §31.19's precedence
distinguishes the layer a decision came from, and an operator asking why a package they granted
nothing today holds a capability is asking exactly that question. The stored form is a mapping so
the scope has somewhere to live:

```yaml
plugins:
  dev.example.echo:
    clock.read:
      decision: allow
      scope:
        paths: ["/var/log/**"]
```

§31.19's example writes the bare word `allow`; that form is read too, and the mapping is what is
written.

**4. Revoking rewrites the store.** An `always` grant revoked in any session is gone from the
store, so the next session denies. §31.18 requires revocable, and a revocation that lasts until
the shell exits is not one.

**5. The audit trail is appended to `<state>/kuang/audit.jsonl`**, one JSON object per line — an
append-only trail under state rather than config, because it is a record of what happened and not
a decision anyone edits. It is flushed at the start of every pipeline, so a session that is killed
loses at most its last pipeline, and once more when the session ends. `get audit` answers with what
is on disk followed by what this session has not yet written.

**6. An audit event's identity says which trail it came from.** `ono.plugin-audit-event/1`
declares `identity: [id]`, and the host assembles one stream out of its own events and every
instance's — two counters both starting at one, minting two events that claimed to be the same
one. `AuditTrail::for_source` mixes a stable FNV-1a hash of the source name into the identity's
leading bytes, and the host uses a namespace of its own. The ids stay deterministic, which is what
makes them citable from a finding.

## Consequences

- `Host::configure` now takes the config directory as well, and reads both stores; it is called
  before every pipeline, so a policy file edited by hand between commands is picked up.
- A grant made with `--duration always` survives an upgrade of the package it was made to, which
  is the point of storing it outside the package.
- The audit file grows without bound. Rotating it is not this increment's work and no spec section
  asks for it; it is noted rather than pretended away.
- Encoded by `ono-cli/tests/plugins_missing.rs::should_read_back_an_always_grant_in_a_later_session`,
  `::should_forget_a_stored_grant_when_it_is_revoked_in_a_later_session`,
  `::should_keep_the_audit_trail_across_sessions`,
  `::should_give_every_audit_event_an_identity_of_its_own`, and acceptance case
  `125-kuang-capability-policy`.

## Alternatives considered

- **Storing every grant, with its duration** — a `session` grant read back in another session is
  not the grant that was made.
- **Keeping the trail in the state file the management state already uses** — `management.json` is
  one file per package, rewritten whole; an audit trail is append-only and cross-package.
- **Deduplicating audit events by content instead of fixing the identity** — would have worked and
  would have left `ono.plugin-audit-event/1` emitting two records that its own `identity` says are
  one.
