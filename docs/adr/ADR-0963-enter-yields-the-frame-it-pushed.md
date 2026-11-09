# ADR-0963: `enter <target>` yields the frame it pushed

- Status: accepted
- Date: 2026-11-09
- Spec refs: spec §14.1, §14.3, §14.5, §29.4; ADR-0023, ADR-0075, ADR-0118, ADR-0935
- Decided by: agent (autonomous)

## Context

Twelve `enter <target>` commands — `user`, `group`, `interface`, `socket`, `process`, `service`,
`mount`, `file`, `dir`, `container`, `link` and their contracts — declare `output: ono.context/1`.
The shell ran them itself (ADR-0023: a frame is session state, like `cd`), and only in two shapes:
as a statement of their own, and as the *last* stage of a pipeline whose earlier stages supplied
the object (`get socket 443 | enter socket`, ADR-0075). Anywhere the declared value was consumed —
`enter user root | to json`, `$f = (enter user root)` — the stage reached the command registry,
which binds no implementation to the id and answered `resolve.command_not_found` "declared but
this build implements nothing for it". The v0.6.3 command conformance suite (ADR-0935) found it
and had to exempt seven examples (#290).

Either the contract or the runtime was wrong. The contract is right: a frame is a value — `get
context` already streams the stack as `ono.context/1` records — and a command that changes the
session may still answer with what it changed, exactly as `link host` and `load plugin` do when
reached through a pipe (ADR-0118).

## Decision

**An `enter <target>` inside a pipeline yields the one frame it pushed**, as the `ono.context/1`
record `get context` would show on top of the stack after it: its `depth` is the stack's new
depth, `kind`, `target`, `identity` and `selector` are the frame's. The stages after it read that
value like any other; the frame stays pushed, because yielding it does not make `enter` a query.

Both spellings of spec §14.3 behave the same: `enter user root | …` runs the head form and seeds
the rest with the frame; `get user root | enter user | …` collects the prefix, enters the object
it produced, and seeds the rest the same way. `enter` as the last stage pushes and yields nothing
to nobody, as before. A spatial move that pushes no frame — `enter /etc/hostname` stands on the
file and narrows nothing — yields nothing, which is what `ono.place.enter` declares (`output:
null`).

The record is built by one function, `ono_command::context_frame_record`, which `get context` uses
too, so the two can never disagree about what a frame looks like.

The contracts' examples name objects every environment has — `enter user root`, `enter group
root`, `enter interface lo`, `enter process 1`, `enter mount /` — so the conformance suite runs
them and holds the value to the schema. `enter link prod-db` and `enter service nginx` stay
exempt for the honest reason: a fresh environment has neither.

## Consequences

Easy: scripts read what they entered; the conformance suite covers five more commands; the
exemption that said "this build implements nothing" is gone.

Hard: nothing shipped relied on the refusal.

Encoded by `crates/ono-cli/tests/context.rs::should_yield_the_frame_it_pushed_when_entering_an_object_feeds_a_pipeline`,
`::should_yield_the_frame_when_the_object_to_enter_arrives_through_the_pipe`,
`::should_yield_a_filesystem_frame_when_entering_a_directory_feeds_a_pipeline`,
`::should_still_change_the_frame_when_the_value_is_consumed`, the generated
`command_conformance.rs` examples for the five `enter` commands, and acceptance case 045.

## Alternatives considered

**Change the contracts to `output: null`.** It would have made the declaration honest and the
value unavailable; a frame is already a value elsewhere (`get context`), and a script that enters
an object has every reason to want to see what it entered.

**Yield the whole stack.** `get context` does that. A command answers for what it did, and
`enter` pushed one frame.

**Register an implementation in the command registry.** The registry's implementations run in the
command runtime and cannot change the session's stack (ADR-0023); the shell's own dispatch is
where the frame is pushed, so that is where the value is yielded.
