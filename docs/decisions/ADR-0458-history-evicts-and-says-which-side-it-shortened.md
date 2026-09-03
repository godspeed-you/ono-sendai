# ADR-0458: History evicts, and says which side it shortened

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.4, §21.1, §21.3, §24.1, §24.2, §24.3, §31.3, §54.1, §60.6, §65.6, §67.6,
  Appendix A; v0.2 §6.4, §17.5, §20.2; ADR-0249, ADR-0262
- Decided by: agent (autonomous)

## Context

The session retains recent structured results so `@-1` and `@N` can reach them. It was bounded by
two counts — sixteen results, ten thousand values each — and by nothing else, which is §65.6's
defect exactly: ten thousand values whose size nobody bounds is not a memory bound. §24.1 adds
16 MiB per result and 64 MiB in total.

§24.2 then says something no other budget in this tranche says:

> Result history is a cache, not a correctness requirement. It therefore uses eviction rather than
> failing the user's command.

And §21.3 says the two responses must never be mixed implicitly. So the interesting decision here
is not the ceiling. It is how one abstraction (§21.1) serves two responses without either of them
leaking into the other.

§31.3 says where it lives: *"result-history byte-budget enforcement belongs in
`ResultHistoryState`, not scattered across evaluator call sites."* It was scattered across four:
`native.rs`, `view.rs` twice, and `context_jobs.rs`, each calling `retain_result` and then
`retention_notice` with two numbers it had computed itself.

## Decision

### 1. `ResultHistory` in `ono-history`, and it cannot fail

The retained results move out of `Session` into `ono_history::ResultHistory` with
`RetentionLimits` — Appendix A's four figures, configurable through `limits.history_*`
(ADR-0456). It lives in `ono-history` because that crate is what the shell's histories live in;
what was there before is the *command* history, and the module documentation now says which is
which, because "history" meaning two things silently is how the confusion started.

`retain` returns `Retained`, never a `Result`. §21.3's two branches are kept apart by the type:
the materializer's `Budget::charge` can refuse and this cannot, so a caller cannot accidentally
implement the wrong one. The shared abstraction of §21.1 is still there — `retain` spends a
`Budget` and the estimator of §21.2 — and what differs is only what it does when the budget says
no.

### 2. It is handed a borrow, so §60.6 is a property rather than a promise

```rust
pub fn retain(&mut self, values: &[Value]) -> Retained
```

§24.2 rule 1 is that *"the live pipeline result is never truncated merely to fit history"*, and
§60.6 makes it an acceptance scenario. A function that took `Vec<Value>` could shorten it and be
correct only by remembering not to; a function that takes `&[Value]` cannot. The old
`retain_result(Vec<Value>)` remains as a thin wrapper for callers that own their values, and does
the same thing.

Redaction (spec §17.5, ADR-0262) runs *inside* retention through `retain_mapped`, so a result of
eighty thousand values pays the redaction policy only for what history keeps — which is what the
old truncate-then-redact order achieved and what a redact-then-retain rewrite would have lost.

### 3. The notice says which side was shortened

The old sentence was:

```
retained the first 10000 of 10005 values for reuse; `@-1` sees 10000
```

§24.3 requires more than a count: *"It MUST NOT present the retained subset as though it were the
complete original output."* The user has just watched 10 005 rows go past, and the sentence has to
make clear that those rows were all of them. §54.1's shape becomes:

```
result history kept 10000 of 10005 values because its retention budget was reached;
the command's own output was complete; `@-1` sees 10000
```

`Retained::truncated_for_history()` is §24.2 rule 3's marker, and it outlives the run:
`Session::previous_result_retention(n)` answers it for any retained entry, so an inspection of a
partial entry can say so rather than the notice being the only chance the user had.

### 4. The newest entry is never evicted to satisfy the total

Eviction is oldest-first until the slot count and the total byte budget are met, with one
exception: the entry just retained stays. A history that answered nothing after every command
would be a cache that is never a cache, and its own size is already bounded by the per-result
ceiling, so keeping it cannot be unbounded.

## Consequences

Easy: `@-1` is bounded in bytes as well as in values, and the four call sites are one line each
rather than three. Configuration reaches it: `limits.history_bytes_per_result` and
`limits.history_bytes_total` narrow a running session's history when the config layers land.

Hard: the notice's wording changed, and `native.rs::should_say_so_when_a_result_is_too_large_to_retain_whole`
changed with it. That is a contract change, recorded here, made in the same increment as the
change it describes (AGENTS.md §7) — the behaviour it asserts, 10 000 of 10 005 retained and
`@-1` seeing 10 000, is unchanged.

Also hard: `RETAINED_RESULTS` and `RETAINED_VALUES` are gone as public constants, replaced by
`DEEPEST_REFERENCE` for the one thing that still needs a compile-time figure — how far a `@-N`
reference may reach when the argument scope is built. A limit that is configurable cannot also be
a `const`, and `native.rs` was using `RETAINED_RESULTS` for exactly that.

Encoded by `crates/ono-history/tests/result_history.rs` — eight tests, from
`should_evict_the_oldest_result_when_the_total_byte_ceiling_is_reached` to
`should_never_fail_the_command_however_far_past_its_ceilings_a_result_is` — and by
`crates/ono-cli/tests/resource_limits.rs::should_leave_the_pipeline_output_complete_when_history_could_not_keep_it_all`,
`::should_stop_retaining_a_result_at_its_configured_byte_ceiling`,
`::should_evict_the_oldest_retained_result_when_the_total_history_ceiling_is_reached`.

## Alternatives considered

**Keeping the results in `Session` and adding the byte counting there.** §31.3 asks for the
opposite in as many words, and the four call sites are the evidence: each of them knew the
retention policy well enough to compute the notice's two numbers, which is the scattering the
section names.

**Making `retain` return a `Result` and having callers ignore it.** One type for both of §21.3's
responses, and the ignoring is where they get mixed. A cache whose failure is discarded at four
call sites is a cache that will one day fail a command at the fifth.

**Estimating a result's size once, after truncation, instead of per value.** Cheaper, and it
cannot stop at the byte cap: it would have to keep everything to measure it, which is what the cap
exists to prevent.

**Evicting the newest entry when a single result exceeds the total budget.** Consistent, and it
means a session whose results are all slightly too large answers `@-1` never, with nothing said.
The per-result ceiling already bounds the newest entry; keeping it is what makes the cache useful
at the size the user actually configured.
