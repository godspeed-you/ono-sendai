# ADR-0226: One label rule for an object — the schema's own short form

- Status: accepted
- Date: 2026-08-29
- Spec refs: §11.5, §22.4, §27.3, §33.1
- Decided by: agent (autonomous, `close-data`)
- Supersedes: ADR-0224

## Context

ADR-0224 closed the split ADR-0116 left open — two label rules for one object — by choosing
`ObjectRef::of`'s: the first default-view column outside the identity. On the two objects it was
tested against that read well (`ono.mount/1[/] /dev/sda2`, `…[1, …] systemd`). On a third it does
not: `ono.socket/1`'s identity is an inode and its first non-identity column is `protocol`, so a
stopped socket rendered as

```text
ono.socket/1[192401674] tcp
```

— a label that names a kind, not an instance, and drops the endpoint that says *which* socket.
`network_missing.rs::should_report_a_permission_failure_when_stopping_a_socket_unprivileged_with_confirm`
asserts spec §11.5's requirement that the row reference the socket it acted on, and it caught
this. (It caught it one gate late: the gate run for ADR-0224 aborted at an earlier crate and
never reached that test. A gate that stops early has verified only what it ran.)

The generic rule cannot know which column identifies. The per-schema rule already did: it is
`ono_graph::label_of`, written from spec §22.4's own examples — `nginx.service`,
`process/921 nginx`, `tcp/:443`, `/etc/nginx/nginx.conf`.

## Decision

**The label of an object is the short form its schema declares, and there is one implementation
of it.** That table moves from `ono-graph` to `ono-provider-api`, beside `ObjectId` and
`ObjectRef`, as `declared_label`; `ObjectRef::of` uses it, and `ono_graph::label_of` is it. A
graph node, an `ActionResult` target and a provider's resolved reference call an object the same
thing.

**A schema that declares no short form falls back to what suits the place the label is read.**
Spec §22.4 gives a form for the objects it draws; a plugin's schema, an adapter's, a remote
fixture's has none. A graph node stands alone, so it falls back to `<kind>/<identity>`. An
object reference is printed beside its identity, so it falls back to the first default-view
column outside the identity — the thing the identity does not show. The fallback is where the
two readers may differ, because the two renderings differ; the declared form is where they may
not, because it is what the object is called.

ADR-0224's aim stands — one rule, both branches of `ProviderMutation::targets` build an
`ObjectRef` — and only the rule itself is replaced.

ADR-0116 §1 still leaves the label off when it only repeats the identity, so `ono.mount/1[/]`
stands alone while `ono.socket/1[192401674] tcp/127.0.0.1:45801` does not.

## Consequences

- Both spellings of every mutation render the same target; the socket keeps its endpoint, the
  mount stays `ono.mount/1[/]`, the process reads `process/1 systemd`.
- `ono-provider-api` now owns "what an object is called", which is where `ObjectRef` already
  said it belonged: "an object's identity together with enough of it to show a person which one
  it is". `endpoint_text` and `endpoint_label` become public there for the same reason.
- A file is labelled by its path, as spec §22.4 draws it, rather than by its base name.
  `ono-provider-linux/tests/file.rs::should_resolve_a_path_to_one_object_reference` asserted the
  base name and now asserts the path — the same exactness, the spelling this decision gives it.
  Wherever the label is printed beside the identity, ADR-0116 §1 leaves it off, because for a
  file the identity *is* the path.
- The assertions ADR-0224 adjusted stay adjusted: `storage_missing.rs` asserts the identity as
  the target's prefix (as its sibling test already did) and acceptance case 042 checks the prefix
  and that the two spellings agree. Both hold under this rule too, and the agreement test is
  what would have caught either rule diverging.

## Alternatives considered

- **Refine the generic rule to skip enum columns.** Rejected: it would answer `local` for a
  socket, which is a record with no canonical text, and the label would fall back to the
  identity. Each patch of the generic rule is a schema's short form being rediscovered badly.
- **Keep both rules and adjust the socket test.** Rejected outright: the test asserts spec
  §11.5, and the behaviour it was asserting was correct.
