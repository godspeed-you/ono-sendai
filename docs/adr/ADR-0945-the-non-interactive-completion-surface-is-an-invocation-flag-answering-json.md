# ADR-0945: The non-interactive completion surface is an invocation flag answering JSON

- Status: accepted
- Date: 2026-10-07
- Spec refs: §15.1, §34; v0.4.1 §36.2, §61.4; ADR-0010, ADR-0252, ADR-0944
- Decided by: agent (autonomous)

## Context

ADR-0252 named a non-interactive completion surface and issue #21 could not deliver it; issue
#176 asks for one, so an editor or a tool can ask the shell what Tab would offer, and so the
container can measure a first completion without a terminal. Two routes were open: a flag of the
invocation (`crates/ono-cli/src/invocation.rs`) or a native command. A command needs a verb, and
`docs/contracts/verbs.yaml` is spec §7.1's verb table "transcribed complete and unaltered in
meaning" — no `complete` verb exists, and inventing one would be a deviation for a tool-facing
query that is not an operation on the system.

## Decision

1. **`ono [--config <path> | --no-config] --complete <line> [--cursor <n>]`.** The cursor is a
   byte offset into the line and defaults to its end; one outside the line or inside a character
   is a usage error (exit 2, ADR-0008), as is a missing line or an unknown word after it.
2. **The same pipeline.** The session is read as `-c` reads one (configuration included, no
   terminal), and the answer comes from `repl::ShellCompleter::for_session` — the constructor the
   interactive prompt now uses too — through `ShellCompleter::answer`, which the editor's
   `Completer` is a thin presentation of. Registry candidates, provider values within the
   session's §36.2 budgets, adapters' declared flags, commands on `PATH`, builtins, paths and the
   §9.4 neighbourhood are all exactly what Tab sees.
3. **One JSON document on one line**, `ono.completion/1`
   (`docs/contracts/schemas/completion.v1.yaml`): `line`, `cursor`, `start` and `end` of the span
   a candidate replaces, `complete` (ADR-0944) and `candidates`, each `{text, kind, doc}`. The
   kinds are the registry's (`verb`, `target`, `option`, `value`, `field`, `operator`) and the
   shell's own sources (`builtin`, `program`, `path`, `place`, `relation`; `function` and
   `alias` with #223). It is written with `to json`'s encoding of a record built against the
   schema, so the document is the contract.
4. **A partial answer is answered, not waited out.** A cold provider read the soft budget did not
   cover answers `complete: false`; the caller may ask again. No terminal is opened or required.
5. `--help` lists the flag; `docs/reference/schemas.md` documents the document.

## Consequences

- Editors and tooling have a stable machine-readable completion; the prompt and the flag cannot
  diverge because there is one `answer`.
- Case `060-performance-budgets` measures the first completion without a terminal: from metadata
  against spec §34's 50 ms, provider-backed against §61.4's 150 ms hard budget, each a whole cold
  process.
- Tests: `crates/ono-cli/tests/complete_surface.rs` (document shape, cursor, kinds, provider
  values through the same completer, `complete: false`, usage refusals); the PTY suites in
  `completion.rs` unchanged; acceptance cases `060` and `383-complete-without-a-terminal`.

## Alternatives considered

- **A native `complete` verb.** Not in §7.1's table; a meta query about the line editor, not an
  object operation.
- **Waiting for the hard budget before printing.** Would make the flag disagree with the prompt;
  `complete: false` says what a partial answer is.
- **Plain text, one candidate per line.** Loses the span, the kind, the doc and the completeness
  a tool needs.
