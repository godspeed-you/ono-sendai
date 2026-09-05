# ADR-0075: `enter` is delivered for every target a provider serves

- Status: accepted
- Date: 2026-08-27
- Spec refs: §14.1, §14.3, §14.5, §52; ADR-0023, ADR-0076
- Decided by: agent (autonomous)

## Context

Spec §14.1 gives `enter` one job — push a frame — and §14.3 gives the frame one meaning — an
implicit selector for later commands. The registry declared `enter` for process, user, group,
interface, socket, mount and file with `stability: planned` and `validation_required: true`,
because spec §52 marks those matrix cells `C?`: a context whose usefulness is to be validated
first. `crates/ono-cli/src/context.rs::enter_object` read the marker as "not delivered" and
refused every one of them, resolved the object by a `name` selector whatever the target's
handle actually is (a pid, a mount point, a path), and knew no way for the object to arrive
through the pipeline although every contract's `input` says `null | ono.<target>/1` and
`ono.socket.enter` was documented only that way.

The RED suites of `crates/ono-cli/tests/{processes,identity,network,storage,files}_missing.rs`
assert the delivered behaviour. This ADR records the decisions they required.

## Decision

### 1. The `planned` marker gates a command's implementation, not the context stack

`enter <target>` is not a command a provider implements; it is the shell's, and its whole
mechanism — resolve the object, push a frame, narrow later queries (ADR-0076) — is generic over
the registry. Validation of a `C?` cell therefore does not wait for code that will never exist:
**entering is delivered for every target whose provider answers `get <target>`.** The seven
contracts move to `stability: stable`; `ono.container.enter` stays planned because nothing
serves `container` yet, and `enter_object` no longer consults stability at all — a target with
no provider fails through the provider registry's own `provider.unavailable`, which names the
real reason.

### 2. The object is named the way its `enter` contract declares

`enter <target> <word>` binds the word against the contract's selectors exactly as `get
<target> <word>` does — `pid` for a process, `path` for a file, `target` for a mount, `name` for
a user, group, interface or service, `port` for a socket — and the bound query goes to the
provider. The first record that answers is the object; nothing answering is
`resolve.target_not_found` naming what was asked, and a provider failure (a pid that is not in
`/proc`) is that failure. A refused `enter` pushes nothing.

`ono.socket.enter` gains the `port` selector so the words form exists at all (`enter socket
443`, the spelling spec §22.3 uses for `trace socket --port 443`); the piped form is unchanged.

### 3. `… | enter <target>` takes the object from the pipeline

A pipeline whose last stage is `enter <target>` runs its head as the native pipeline it is,
keeps the result instead of showing it (`Session::begin_capture`, `native::run_collecting` —
a result that was never shown is not retained for `@-1` either), and enters the first
`ono.<target>/1` record that arrived. Anything else — nothing arrived, a record of another
schema — is `resolve.target_not_found` with the spelling that would work.

### 4. What the frame remembers

An object frame carries the identity the prompt shows and the **handles** the object answers to
(ADR-0076): every scalar field of the record by name, plus the scalar fields of its structural
sub-records under their own names where the top level claims nothing of that name, so a
socket's `local.port` is the handle `port`.

The identity is the value of the record's field named after the `enter` contract's first
selector — the object's own spelling of the handle the user gave, which is why `enter service
nginx` still shows `nginx.service`. Where that field lives inside a sub-record, the whole
sub-record is the identity, rendered as its known scalar fields joined by `:` —
`127.0.0.1:443` for a socket, never a bare port. A target entered only through the pipeline with
no selector at all shows its first schema identity field. `get context` renders every identity
as text, which is what `ono.context/1` declares.

## Consequences

Easy: every navigable object of spec §14 can be entered today, by word or by pipe, and the
frame's identity is what a user would type to get the object back. `enter` gains no per-target
code; a new provider target is enterable the day its `get` works.

Hard: `enter socket 443` enters the *first* socket on the port, as `trace socket --port 443`
traces the first; a user who means one connection pipes it in (`get connection | where … |
enter socket`).

Encoded by: `should_push_an_object_frame_when_entering_a_process`,
`should_refuse_to_enter_a_process_that_does_not_exist` (processes_missing.rs);
`should_enter_a_user_and_show_it_on_the_context_stack`,
`should_keep_the_stack_unchanged_when_entering_a_user_fails` (identity_missing.rs);
`should_push_a_socket_frame_when_entering_the_listening_socket` (network_missing.rs);
`should_keep_the_working_directory_when_entering_a_mount` (storage_missing.rs);
`should_push_a_file_frame_when_entering_a_file` (files_missing.rs).

## Alternatives considered

- **Keep the seven contracts `planned` and refuse `enter`** — rejected: the marker would be
  gating a mechanism that already exists for `service`, and §52's "validate first" is answered
  by delivering it and watching, not by withholding it.
- **A per-target identity table in the shell** — rejected: the contract's selector already says
  what a user types to name the object; a second table would drift from it.
- **Print the head's result and enter its first row** — rejected: `get socket 443 | enter
  socket` is navigation, and a table the user did not ask for in the middle of it is noise the
  next `to json` would also have to skip.
