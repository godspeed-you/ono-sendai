# ADR-0467: An authorization update is a rename, or it did not happen

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §9.2, §9.8, §55.3; ADR-0355, ADR-0435, ADR-0466
- Decided by: agent (autonomous)

## Context

§9.8: "authorization-store changes MUST be written atomically using write-to-temporary, fsync,
rename and directory sync where supported by the existing persistence conventions. A failed update
MUST leave the previous valid store intact."

The requirement has teeth here that it does not have for most files, because of ADR-0466's strict
parser: a half-written `authorized_clients` is not a store with some entries missing, it is a store
that refuses to load, and a listening agent that cannot load its policy stops serving. The failure
mode of a torn write is the whole agent, not one grant.

The repository already had two nearby conventions and they disagreed. `TrustStore::persist` writes
a temporary, fsyncs it and renames — no directory sync, no mode. `ono-cli`'s identity migration
(ADR-0435) creates its temporary with `create_new` and mode `0600`, because it carries a private
key.

## Decision

**`ono_protocol::write_store` is the full §9.8 sequence, and the authorization store gets the
stricter of the two neighbouring conventions.**

1. `create_new` a temporary beside the store, with mode `0600`;
2. write, `sync_all`;
3. `rename` over the store;
4. open the parent directory and `sync_all` it, ignoring a filesystem that will not.

`create_new` rather than truncate: a leftover temporary from a crashed update is refused rather
than reused, so two updates cannot interleave into one file. A failed rename removes the temporary
it made, because a temporary that outlived its update would refuse the next one.

**Mode `0600`, and it survives an update.** The file is the operator's decision about who may
reach this machine, and an account that can rewrite it can authorize itself. Creating the
temporary with an explicit mode rather than relying on the umask is what makes the second update
as private as the first — a rename carries the temporary's mode, so a `0644` umask would silently
widen the file on every change.

**The directory sync is best effort.** Not every filesystem supports syncing a directory handle,
and one that does not is not a reason to fail an update that has already happened. §9.8's "where
supported" is doing exactly this work.

## Consequences

Easy: a reader that opens the path at any moment sees one whole store or the other. The proof is
an outcome test rather than a fault injector — the update is driven while the store is read back
between every step, and no temporary is ever found beside it.

Hard: proving "a failed update leaves the previous store intact" needs a failure that is real. The
test makes the configuration directory mode `0500` so the temporary cannot be created, which is
the point in the sequence where a truncate-in-place implementation would already have destroyed
the file, and then compares the store byte for byte.

Also: the trust store of ADR-0355 still has the weaker sequence. That is not this increment's to
change (AGENTS.md §4) and is recorded in `docs/STATE.md` under *Found, not yet filed*.

Encoded by: `crates/ono-cli/tests/authorized_clients.rs::should_replace_the_store_atomically_so_a_reader_never_sees_a_partial_file`,
`::should_leave_the_previous_store_intact_when_a_write_is_interrupted`,
`::should_keep_the_owner_only_permissions_of_the_store_across_an_update`.

## Alternatives considered

**Reuse `TrustStore::persist`.** One writer for both stores. Rejected: it has no mode and no
directory sync, and widening it to have them would change the trust store's behaviour inside a
change about authorization, which §4 of AGENTS.md separates.

**A lock file.** Would make concurrent updates from two shells safe rather than merely
non-destructive. Rejected as speculative: nothing in the repository takes a lock for a
configuration file, `create_new` already refuses an interleaved write, and the requirement §9.8
states is atomicity, not serialisation.
