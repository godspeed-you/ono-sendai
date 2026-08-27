# ADR-0076: The implicit selector — what a frame fills in, and where

- Status: accepted
- Date: 2026-08-27
- Spec refs: §14.3, §14.5, §27.1; ADR-0023, ADR-0075
- Decided by: agent (autonomous)

## Context

ADR-0023 fixed that a frame contributes implicit selectors and nothing else, and that every
contribution has an explicit spelling. The implementation of that rule lived in one place — the
generic producer added a `Selector::field` to its provider query for each object frame — and so
`get process` inside `enter process 1` asked for `name == 1` (nothing), `trace process` inside
the same frame still demanded a pid, `watch` reimplemented the rule, and `get process` inside
`enter user root` asked the provider for `user == "root"`, which the provider then silently
ignored (STATE.md: "provider options are silently ignored"). Spec §14.5 says the same query
must be expressible without the context; the contexts were producing queries no command could
spell.

## Decision

### 1. The frame fills in arguments, once, at the command table

`CommandTable::run` — the one entry every native implementation runs through — asks
`ono_command::narrow` what the object frames in force contribute to the invocation's bound
arguments, and runs the implementation over the amended arguments. A producer, a trace, a
watch and a mutation therefore see the same narrowed arguments, and `contract.query(arguments)`
carries the contribution to the provider without any implementation knowing a context exists.
Only a command with a target *and* a provider capability is narrowed: `get context`, `help`
and every transform mean the same in every frame.

### 2. What a frame contributes to a command

For each object frame, outermost first:

1. **The command's own target** (`get process` inside `enter process 1`): the first declared
   selector, then the first declared option, that the frame has a handle for (ADR-0075 §4) —
   `pid 1`, `name lo`, `target /`, `path …`, `--port 443`. Where no parameter fits, an ambient
   selector on each schema identity field the frame has a handle for.
2. **Another target with a parameter named after the frame's target** (`get process` inside
   `enter user root`, `get route` inside `enter interface lo`): that selector or option, with the
   frame's identity — `--user root`, `--interface lo`.
3. **Another target whose answering schema has a field named after the frame's target**: an
   ambient selector on that field with the frame's identity, as before (`enter service nginx`;
   `get process`).
4. Otherwise `resolve.target_not_found` naming the frame and the field it would have needed —
   spec §14.3's prohibition on widening, unchanged from ADR-0023.

**What was typed wins.** A parameter the user wrote is never overridden by a frame: `get process
5` inside `enter process 1` is `get process 5`. A frame supplies what was not written.

### 3. Ambient selectors are a separate slot

`BoundArguments::ambient()` holds the case-1-fallback and case-3 contributions. They reach the
provider as `Selector::field`s through `contract.query` and are invisible to an implementation
reading its declared parameters — `require_selector`, `option`, `flag` — because they are not
parameters the contract declares.

### 4. Every option a frame can spell must be honoured

Case 2 turns a frame into an option, so an option a provider ignores is a frame that widens
silently. This increment adds `ono.process.get --group` and honours `--user`/`--group` in the
procfs provider (by name or numeric id, against the reference the record carries) and adds
`ono.route.get --interface`, which the netlink provider already honoured; `--port` on
`trace socket` is honoured by the netlink provider as the selector of the same name. The
remaining declared-but-ignored options are the STATE.md item they always were.

### 5. `watch` keeps a query-level form for now

`impls/watch.rs` composes `producer::ambient_selector` into its subscription query. That
function stays, narrowing by the frame's identity-field handle for the entered target and by
schema field otherwise, so `watch` behaves as before this ADR; migrating it onto the argument
seam is the watch family's increment.

## Consequences

Easy: `trace process` and `trace socket` needed no change to work inside a frame; a new
command with a `pid` selector is narrowed by a process frame on the day it is registered.
`explain` can print the explicit spelling by reading the narrowed arguments.

Hard: a frame's explicit spelling now depends on the command (`pid 1` for `get process`,
`--port 443` for `trace socket`); `ono.context/1`'s `selector` column keeps the cross-target
spelling `--<target> <identity>`, which is the one every other target's query uses.

Encoded by: `should_narrow_get_process_to_the_entered_process`,
`should_trace_the_entered_process_without_a_selector` (processes_missing.rs);
`should_narrow_processes_to_the_entered_user`, `should_narrow_processes_to_the_entered_group`
(identity_missing.rs); `should_narrow_routes_to_the_entered_interface`,
`should_trace_the_entered_socket_without_a_selector` (network_missing.rs);
`should_narrow_get_mount_to_the_entered_mount` (storage_missing.rs); the producer tests
`should_narrow_a_producer_with_the_ambient_selector_of_a_context_frame` and
`should_refuse_a_query_the_context_cannot_narrow_rather_than_widening` (ono-command).

## Alternatives considered

- **Keep narrowing in each implementation's query** — rejected: three copies already disagreed,
  and `trace`'s "the selector is mandatory" had no way to learn about a frame.
- **Narrow in the shell before binding** — rejected: the library's `Invocation::with_context`
  would then narrow nothing, and every host (the test fixture, a KUANG/11 test host) would have
  to repeat the rule.
- **Let the frame override an explicit parameter** — rejected: "override" makes the effect of
  a typed selector depend on state the user cannot see, the failure mode ADR-0023 exists to
  prevent.
