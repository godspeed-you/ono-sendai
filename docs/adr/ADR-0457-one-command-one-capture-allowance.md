# ADR-0457: One command, one capture allowance

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §21.1, §21.3, §23.1, §23.2, §23.4, §28.3, §65.6, §65.7, Appendix A;
  v0.2 §19.2; ADR-0069, ADR-0072
- Decided by: agent (autonomous)

## Context

§23.1 names the thing and the failure in one line: *"Capture is not an invisible unlimited
vector."* Ono had exactly one, and it was well hidden:

```rust
// crates/ono-cli/src/session.rs
captures: Vec<Vec<Value>>,

pub fn capture(&mut self, values: &[Value]) -> bool {
    match self.captures.last_mut() {
        Some(capture) => { capture.extend(values.iter().cloned()); true }
        None => false,
    }
}
```

Every capture path in the evaluator funnels through that one method — command substitution
`(…)`, `let x = pipeline`, a function body consumed by a later stage, a nested `(…)` inside a
stage argument, an external command's captured stdout — which is fortunate, because it means
there was one place to fix rather than eleven. It had no item cap, no byte cap and no accounting
across nesting.

§23.4 is the part that is easy to get wrong even after the cap exists:

> Nested captures MUST not each independently consume the full global allowance without
> accounting. […] At minimum, a single shell command MUST have a documented upper bound on the
> total bytes retained by simultaneous evaluator captures.
>
> `command.capture.max_bytes = 256 MiB` — This is a ceiling across nested capture contexts, not an
> invitation for each capture to allocate 256 MiB.

## Decision

### 1. One `Budget` per shell command, shared by every capture inside it

`Session` holds a single `Budget`, minted at the top of each statement in `run_program` and
charged by `Session::capture` for every value any capture takes. Not one per capture, not one per
nesting level: §23.4's ceiling is "a single shell command", so the statement loop is where it
resets and nothing inside a command resets it.

`capture` returns `Result<bool, ErrorValue>` instead of `bool`, so a refusal propagates as
`Flow::Failed` through the three call sites rather than being discarded. §21.3's first branch, in
its own words: the operation stops with a structured resource-limit error. It does not truncate
the capture, and it does not warn and carry on.

The ceiling reads `limits.command_capture_bytes` (ADR-0456), so it is Appendix A's 256 MiB by
default and the user's figure when they have set one.

### 2. `end_capture` does not refund

A capture that has ended has usually just handed its values to something that still holds them —
a variable, an enclosing capture, a stage's argument scope. Refunding on `end_capture` would make
the ceiling bound only what is open at one instant, which is not what §23.4 asks for and is not
what the memory does. So the charge stands until the command ends.

The consequence is visible and is the point. A function body consumed by a later stage, inside a
substitution that binds the result, charges the same four records twice, because two captures
really do hold them:

```
let x = (get process | take 4 | where pid > 0)      # one capture
fn four() { get process | take 4 }
let x = (four | where pid > 0)                       # two, nested
```

Under a ceiling that holds the first, the second refuses. That differential is
`should_accumulate_nested_captures_against_the_one_per_command_ceiling`, and it is written so the
ceiling is *measured* rather than assumed: a process record's size depends on what is running on
the machine, so the test finds the smallest power-of-two ceiling that holds one capture, and the
nested case is then guaranteed to exceed it — one capture costs more than half of that ceiling, so
two cost more than all of it. No literal byte count appears in the assertion.

### 3. The refusal names the ceiling that stopped it

A capture refusal that told the user to raise `limits.materialize_bytes` would send them to change
a number that cannot help them. `Budget::for_settings` attaches the keys a refusal should point at,
so the capture budget names `limits.command_capture_bytes` and a materializer names
`limits.materialize_bytes`.

## Consequences

Easy: §23.1's rule — *"No new direct `Vec<Value>` capture path may be added without an explicit
budget wrapper"* — is enforced by there being one path. A new capture calls `Session::capture` and
is bounded, or it does not and is not a capture the shell knows about.

Hard: a command that legitimately captures more than 256 MiB now fails where it used to succeed
slowly. That is §23.4's decision, and the refusal names the key to raise.

Also hard: the double charge in §2 is conservative rather than exact. Two captures holding clones
of the same `Arc`-backed values retain one copy, and the budget charges two, because the estimate
is taken per value at the moment it is admitted (ADR-0452 §Consequences). Over-charging is the
safe direction for a ceiling; a shared `Estimator` across one command's captures would make it
exact and is not worth the coupling today.

Constrains H6: the streaming repair must not reach for `Session::capture` to make `each` work.
§25.2 forbids `each` capturing its complete input and §65.7 forbids replacing a foreground `Vec`
with an unbounded background queue; a budgeted capture is still a capture, and passing this
ceiling is not what makes `each` stream.

Encoded by `crates/ono-cli/tests/resource_limits.rs::should_charge_a_nested_command_capture_against_the_shared_budget`,
`::should_accumulate_nested_captures_against_the_one_per_command_ceiling`,
`::should_refuse_a_capture_that_would_exceed_the_command_ceiling`.

## Alternatives considered

**A budget per capture, drawn from a parent.** `Budget::child` and `Budget::absorb` exist and are
tested, and the evaluator does not use them: the captures nest through a `Vec` in the session
rather than through a value a parent could lend, and threading a parent budget through every
`begin_capture`/`end_capture` pair would be more machinery than one shared counter for the same
guarantee. They stay because the hierarchical shape §23.4 *prefers* ("SHOULD use hierarchical
budgets") is one refactor away, and a caller outside the evaluator can already use it.

**Resetting the budget when the outermost capture closes.** Simpler, and it bounds "simultaneous"
too narrowly: `stage_scope` evaluates every nested `(…)` of every option of every stage and holds
each result while it evaluates the next, so several captures' worth of values are live inside one
command with no two of them open at the same instant.

**Truncating a capture that exceeds the ceiling.** It would make `let x = (get process)` silently
answer part of the process table, which is a correctness bug wearing a memory limit's name. §21.3
allows truncation only for a defined cache, and a variable binding is not a cache.
