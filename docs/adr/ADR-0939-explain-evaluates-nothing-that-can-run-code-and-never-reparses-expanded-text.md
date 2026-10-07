# ADR-0939: `explain` evaluates nothing that can run code, and never re-parses expanded text

- Status: accepted
- Date: 2026-10-07
- Spec refs: §15.3, §17.3, §42, §54; ADR-0019, ADR-0942, ADR-0943 (amended)
- Decided by: agent (autonomous)

## Context

Spec §15.3 promises that `explain` shows what a pipeline would do "without executing", and
ADR-0942 repeats it: the plan builder answers "without running any part of it". Two paths broke
that promise (release review R1, v0.6.3):

1. **Prefix-assignment values were evaluated.** ADR-0943 made the plan builder call the
   evaluator's `prefix_assignments`, which evaluates a value written against the `=` with the
   full expression evaluator. `explain { X="$(touch p)" ls }` and `explain { X=(touch p) ls }`
   ran `touch`. ADR-0943's Consequences accepted this ("`$( … )` in it would run"), which
   contradicts §15.3.
2. **Expanded words were parsed again as source.** For a bare single-stage subject and for a
   `$var` or quoted subject, `explain::subject` evaluated the arguments (globs, captures,
   interpolations), joined them with spaces unquoted, and `plan` parsed that text as a new
   pipeline. A file named `a.tmp | X="$(touch pwn)" ls .tmp` made `explain remove file *.tmp` plan
   a second stage and run `touch`; `explain ls (touch p)` and `explain ls "$(touch p)"` ran the
   capture while building the subject.

## Decision

1. **Plan mode reads, it never runs.** While building a subject or a plan, `explain` evaluates
   only expressions that can do no more than read the session: literals, `$name`, `$name.field`,
   and strings interpolating only those. A capture `( … )`, an interpolated `$( … )`, a call or a
   block is never evaluated.
2. **Prefix assignments, amended from ADR-0943.** The plan builder and execution share one
   stripping function (`strip_prefix_assignments`); they differ only in how a value is read. In a
   plan a word value has its variables substituted (`GREETING=$who` → `me`), an expression value
   that only reads the session is evaluated, and any other value is stated as the source text it
   was written as (`X="$(touch p)"` → `"$(touch p)"`). ADR-0943's other decisions stand.
3. **The unquoted subject is source, never rebuilt text.** Bare words — one stage or several —
   are the source from `explain`'s first argument to the end of the list, verbatim. The subject is
   parsed once, and the plan is made from that AST.
4. **A glob names its files as data.** For a single bare stage (§17.3, ADR-0942 decision 2), each
   unquoted glob is resolved with the shell's expansion rules (ADR-0019), and its matches replace
   the pattern as arguments of the parsed stage — one argument per path, however the filename is
   spelled. The stage's shown text and the plan's `subject` carry the matches, each quoted when it
   is not one plain word; that text is shown and never parsed.
5. **A quoted subject that interpolates code is refused.** A quoted string is the subject's text
   as it evaluates (ADR-0942); one interpolating anything but a variable is refused with
   `safety.policy_denied` and the help to write it as a block, because building the text would run
   it. A `$var` subject is the variable's value, parsed as the pipeline to plan; whatever that
   text holds is planned, never run.

## Consequences

- Nothing a subject, a filename or a variable contains can run while `explain` builds its answer.
- A plan may state an assignment value as source text rather than as the value it would take;
  that is the honest answer when obtaining the value means running something.
- `explain ls $HOME` (one bare stage) shows the source `ls $HOME`, no longer its expansion; a
  glob still names its targets.
- Tests: `crates/ono-cli/tests/explain_runs_nothing.rs` (marker-file tests for a prefix-assignment
  value with `$( … )` and `( … )`, an argument capture, a glob match with a crafted filename, a
  `$var` subject, and the refused quoted subject); the existing `explain.rs` suite, including
  `should_evaluate_a_prefix_assignment_value_as_execution_does` for a plain `$var`, and
  `files.rs::should_name_the_files_a_glob_resolved_to_when_explaining_a_removal`, unchanged.

## Alternatives considered

- **Evaluate values in a sandbox.** There is no sandbox in which an external program is
  guaranteed to have no effect; reading is the only provably harmless evaluation.
- **Quote the expanded words and keep re-parsing the joined text.** Correct quoting would have to
  be perfect forever, for every lexer rule; planning from the AST removes the class of bug.
- **Show globs unexpanded.** Contradicts §17.3 and the existing plan of a removal, whose one line
  an operator reads before a destructive command.
