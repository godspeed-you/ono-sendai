# ADR-0209: A null a provider left is not an empty exit

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.17, §32.1, §32.2, §35.2, §42.4, §45.2; v0.2 §35.3; AGENTS.md §6
- Decided by: agent (autonomous, `S11c`)

## Context

`docs/dogfood/v0.4-2026-08-28.md` finding 2. Standing at a listener owned by another user,
`look` printed

```text
 exits
   process        0
```

The socket record's `process` field is null, and the spatial layer turned that null into a
neighbourhood group with `count: 0, state: "empty"`. `empty` is defined in §35.2 as "the objects
were read and there are none", and none of that happened: nobody looked. §35.2's own
counter-example is this exact rendering, and invariant §2.17 forbids it outright — "Missing
permission, unsupported provider data and uncertainty MUST not be rendered as absence."

Two separate mechanisms fed that zero, and each was wrong on its own.

**The spatial layer claimed the record had answered.** `crates/ono-cli/src/spatial/relations.rs`
ends `observe` by marking every *composed* exit of the place — the exits a record fills from its
own fields through the provider bridge (§45.2) — as answered, before deciding which exits are
left to be reported as `unknown` or `unsupported`. For a socket the composed exits are `process`,
`peer`, `connections` and `listener`. So `process` was claimed as answered by a field that said
nothing, and the relationship provider that actually serves that exit —
`linux.socket-owners` — never got its state recorded. That provider is declined on a default
`look` on purpose: joining a socket to its holder means reading every `/proc/<pid>/fd` on the
host, which is `expensive` in §32.1's cost classes, and §32.1 says a default `look` MUST avoid
it. The decline is right; losing it is the defect.

**The provider threw away the reason it had no owner.** `SocketOwners::from_proc_root` skips a
process whose descriptor directory it may not read, and then reports an unattributed inode
exactly as it reports an inode nobody holds: `process: null`. But those are different facts, the
provider is the only component that knows which one occurred (§2.16 — providers own facts), and
the kernel handed out the inode, so somebody holds the socket.

## Decision

**A reference field a provider left null is not a statement that the exit is empty.** Two rules
follow, one on each side of the provider boundary.

1. **A provider that knows why it has no answer says so in the field.** Where the owner scan ran
   and was refused at least one process's descriptors, `ono.socket/1`'s `process` carries that
   refusal as an `io.permission_denied` error value instead of a null; where the scan saw every
   process and none held the socket, the field stays null, because that is an answer. Where no
   scan was asked for, the field stays null, because nobody looked. `SocketOwners` therefore
   counts the processes that refused it, and `SocketOwners::refusal()` is the error an
   unattributed inode reports. The schema's own documentation of the field states all three.
   The bridge already turns an error-valued reference field into a withheld group with the state
   `PermissionState::of_refusal` maps it to, so `permission_denied` reaches the place view
   without another rule.

2. **A composed exit is answered only where the record stated it.** `observe` now marks a
   composed exit as answered unless a relationship provider serving the same exit was declined
   for cost *and* nothing has been stated for it — no edge under that exit, no recorded state.
   Such an exit falls through to the answer §32.2 prescribes for an expensive relationship
   nobody has paid for: `unknown — available on request`.

   "Stated" is read in the vocabulary of the **group** — the word `look` prints and
   `SpatialIndex::record_withheld` is keyed by — never the `follow` label, because an edge's two
   ends carry two labels (`socket` and `owner`) and one group each (`sockets` and `process`).
   The first draft compared groups against labels, so an owner the session had *already*
   observed was overwritten with "available on request"; §32.1's own exception is "unless cached
   or already available", and the container caught the difference in
   `docker/acceptance/cases/091-spatial-unknown-web-service.case` `44.2m` and
   `docker/acceptance/cases/094-spatial-network-path.case` `44.5g`.

The condition is deliberately narrow. A listener's `peer` is null because the kernel said
`0.0.0.0:0`, meaning there is no peer; no relationship provider serves that exit, nothing was
declined, and it keeps reporting `0`. Only an exit whose real answer lies behind a scan this
view refused to spend becomes `unknown`.

## Consequences

- At a listener reached cold, a default `look` now prints
  `process   unknown — available on request` — for another user's socket and for one's own
  alike, because in neither case was anything read. `look --all` and `near process` spend the
  scan and answer with the owner, or with `permission denied — N process(es) did not let their
  open files be read`. A listener reached *through* the process holding it keeps naming its
  owner, because that edge is already in the index and §32.1 excepts what is cached.
- §32.1 is not weakened to reach the more informative answer. A default `look` that resolved
  socket owners would be a whole-`/proc` scan per place, which is the cost class the spec names
  and forbids; the honest reading of §32.2 is that the exit is *unloaded*, not empty, and the
  user is told how to load it.
- `get socket --process` is honest for every socket in the dump, not only the ones the caller
  happens to own.
- The tests that encode it:
  `crates/ono-cli/tests/spatial_relationships_missing.rs::should_not_report_the_owner_of_a_socket_nobody_looked_up_as_no_owner`,
  `crates/ono-provider-netlink/tests/socket_process_join.rs::should_say_the_owner_is_denied_when_the_scan_was_refused_a_process`
  and `::should_leave_the_owner_null_when_a_complete_scan_found_nobody_holding_it`.
- ADR-0203's T17 row is extended with the first of those: the row's existing tests all exercise
  a provider that *stated* a refusal, and none exercised a provider that answered `null`.

## Alternatives considered

- **Let a default `look` resolve socket owners, so it can say `permission_denied` outright.**
  Rejected: it is precisely the expensive relationship §32.1 tells a default `look` to avoid,
  and it would pay a full procfs scan every time a user steps into a socket.
- **Infer the refusal from the socket's owning uid**, which `sock_diag` reports without any
  scan: a socket owned by uid 113 is held by a process this uid 1000 reader cannot inspect.
  Rejected: the holder is not necessarily the creator (`fork` after `setuid`, `SCM_RIGHTS`), so
  this is the spatial layer inventing a fact the provider never stated — the second source of
  truth §2.16 forbids.
- **Treat every null reference field as `unknown`.** Rejected: it would report a listener's
  absent peer as unknown, which is a lie in the other direction. A null whose meaning is "none"
  belongs to the provider that wrote it, and only the exits an unasked provider serves are in
  doubt.
