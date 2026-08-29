# ADR-0252: Provider-backed completion that never blocks the prompt

- Status: accepted
- Date: 2026-08-29
- Spec refs: §15.1 (completion is semantic and provider-aware), §34 (the completion budget),
  §17.4, §49 (T4, poisoned completion sources); ADR-0074, ADR-0103, ADR-0112, ADR-0245
- Decided by: agent (autonomous)

## Context

`ono_command::ValueCompleter` has existed since phase D with a doc comment naming exactly what it
is for — "the users on this machine, the services of this host" — and the shell installed nothing
in it. The only implementation was `FieldCompleter`, which completes schema *field names*, again
from metadata. So `get user <TAB>` could offer a target, an option and a filesystem path, and
never an account.

Filling the seam means letting a keystroke reach a provider, and that is where the difficulty is.
Spec §34 budgets the first completion at 50 ms; a provider can be a procfs walk, a container
socket that is not listening, or a D-Bus round trip. The line editor holds the terminal in raw
mode while it waits, so a provider that hangs is a shell that hangs.

## Decision

**The shell installs a provider-backed `ValueCompleter`, bounded three ways.**

1. **A hard deadline.** The query runs on a thread of its own and the keystroke waits 40 ms — less
   than §34's 50 ms, because the rest of the completion has to fit too. A provider slower than
   that contributes nothing to *this* keystroke. Nothing is ever waited for indefinitely, and the
   thread is detached: it finishes on its own and leaves what it read behind.

2. **A cache.** What a provider said about a target is kept for five seconds, so a burst of Tabs
   costs one query and every completion after the first answers in microseconds. The *first* Tab
   for an expensive target — `get process`, half a thousand records — may run out of budget; it is
   in the cache a moment later, and the next keystroke has it. Warming every target at startup is
   the other way to hide that, and it is precisely the eager work spec §34 and case `027` forbid.
   What the shell *does* warm, on a thread, at startup, is the provider registry itself and one
   local account read: the first provider read in a process is far more expensive than the rest —
   the async runtime, and whatever the C library loads the first time an account database is
   consulted — and that cost is process-wide rather than per-target. Paying it once, off the
   keystroke, is what makes the first Tab of a cheap target as fast as the ones after it.

3. **The synchronous registry only.** systemd and the login-session provider are reached with an
   `await` and are registered separately at startup (`providers::register_async`); the completer
   asks the registry built without them, so no keystroke can end up on a D-Bus round trip.

**Every selector of the command is offered, not the one the position would bind.** `get user`
declares `uid` then `name`; the binder resolves a positional word by type, so `get user 0` is a
uid and `get user root` is a name. Completing only the positionally-next selector answers
`get user ro<TAB>` with uids, which is the one answer that is certainly wrong.

**Only values that can be typed back are offered** — strings, integers and paths. A record or a
list has no spelling in a command line, and offering its debug shape would be offering a line
that does not parse.

**Nothing is executed.** A snapshot is the read `get <target>` performs and nothing else: no
mutation, no candidate run, no side effect. That is ADR-0245's T4 unchanged, and it is why the
completer asks providers rather than, say, running the command with a partial argument.

## Consequences

`get user <TAB>` offers this machine's accounts, `get group <TAB>` its groups, and so on for every
target a synchronously-built provider serves. The budget is kept by construction rather than by
hope: the slowest possible completion is 40 ms plus what the metadata answer costs.

The first completion of an expensive target can be metadata-only. That is visible and it is the
price of the first rule; the alternative — blocking until the provider answers — is the failure
mode that makes a shell feel broken.

Not delivered here: the **container measurement** of §34's completion budget. It is still asserted
by `crates/ono-command/tests/completion.rs::should_stay_far_inside_the_first_completion_budget`, a
1 000-iteration in-process proxy. Measuring the real thing needs a completion the container can
invoke without a terminal, which is new public surface and belongs to its own increment;
`docs/STATE.md` keeps that half of B-split-D4 open.

Encoded by `crates/ono-cli/tests/completion.rs::should_offer_this_machines_users_when_completing_a_user_selector`,
`::should_answer_a_completion_that_no_provider_can_serve_without_waiting_for_one`, and
`docker/acceptance/cases/044-semantic-completion.case`.

## Alternatives considered

- **Asking the providers on the editor thread.** Simplest, and one unreachable container socket
  makes the prompt stop responding. Rejected.
- **Completing from the spatial index instead**, the way `spatial_offers` does. The index holds
  what the session has *observed*; a user who has not run `get user` has no accounts in it, so it
  answers nothing on the first day. It is the right source for places, and not for accounts.
- **Warming every target at startup.** Fast Tabs, and a shell that enumerates the host before
  drawing its first prompt. Rejected on §34 and case `027`.
- **A longer deadline.** Every millisecond added is felt on every Tab that has nothing to offer,
  which is most of them.
