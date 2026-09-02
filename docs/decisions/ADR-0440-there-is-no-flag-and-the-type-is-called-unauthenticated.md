# ADR-0440: There is no flag, and the type that authenticates nobody is called Unauthenticated

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §7.4, §11.1, §65.1; §4.3; AGENTS.md §5 level 2, §11; ADR-0037 §4, ADR-0437,
  ADR-0439
- Decided by: agent (autonomous)

## Context

§7.4 is two sentences and they pull in different directions:

> The normal direct listening-agent mode MUST NOT provide a flag that disables client
> authentication.
>
> If an unauthenticated transport remains necessary for tests or in-process duplexes, it MUST be
> inaccessible from ordinary network CLI configuration and clearly named `Unauthenticated` in
> internal APIs.

The first is about a flag that does not exist. The second is about a type that does:
`PlainTransport`, in `ono-protocol`, a byte stream with no protection of its own, used by every
in-memory duplex in the test suites and by the stdio agent path §4.3 keeps deliberately.

§65.1 explains why the second sentence is in a specification at all: "using encryption while
accepting any client certificate/no certificate and then calling the session authenticated is
forbidden". That mistake is rarely made on purpose. It is made by reaching for the type whose name
does not warn you.

## Decision

**Nothing is added, and the absence is written down as a test.**
`crates/ono-cli/tests/listening_agent.rs` enumerates fourteen spellings of the flag §7.4 forbids —
`--insecure`, `--no-client-auth`, `--allow-anonymous`, `--legacy` and the rest — and asserts each
one is a usage error, both through `Invocation::from_args` and through the real binary, and that
nothing is bound on the way to the refusal. It also asserts the shape of the only listening form
there is: `--agent --listen <address>`, whose sibling `--host-key` can point the agent at a
different identity and cannot say not to have one.

The tests were green the moment they compiled. That is what a guard on an absence does, and it is
what issue #39 asks for in as many words ("a test proving no CLI flag combination disables client
authentication"). The value is not in today's run: it is that the day somebody adds
`--allow-anonymous` for one awkward deployment, three tests go red and an ADR has to be written to
explain it. Absences do not defend themselves.

**`PlainTransport` becomes `UnauthenticatedTransport`.** §7.4 says `clearly named
Unauthenticated`, and "plain" is a word about formatting. The rename is source-breaking for
`ono-protocol`'s public API and is done without a compatibility alias: two names for one type is
the ambiguity the sentence is trying to remove, and `-D warnings` makes a `#[deprecated]` alias
unusable in this workspace anyway. Twenty-seven mentions across six files change; no test body,
assertion or fixture changes, which AGENTS.md §11 is the test that this really was a rename.

Its documentation now carries the reason rather than only the fact, and `with_peer_key` keeps its
meaning: an outer layer that *did* authenticate the peer declares what it authenticated, so the
trust store sees in a test exactly what it sees in production. A transport that authenticates
nobody and one that reports what somebody else authenticated are the same adapter, and the name
describes what the adapter itself proves — nothing.

**`StdioTransport` and `SubprocessTransport` keep their names.** They are named for the pipe they
run over, not for a trust claim, and `SubprocessTransport::peer_key` has answered `None` truthfully
since ADR-0037 §4. §4.3 keeps the ssh-carried agent's trust model on purpose and requires only that
it be described accurately, which `ono.link/1`'s `transport_trust: unauthenticated` now does
(ADR-0438).

## Consequences

Easy: there is one listening mode, it authenticates, and the type that does not is impossible to
reach by accident from a name.

Hard: issue #39 also asks for "a gate check" beside the test. A gate check belongs in `xtask`,
which this tranche does not own, so it is **not delivered here** and is reported to the
orchestrator as the one open piece of #39. What it should assert is narrow and cheap: that
`crates/ono-cli` contains no argument string matching `(?i)(no[-_])?(client[-_])?auth|insecure|anonymous|unauthenticated`
outside this test file. Until it exists, the three tests above are the guard, and they cover the
flag reaching `Invocation` — which is every flag, because `--agent` refuses anything it does not
recognise.

Also hard: the rename touches `ono-protocol`'s public API in a tranche whose subject is the
transport. It is deliberately its own increment for that reason, with no behaviour in it, so a
`git log` reader can see that nothing else moved.

ADR-0430 refers to `PlainTransport` by its old name. Accepted ADRs are not edited (AGENTS.md §8),
so it keeps it, and this ADR is where the two names are connected.

Encoded by: `crates/ono-cli/tests/listening_agent.rs::should_have_no_flag_that_turns_client_authentication_off_for_a_listening_agent`,
`::should_offer_exactly_one_listening_form_and_it_authenticates_its_clients`,
`::should_report_a_usage_error_rather_than_listening_when_asked_to_authenticate_nobody`.

## Alternatives considered

**Keep `PlainTransport` and document the risk.** Cheapest. Rejected: §7.4 says MUST about the
name specifically, and a comment does not travel to the call site the way a type name does.

**Add `--allow-unauthenticated` with a loud warning, for a compatibility window.** §13.3's third
sentence describes the shape such a mode would have to take if one existed. Rejected: §7.4 forbids
it for the *listening* side without qualification, and a compatibility window is exactly the thing
that never closes. ADR-0439 records that no legacy mode exists at all, which is the strongest form
of the same rule.

**Make the guard a `spec-check` rule instead of a test.** Better, and the issue asks for both. Not
possible in this tranche's file scope; specified above so the increment that adds it does not have
to redesign it.
