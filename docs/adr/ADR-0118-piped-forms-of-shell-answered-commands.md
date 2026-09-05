# ADR-0118: The piped forms of the commands the shell answers itself

- Status: accepted
- Date: 2026-08-27
- Spec refs: §9.1, §10.2, §11.5, §14.4, §16.5, §21, §31.3, §31.36, §31.42, §43
- Decided by: agent (autonomous)

## Context

`remove link`, `detach link`, `set link`, `rename link`, `connect host`, `test host`, `link
host`, `verify plugin`, `load plugin`, `install plugin`, `grant capability` and `ask assistant`
are answered by the evaluator's seams (ADR-0093, ADR-0104, ADR-0108, ADR-0109): their effect is
session state no provider can own. Every seam claimed the *head* of a pipeline only, so the
piped form — `get link | remove link`, `get plugin | verify plugin` — fell through to the
registry as an unbound stage and answered E0101 "declared but this build implements nothing for
it", which is false: the command is implemented, one stage to the left.

The contracts already say what the piped form means. `docs/contracts/commands/remote.yaml` and
`kuang.yaml` declare `input:` per command:

| command | `input` | piped form |
|---|---|---|
| `remove link`, `detach link`, `set link` | `null \| stream<ono.link/1>` | the piped links are the targets |
| `rename link` | `null \| ono.link/1` | one piped link; the one positional left is the new name |
| `verify plugin`, `load plugin` | `null \| stream<ono.plugin/1>` | the piped packages are the targets |
| `ask assistant` | `null \| any` | the pipe is the turn's context; the assistant is still named (spec §7.1) |
| `connect host`, `test host`, `link host`, `add link`, `install plugin`, `grant capability` | `null` | no piped form |

## Decision

1. **A seam-answered command after a pipe is answered by its seam.** `crates/ono-cli/src/piped.rs`
   finds the first stage after the head that `remote`, `plugins` or `context` (`link host`,
   `load plugin`) claims, runs the stages before it as the native pipeline they are with their
   values captured (`native::run_collecting`, as `… | enter <target>` does since ADR-0075),
   hands those values to the seam as targets, and seeds the stages after it with what the seam
   produced — exactly the head form's path.
2. **A stream input names the targets.** Every piped value must be a record of the declared
   schema carrying the identity field (`ono.link/1` `name`, `ono.plugin/1` `id`); anything else
   is a type error naming the piped form. The stage's own options apply to every target
   (`get link | set link --transport local`). One `ono.action-result/1` per target; a target the
   seam refuses is a `failed` row carrying the error, and any failed row makes the run's status 1
   (spec §16.5, ADR-0006) — the bulk rule, not the head form's single refusal.
3. **`rename link` takes exactly one piped link**, and the positional that would be `name` in
   the head form is the new name. Zero or several piped links is a type error.
4. **`input: "null"` is refused before anything runs**, with a type error (`type.mismatch`,
   E0201) that says how the head form is spelled: "`connect host` takes nothing from the pipe:
   name the host — `connect host <name>`". Never E0101: the command exists. The refusal comes
   before the stages to the left run, because they may have effects and what was asked for
   cannot happen.
5. **`ask assistant` keeps its head-form answer.** No assistant package loads in this build
   (ADR-0111 §3), so the piped context has nowhere to go yet; the stage's refusal (`needs the
   assistant to ask` / `no loaded assistant answers to …`, E0102) is what the piped form answers.
   When an assistant runtime arrives, the captured values become the turn's context here.
6. **`load plugin` piped loads each package** with the stage's options; a package named on the
   stage as well is a type error (from the pipe or by name, not both). `load plugin` prints its
   summary and produces no records in this build, so stages after it see an empty stream.

## Consequences

- `crates/ono-cli/src/piped.rs` (new), `remote.rs` (`answer_piped`, `bind`/`act` split,
  `no_stream_input`, `piped_names`), `plugins.rs` (`run_piped`, `load_piped`), the hook in
  `eval.rs` after the `… | enter` block.
- Encoded by `crates/ono-cli/tests/remote_missing.rs` (`should_remove_the_piped_links_…`,
  `should_detach_the_piped_link_…`, `should_modify_the_piped_links_…`,
  `should_rename_the_piped_link_…`,
  `should_refuse_a_piped_host_with_a_type_error_naming_the_head_form_…`) and
  `crates/ono-cli/tests/plugins_missing.rs` (`should_verify_the_piped_packages_…`,
  `should_load_the_piped_packages_…`, `should_refuse_to_ask_when_no_assistant_is_named_…`).
- Observed while writing the detach case, not fixed here: inside a link frame, `get link` is
  sent to the other side (spec §14.4) and lists the remote agent's empty link table, so `get
  link | detach link` cannot be spelled from inside the link it would detach. Whether `get link`
  should always describe *this* session is a separate decision (noted in `docs/STATE.md`).
- `set`/`remove` of other targets (`set plugin`, `remove plugin`, `unload plugin`) go through
  the registry's `ProviderMutation` (commit 7ec0d83) and are not touched by this ADR.

## Alternatives considered

- **Registering the seams as `CommandImpl`s in the registry** so the piped form arrives as
  ordinary input — rejected: the seams need `&mut Session` (the link table, the frame stack,
  the KUANG/11 host), which the command table deliberately does not see (ADR-0104).
- **Refusing every piped form with "use the head form"** — rejected: the contracts declare the
  stream inputs, and a contract the shell refuses to honour is drift `spec-check` should catch.
