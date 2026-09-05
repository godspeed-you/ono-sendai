# ADR-0430: A failure proof arranges the failure from outside the process

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §0.5.1, §0.5.3, §2.1, §2.2, §2.3, §7, §13, §16.1–§16.5, §57 (phase H0),
  §58.1, §59.1, §59.7, Appendix D; AGENTS.md §7 (RED before GREEN, `#[ignore]` needs a
  `// REASON:` and a board entry), §11 (tests assert outcomes), ADR-0426 (a suite is named for
  its subject)
- Decided by: agent (autonomous)

## Context

v0.4.1 §57 opens with phase H0 and one rule: *"No production fix lands before the corresponding
failure proof where practical."* Issue #31 enumerates the four proofs. This ADR covers the two
that live in the security crates:

1. **The unauthenticated client** (§0.5.1). `TlsListener::bind` builds its `rustls::ServerConfig`
   `with_no_client_auth()` and `TlsListener::accept` returns a `TlsTransport` whose `peer_key` is
   `None`, with a comment saying so in as many words: *"The listening side authenticates
   nobody."* Everything downstream — the Ono `Hello`, the provider inventory, capabilities,
   actions — is then reached by whoever dialled the port. The `Identity` the peer sends is a
   string it chose about itself, which §2.1 forbids from satisfying the word *authenticated*.
2. **The ignored KUANG control failure** (§0.5.3). `sandbox.rs` sets four `setrlimit`s, a
   `setpriority`, a `setsid` and a `PR_SET_NO_NEW_PRIVS` inside a `pre_exec` closure, discards
   every return value, and ends with an unconditional `Ok(())`. §16.2 requires each of those
   returns to be checked and §16.3 requires the failure to reach the parent *in a way that
   prevents `exec`*; Appendix D gives the same answer as a table, where the Failure column for
   `no_new_privs`, resource limits and session separation reads `spawn fails`.

Both proofs have the same shape and the same difficulty: the wrong behaviour is what the code
does on a perfectly ordinary day, so the test has to supply the *adversary*, not the defect. For
(1) that adversary is a TLS client the production code would never build. For (2) it is a kernel
that refuses a syscall, which §59.7 anticipates by asking for "an injectable platform layer/test
hook".

The question this ADR settles is how much production code a failure proof is allowed to grow in
order to see the failure.

## Decision

**A failure proof arranges its failure from outside the process under test, and adds no
fault-injection machinery to production code.** Where the arrangement needs a boundary the crate
does not yet expose, the proof exposes the boundary that already exists rather than inventing a
hook beside it.

Applied to the two proofs:

**`crates/ono-remote/tests/client_authentication.rs::should_refuse_a_tls_client_that_presents_no_certificate`.**
The test builds its own `rustls::ClientConfig` — `with_no_client_auth()`, a server verifier that
accepts anything, TLS 1.3 — connects to a `TlsListener` serving the fixture registry, wraps the
TLS stream in `PlainTransport` and opens a `RemoteLink` over it. The observable outcome is the
list of provider ids the anonymous peer read back; §59.1 says a peer that has not been authorized
may not see that inventory at all. The test does **not** dial through `ono_remote::tls_connect`,
which happens to present no certificate today: issue #35 will make it present one, and a proof
written through it would silently stop being a proof of anonymity the moment it went green. Two
ALPN tokens, `ono/2` before `ono/1`, are offered together, because §13.4 permits the fix to
advance the token and a client offering only `ono/1` would afterwards be refused for speaking the
wrong protocol — green for the wrong reason. What is left is exactly one thing the client lacks:
a certificate.

**`crates/ono-kuang-supervisor/tests/confinement.rs::should_not_exec_the_plugin_when_a_mandatory_confinement_control_fails`.**
No syscall is injected and no platform layer is introduced. One mandatory control can be made to
fail from outside the process with the standard library alone: `setsid` returns `EPERM` when the
caller is already a process-group leader, and `Command::process_group(0)` makes the child exactly
that, in the `setpgid` std performs *before* it runs the `pre_exec` closures. Session separation
is mandatory — §16.4 lists it `mandatory for the native supervised tier`, Appendix D `required`,
failure `spawn fails` — so a child that reaches `exec` afterwards is §0.5.3 observed rather than
simulated. The stand-in artifact records `/proc/self/stat` as the startup marker §59.7 requires
to remain absent, so the failure message names the session the plugin ran in and the supervisor's
own; today they are the same number, which is the whole defect in one line.

**The seam is `pub use sandbox::apply`, and nothing else.** `Sandbox`, `native_process`,
`nice_of` and `working_directory` were already public; `apply` — the function that actually
installs the confinement — was not, so the crate exported the description of a boundary and not
the boundary. Exporting it changes no behaviour, adds no test-only parameter, and gives the proof
a contract boundary to assert at: given this command and this sandbox, this is what did or did
not run. Nothing about the fault lives in the crate.

**Both files are named for their subject, not for their colour.** ADR-0426 retired
`spatial_*_missing.rs` precisely so a suite would not have to be renamed when it goes green, and
`client_authentication.rs` and `confinement.rs` are already the names those suites will keep. The
RED phase is carried where AGENTS.md §7 puts it — the `#[ignore]`, the `// REASON:` above it, the
module documentation's present-tense account of what the code does instead, and the *Deferred*
entry on the board that points back here.

**Un-ignoring is the fix's work, and only the attribute moves.** Issue #35 removes the
`#[ignore]` from the first when the listener demands and verifies a client certificate; issues
#59 and #60 remove it from the second when a mandatory pre-exec failure aborts the spawn. Neither
increment may edit the assertion: a proof rewritten by the change it was measuring proves the
change, not the requirement.

## Consequences

Easy: the two proofs run against unmodified production code, so nothing has to be removed when
they go green, and there is no fault-injection path that could ever be reachable in a shipped
binary. The KUANG proof needs no privileges, no container and no particular kernel configuration
— `EPERM` from `setsid` is a POSIX guarantee about process-group leaders, not a permission — so
it is as deterministic for root as for anyone else.

Hard: `setsid` is the *only* mandatory control that can be failed this way. `PR_SET_NO_NEW_PRIVS`
does not fail on any Linux that has it, and an unprivileged `setrlimit` failure needs a hard
limit the test would have to lower for the whole test binary. §59.7 names `no_new_privs` by name
and issue #59 asks for "one per mandatory control", so **phase H4 still owes the injectable
platform layer** — this ADR narrows what H0 must build, not what H4 must. When that layer
arrives, the proof below should keep working unchanged: it drives the real syscall, which is a
better test than the injected one it will sit beside.

Also hard: `apply` is public API now, and the H4 fix will very likely change its signature to
return a `Result` or to be folded into a spawn helper. That is a contract change and belongs in
that increment's ADR; the proof asserts on the marker file and not on the signature, so it
survives either shape.

Encoded by: `crates/ono-remote/tests/client_authentication.rs::should_refuse_a_tls_client_that_presents_no_certificate`,
`crates/ono-kuang-supervisor/tests/confinement.rs::should_not_exec_the_plugin_when_a_mandatory_confinement_control_fails`.

## Alternatives considered

**A `ControlSyscalls` table of function pointers in `sandbox.rs`, swappable by a test.** The
straight reading of §59.7, and the thing H4 will probably need for `no_new_privs`. Rejected for
H0: it is production code written before the test that demands it (AGENTS.md §7 rule 3), it makes
the confinement path indirect for every real spawn in order to serve one test, and a table of
pointers a caller can replace is a hook that has to be argued about at review time. The natural
`EPERM` costs none of that.

**A test-only `fail: Option<Control>` field on `Sandbox`.** Smaller than the table and much
worse: a struct the shell fills in for every plugin would carry a field whose only purpose is to
break confinement. §2.3 is about controls failing closed, and shipping the ability to ask for a
failure is the wrong direction to point that at.

**Drive the KUANG proof through `Supervisor::load` or `TestHost`.** The honest outer boundary,
and where H4's exit tests belong once a structured `plugin.confinement_failed` exists to assert
on. It cannot host the H0 proof: neither `LoadConfig` nor `TestHost` can put the child in its own
process group, so there would be no failure to observe without adding the injection layer first.

**Write the TLS proof through `ono_remote::tls_connect`.** One line instead of eighty. It proves
the same thing today and stops proving it the day issue #35 gives the client a certificate,
because the test would then be asserting that an *authenticated* client is refused — which is the
opposite requirement.

**Assert that the listener's transport reports `peer_key() == Some(..)`.** That is §58.1's second
Done criterion and a good test for issue #35 to add. It is not this proof: it asks what the
accepted connection knows, where H0 has to ask whether the connection was accepted at all, and a
peer key is a structural fact about a transport rather than an outcome a caller observes
(AGENTS.md §11).
