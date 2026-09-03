# ADR-0464: A flag is refused for saying *whether*, not for containing "auth"

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §7.4 (no unauthenticated network mode in the canonical agent), §11.1, §65.1;
  AGENTS.md §10; ADR-0440 (whose open piece this closes), ADR-0437, ADR-0439
- Decided by: agent (autonomous)

## Context

ADR-0440 delivered issue #39 as a guard: `crates/ono-cli/tests/listening_agent.rs` enumerates
fourteen spellings of the flag §7.4 forbids and asserts each is a usage error. Its *Consequences*
section names the one piece it could not deliver, because a gate check belongs in `xtask` and that
tranche did not own it, and it specifies what the check should assert:

> that `crates/ono-cli` contains no argument string matching
> `(?i)(no[-_])?(client[-_])?auth|insecure|anonymous|unauthenticated` outside this test file.

That expression does not survive contact with the repository. Its alternation makes every branch
optional except the last three, so it matches the bare substring `auth` — and phase H2, which
starts next, is built on `authorized_clients`, `AuthorizationContext`, `remote.unauthorized` and
`capability_denied`. The check would have fired on the vocabulary of the phase it was written to
protect, and the only way to keep the gate green would have been to weaken it until it meant
nothing.

## Decision

**The check reads command-line flags, and refuses a flag for saying *whether* to authenticate.**

Two narrowings, both of which make it stricter where it matters and quiet where it does not:

**1. Only string literals beginning with `--` are read.** That is how every flag in this
repository is written — `"--agent"`, `"--listen"`, `"--print-peer-key"` — and there is no second
way to spell one, so nothing escapes by being written differently. Reading literals rather than
lines is what stops the rule firing on the word "authenticated" in a doc comment, which is where
most of its occurrences are and where they belong.

**2. A flag is refused when it carries `insecure`, `anonymous`, `unauthenticated` or `noauth`, or
when it turns something off — `no-…`, `disable…` or `skip…` combined with `auth` or `verify`.**
The distinction is the one §7.4 draws: `--listen` says *where*, `--host-key` says *which*, and
neither is a problem. `--no-client-auth` and `--skip-peer-verify` say *whether*, and that is the
only question the canonical agent does not offer.

`tests/` is out of scope. ADR-0440's guard has to name the flags it refuses in order to refuse
them, and a rule that caught the guard would delete the guard.

## Consequences

- Issue #39 is complete: the test covers a flag that reaches `Invocation`, and the scan covers one
  written anywhere in a crate's source, including a flag parsed somewhere new.
- `--print-peer-key`, `--print-host-key`, `--host-key` and `--no-config` are all explicitly proven
  to pass, so the rule cannot be said to be tuned to today's flag list by accident.
- H2 can use the word `authorized` freely, which it must.
- A flag that says *whether* can still be added — by superseding this ADR and §7.4's reading of it,
  which is the point. The check makes the addition a decision instead of a diff.
- The scan is repository-wide rather than `ono-cli`-only. A flag defined in another crate and
  re-exported would otherwise be invisible, and the cost of the wider walk is nothing.

## Proof

`xtask/tests/scan.rs::should_report_a_command_line_flag_that_would_switch_client_authentication_off`,
`::should_report_every_spelling_of_the_flag_the_spec_forbids` (seven spellings, one assertion
each), `::should_accept_the_flags_a_listening_agent_actually_has`,
`::should_let_the_test_that_proves_the_absence_name_the_flags_it_refuses`, and
`::should_find_no_authentication_disabling_flag_in_this_repository`. The first two were red before
the implementation existed; the third and fourth are the guards against a rule that fires too
widely, and they are the ones ADR-0440's expression would have failed.
