# ADR-0966: An adapter may declare the stderr lines it holds back

- Status: accepted
- Date: 2026-11-09
- Spec refs: spec v0.3 §1.20, §1.32, §1.44; ADR-0055, ADR-0057, ADR-0059
- Decided by: agent (autonomous)

## Context

Bare `ss` asks the kernel for every socket family. A kernel with no diag module for one of them —
tipc, vsock — answers `RTNETLINK answers: Invalid argument` on stderr, above a table that is
complete for every family the kernel has. The adapter passed the program's stderr through
untouched, so the line sat above the decoded table on every call (#138, found by #131).

Spec v0.3 §1.20 is exact about what an adapted execution preserves: the child's status, its
signal, and "stderr bytes unless the adapter explicitly declares a machine error protocol". Whether
an adapter may hold back a diagnostic its program prints about a family the user did not ask
about is therefore a question of declaration, not of decoding: without one, every byte is the
program's to say; with one, the adapter has taken responsibility for what those bytes mean.

## Decision

**An invocation's plan may declare `stderr_filter`**: regular expressions naming the lines of the
program's stderr that are diagnostics about something the invocation did not ask about. The
declaration is the machine error protocol §1.20 speaks of, and it is validated with the pack —
a pattern that does not compile is refused, and so is the combination with `unbounded`, because a
live stream's stderr cannot be held until the program ends.

With a declaration, the runtime collects the adapted child's stderr instead of inheriting it, and
when the child has ended:

- a child that **succeeded** has the matching lines dropped and every other line written to the
  shell's stderr, in order;
- a child that **failed** has every line written out, the declared ones included — its stderr
  says why it failed, and §1.20's status rule already makes the failure the answer.

The bare `ss` adapter declares `^RTNETLINK answers: `. The `ss -t` and `ss -u` adapters do not:
they ask for one family and have no such line to account for.

An adapter that declares nothing is untouched: its child's stderr stays inherited and reaches the
terminal as it is written, which is what every adapter did before.

## Consequences

Easy: `ss | …` on a kernel missing a diag module shows the table and nothing else; another
tool's noise of the same shape is one line in its pack.

Hard: the stderr of a declaring adapter arrives after the child ends rather than as it is
written, which for a bounded invocation is the same moment to a reader; an adapter must not
declare a line that can mean the program's answer is incomplete, because the user would no longer
see it — `RTNETLINK answers: Operation not permitted` on a failing `ss` is still shown because the
exit status is non-zero, and a tool that prints such a line and exits 0 is a tool whose pack
should not declare it.

Encoded by `crates/ono-cli/tests/adapters.rs::should_hold_back_the_diagnostic_an_adapter_declares_when_the_program_succeeds`,
`::should_pass_through_stderr_the_adapter_did_not_declare_when_the_program_succeeds`,
`::should_pass_every_stderr_line_through_when_the_program_fails`, the pack validation in
`crates/ono-adapter`, and acceptance case 081.

## Alternatives considered

**Filter the line in the `ss` decoder.** The decoder reads stdout; stderr never reaches it, and a
fix there would be a special case in Rust for one tool rather than a declaration any pack can make.

**Ask for the families explicitly** (`ss -t -u -x -w …`) so the kernel is never asked about one it
lacks. The set of families differs by kernel and by iproute2 version, and the adapter would be
guessing at what the user's bare `ss` means on this machine.

**Turn the line into a structured notice** on the shell's stderr. It would still reach the
terminal, which is what the issue is about; and the line says nothing about the answer the user
received, which was complete.

**Hold back stderr for every adapter by default.** §1.20 forbids it: without a declaration the
bytes are the program's.
