# ADR-1041: A store that decides trust is replaced one way, under one lock

- Status: accepted
- Date: 2026-11-24
- Spec refs: spec §21.5, §49; v0.4.1 §9.2, §9.8, §55.3; ADR-0015 (T5, T6, F16), ADR-0355,
  ADR-0466, ADR-0467
- Issues: #195
- Decided by: agent (autonomous)

## Context

Two files in the configuration directory decide who this machine trusts. `trusted_hosts`
(ADR-0355) is which hosts this shell will link to; `authorized_clients` (ADR-0466) is which
clients its listening agent serves. An account that chooses either file's contents chooses who is
trusted.

ADR-0467 gave the authorization store §9.8's full sequence in `ono_protocol::write_store` and left
the trust store's writer, `TrustStore::persist`, as it was: `File::create` on a fixed
`<store>.tmp`, write, fsync, rename — no mode, no directory sync, and a leftover temporary
*truncated and written through*. That last part is accepted risk F16 of the 2026-08-26 security
review: a predictable temporary opened without `O_EXCL` follows a link planted at its name, so an
update writes the store's contents into whatever the link points at and then renames the link over
the store.

Two more gaps sat beside it, in both stores:

- **Lost updates.** A `TrustStore` is opened once and kept; every update rewrote the file from the
  handle's in-memory copy. Two handles opened before either wrote — two shells, or a shell and an
  agent — each wrote a store without the other's change. That loses a pin (safe), but it also
  *restores* a host another shell had just forgotten with `remove host-key`, which re-trusts a key
  somebody deliberately stopped trusting. The CLI's read-modify-write of `authorized_clients` had
  the same shape and could bring back a revoked client.
- **`write_store`'s leftover handling** removed `<store>.tmp` and then created it, which refuses
  an interleaved creation but not an interleaved *rename*: a concurrent writer still holding the
  unlinked temporary open renames whatever is at the name now — the other writer's half-written
  file — over the store.

ADR-0467 considered a lock and rejected it as speculative, because nothing yet needed
serialisation. Issue #195's exit asks both writers to survive the same interrupted-write proof,
and the run brief asks for concurrent writers to be considered; the lost re-trust above is a
security consequence, not a tidiness one.

## Decision

**One writer for both stores, `ono_protocol`'s private `store_file` module, and every update is a
read-modify-write under an exclusive lock.**

1. **Lock.** An update takes `flock(LOCK_EX)` on the *directory* the store lives in (std's
   `File::lock`), held from before it reads the store until after the rename. The directory rather
   than a lock file, so the directory gains no file and there is no second predictable path to
   plant a link at. The lock is advisory and orders Ono's own updates, which is all it is for; the
   two stores share a directory and therefore a lock, and no code path holds one while taking the
   other.
2. **Read under the lock.** The update re-reads the store from the file — with the same strict
   parser as `open`, so a store that cannot be read or does not parse refuses the update and is
   left exactly as it was — and applies its change to *that*, not to a copy taken earlier. A
   `TrustStore` handle's in-memory entries are refreshed from the result.
3. **Replace (§9.8).** A temporary `<store>.tmp`: whatever is at that name is unlinked (which
   never follows a link) — under the lock it can only be the remains of an update that died — and
   the temporary is created `O_CREAT|O_EXCL` with mode `0600`, written, `fsync`ed, renamed over the
   store, and the directory synced. A failure at any step leaves the previous store byte for byte
   and removes the temporary.

The public surface: `TrustStore`'s methods keep their signatures; `ono_protocol::update_store(path,
change)` is new, for the authorization store's read-modify-write, and `write_store` keeps its
signature and now takes the lock too. The CLI's `add`/`set`/`remove client-key` go through
`update_store`.

A store that cannot be read is never an empty store. `TrustStore::open` already refused one; an
update now refuses too instead of writing its stale copy over a file it could not see.

### F16

Closed for the trust store: its temporary is no longer opened without `O_EXCL`, and a planted
link at it is unlinked rather than followed. Still open, and not this issue's: the history
rewrite's `history.jsonl.new` (`crates/ono-history/src/store.rs`). What no writer can close stays
accepted: an account that can write the configuration *directory* can replace the store itself.

## Consequences

Both writers survive the same proof, one table over both stores in
`crates/ono-protocol/tests/store_writers.rs`: owner-only creation whatever the umask
(`should_create_the_store_owner_only_whatever_the_umask_allows`), a half-written temporary neither
trusted nor reused (`should_neither_trust_nor_reuse_a_temporary_an_interrupted_update_left_behind`),
a planted link never written through
(`should_never_write_through_a_temporary_that_is_a_link_to_somewhere_else`), a failed update
leaving the previous store intact
(`should_leave_the_previous_store_intact_when_the_temporary_cannot_be_created`), and concurrent
updates all landing (`should_keep_both_updates_when_two_writers_started_from_the_same_store`).
The trust store's own handle: `should_not_undo_a_pin_another_handle_wrote_since_this_one_was_opened`,
`should_not_bring_back_a_host_another_handle_forgot`,
`should_refuse_to_pin_rather_than_overwrite_a_store_it_cannot_read`.

A pin of a host the handle already has cached as pinned to the presented key still answers without
touching the file — the common case of every link stays I/O-free. Any other decision (an unknown
host, a mismatch) is taken against the file as it is now, so a host re-pinned by another shell is
not refused on a stale copy.

An update blocks while another update of the same directory is in progress. Updates are a few
hundred bytes and one rename; nothing holds the lock across a network round trip.

`remove host-key` of a host that is not pinned no longer rewrites the file (there was nothing to
write); `forget` on the library likewise writes only when it removed something.

## Alternatives considered

**A lock file beside the store.** The usual shape. It leaves a file in the directory that the
existing "no leftovers beside the store" case would have to learn about, and is one more
predictable path in a directory F16 already worries about.

**Unique temporary names (`<store>.tmp.<pid>.<n>`) and no lock.** Removes the shared temporary,
but not the lost update, which is the half with a security consequence; and crashed writers would
leave temporaries nobody cleans.

**Keep the cached copy and compare-and-swap on the file's modification time.** Second-resolution
timestamps make it unsound, and it turns a concurrent update into a failure the user has to retry
rather than an update that waits a millisecond.
