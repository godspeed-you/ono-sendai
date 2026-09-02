# ADR-0456: A limit has one home and a range

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §4.5, §12.4, §52.1, §52.2, §52.3, §54.3, §55.1, §55.2, §55.4, Appendix A;
  v0.2 §30; ADR-0010, ADR-0094
- Decided by: agent (autonomous)

## Context

Appendix A fixes sixteen numbers and §55.1 names twelve configuration keys for them. §52.2 says
what must not happen to those numbers:

> A number such as `max_connections = 32` MUST not be independently typed into five files if one
> contract can generate the others.

§55.2 adds the rule that turns a configuration key into a boundary rather than a hint, and it is
the one sentence in §55 written as a MUST NOT:

> A security-sensitive agent limit MUST NOT silently become unlimited because a value failed to
> parse.

Ono had none of it. `RETAINED_RESULTS = 16` and `RETAINED_VALUES = 10_000` were constants in
`session.rs`; `BUDGET = 40ms` was a constant in `complete.rs` where Appendix A says 50; the
materialization and capture ceilings did not exist; the remote ceilings did not exist. The
settings catalogue of ADR-0094 type-checked every assignment and range-checked none, so
`render.table.max_rows = -5` was accepted and sanitised at the read site, which is the shape §55.2
forbids applied to a limit that matters.

## Decision

### 1. One registry, `docs/spec/hardening/limits.yaml`

Every runtime limit is declared once, as data: key, type, Appendix A's default, the permitted
range, the unit, and **what enforces it**. `ono_cli::settings::CATALOGUE` declares the same
thirteen rows, and `crates/ono-cli/tests/resource_limits.rs` compares the two in both directions —
a key the shell enforces and the registry omits is a failure, and so is the reverse.

§52.1 names `materialization_limits` and `remote_limits` as two of its seven registries. This is
one file for both, because §55.1 presents the keys as one family and §52.2 is about the *number*
having one home rather than about the file count. The remote rows carry `enforced_by: pending`:
their **configuration** is this file's (§55.2 is a configuration-layer requirement), and their
**enforcement** is `ono-remote`'s, arriving with phase H3. Saying `pending` rather than leaving
them out is the same discipline Appendix D uses for a confinement control a tier does not install
— a reader can see that the key was considered and does not yet act.

The thirteenth row is `limits.command_capture_bytes`, Appendix A's "Nested command capture bytes"
(256 MiB). §55.1 does not list it; §23.4 requires the number and §52.2 requires it to have one
home, so it is here rather than in the evaluator.

### 2. Every limit is range-checked, once, where every layer passes

`SettingSpec` gains `range: Option<Range>`, and `Settings::assign` checks it after typing —
which means the file, the environment and `set config` at the prompt are all checked by one piece
of code, because all three already funnel through `assign` (ADR-0094). Nothing is stored when the
check fails, so the earlier layer's value stays in force, which is ADR-0010's existing rule and is
exactly what makes §55.2's sentence true: a limit that failed to parse does not become unlimited,
it stays what it was.

The refusal reuses `type.mismatch` (`Ono-Sendai-E0201`) rather than adding a code. ADR-0094
routes every rejected assignment through it, `get config --problems` reports one shape, and a
second code for the same layer's refusal would make a script that read one of them incomplete.
What changes is the message, which names the key, the value and the range it is outside — a
diagnostic that says only "invalid" leaves the user to guess which end they were on.

**Zero is not unlimited, and neither is one.** §22.2 fixes that a ceiling of zero means "no values
permitted", so the materialization and history minima are zero. The listening agent's are one:
a connection ceiling of zero would turn the listener off silently, and §2.3 wants a boundary that
refuses rather than one that disappears. `limits.remote_handshake_timeout_ms` floors at 100 ms
for the same reason — a timeout too short to complete a handshake is a denial of service spelled
as a configuration value.

### 3. No new environment variable

§55.4 asks that new security-sensitive environment variables be avoided, and that any that are
added follow the existing precedence and be documented. None are added: ADR-0010's mapping is
mechanical, so `limits.materialize_items` is `ONO_LIMITS_MATERIALIZE_ITEMS` without a table, at
layer 4 of five, and range-checked by the same `assign` a file goes through.

### 4. `history.result_cache` is superseded, not removed

§4.5: *"Existing configuration files MUST continue to parse."* `history.result_cache` is a
declared key, a documented example in `docs/spec/commands/meta.yaml` and an acceptance case, and
nothing reads it. `limits.history_bytes_total` is v0.4.1 §55.1's spelling for the same ceiling and
is what the shell now enforces. The old key stays declared, with its description naming its
successor, so an existing file parses and nobody has to guess which of the two works. Retiring it
belongs to a release that may break configuration, and is recorded for the board rather than done
here.

## Consequences

Easy: a limit is one row. Changing Appendix A's default is one number in one file plus one in the
catalogue, and the test that compares them fails if only one moves. `inspect limits` (#120) and
`explain` (#69) read the catalogue, so a user and a test see the figure the shell enforces.

Hard: thirteen keys is public configuration surface, and §4.1 makes a stable key a compatibility
promise. The four remote keys promise a *setting*, not yet an effect, and `enforced_by: pending`
is how that is said out loud rather than discovered.

Also hard: `complete.rs`'s 40 ms budget and Appendix A's 50 ms soft budget still disagree, and
this ADR does not resolve it. `limits.completion_soft_ms` and `limits.completion_hard_ms` are
declared, validated and readable, and the completion path does not yet read them —
`enforced_by: ono-cli` overstates that by one increment, which is why their descriptions say
"Recorded; the completion budget is phase H7's". Aligning the two numbers is #86's, not this one's
(AGENTS.md §4), and it is recorded for the board.

Coordination: `ono-remote`'s agent owns the enforcement of the four remote rows. They should read
their ceilings from `ono_cli::limits::magnitude` (or from a small reader over the same catalogue)
rather than declare constants, and `docs/spec/hardening/remote_limits.yaml` — if §52.1's separate
registry is still wanted — should describe the *semantics* of the enforcement and reference these
keys rather than restate the numbers.

Encoded by `crates/ono-cli/tests/meta_config.rs::should_accept_every_documented_limits_key_and_reject_an_unknown_one`,
`::should_refuse_a_limits_value_outside_its_permitted_range_and_name_the_range`,
`::should_apply_the_documented_environment_override_for_a_limits_key`,
`crates/ono-cli/tests/resource_limits.rs::should_answer_the_same_figures_inspect_limits_shows_from_the_contract_registry`,
and `xtask/tests/contracts.rs::should_reject_a_limit_whose_default_lies_outside_its_own_range`.

## Alternatives considered

**A `Limits` struct with thirteen named fields, built once at startup.** §12.4's own words are "the
centralized `Limits`", and it would be a second copy of every number — the thing §52.2 forbids —
kept in step with the catalogue by hand. The catalogue *is* the centralization; what this
increment adds are the typed readings each component needs and nothing more.

**A new `config.out_of_range` error code.** More precise, and it splits one layer's refusal into
two shapes for a script and for `get config --problems`. ADR-0094's single funnel is worth more
than the precision, and the range is in the message and in the metadata for anyone who wants it.

**Range-checking at the read site instead of at assignment.** It is what the shell did for
`render.table.max_rows`, and §55.2 says "at configuration load time" for a reason: a value checked
at every read is a value that was accepted, so `get config` shows a figure the shell will not
honour, and the user learns about it from behaviour instead of from a diagnostic.

**Two registries, one per §52.1 name.** It would put `limits.remote_connections`'s default in one
file and its configuration key in another, which is §52.2's own example of the defect.
