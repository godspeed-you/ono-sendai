# ADR-1083: `copy file <from> <to>` is the one spelling, and ADR-0813's `to` is superseded

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.6 §3.1, §5.2, §31, §64; v0.2 §7, §40.1; AGENTS.md §14 (no one-off grammar)
- Supersedes: the `copy file <from> to <to>` spelling of ADR-0813 (its decisions stand otherwise)
- Issues: #292
- Decided by: agent (autonomous)

## Context

ADR-0813 resolved v0.6 §5.2/§31's `replace file /etc/nginx/nginx.conf from ./nginx.conf` into
"`copy file ./nginx.conf to /etc/nginx/nginx.conf`", and recorded that as its *Spec deviation*. The
shell implements `copy file <from> <to>`: `ono.file.copy` declares two positional selectors
(`docs/contracts/commands/file.yaml`), and `copy file /etc/hostname to /tmp/x` is refused with
`type.mismatch` — "`copy file` takes 2 selector(s), and `/tmp/x` is one too many". Two authorities
disagree, and one of them does not run.

The evidence for which one to keep:

- **The grammar has no preposition keywords.** No command in `docs/contracts/commands/` takes a
  connective word between its selectors; arguments are positional selectors and `--options`
  (`grammar.ebnf`, ADR-0082). Every other two-object command (`move file`, `rename file`,
  `link …`) is positional.
- **`to` and `from` are verbs** (`ono.verb.to`, `ono.verb.from` in `verbs.yaml`): `to json`,
  `from json`. Making `to` punctuation inside one command's arguments would give one word two
  grammatical roles depending on which command it follows.
- **A file named `to` is a file.** Under the connective reading `copy file a to` is either "copy
  `a` to the file `to`" or "copy `a` to — nothing"; positionally it is unambiguous: the first.
  Accepting `to` would need a rule for that case, written for this one command — the one-off
  grammar AGENTS.md §14 forbids.
- **Every user-facing text already uses the positional spelling**: README, `docs/guides/change.md`,
  `docs/reference/commands.md`, the `plan` example in `docs/contracts/commands/change.yaml`
  (`plan copy file ./nginx.conf /etc/nginx/nginx.conf`, after #149's commit f5bb313d), and every
  acceptance case that copies a file (`docker/acceptance/cases/2xx–3xx`). Only ADR-0813 (and
  ADR-0820's prose, which cites it) carries `to`.

## Decision

**The spelling is `copy file <from> <to>`, as implemented.** ADR-0813's block reads, in this shell:

```text
plan {
    copy file ./nginx.conf /etc/nginx/nginx.conf
    restart service nginx
    verify service nginx state == running
    verify socket :443 exists
}
```

ADR-0813's status line names this supersession; its body is not edited, and its other decisions —
a block holds ordinary command spellings, `replace` is not a verb, `verify` lines are contracts,
`validate config nginx` is not plannable — stand.

## Spec deviation

- Section: v0.6 §5.2, §31, §64
- Text: "`replace file /etc/nginx/nginx.conf from ./nginx.conf`"
- Instead: `copy file ./nginx.conf /etc/nginx/nginx.conf` (source first, destination second, no
  connective word), replacing ADR-0813's `copy file ./nginx.conf to /etc/nginx/nginx.conf`.
- Why: the verb-target grammar has positional selectors and no preposition keywords; `to` is a
  verb; and a connective would make `copy file a to` ambiguous for a file named `to`.

## Consequences

- `crates/ono-cli/tests/files.rs::should_copy_a_file_and_report_the_result`, `…::should_copy_onto_a_file_named_to_when_the_destination_is_the_word_to`, `…::should_refuse_a_connective_between_source_and_destination`
  and `change_planning.rs::should_plan_and_apply_a_block_written_in_the_positional_copy_spelling` run the spelling
  through the binary in a scratch directory; the contract example of `ono.file.copy` and `help copy
  file` show it.
- `ono-change-actions`' library parser (`intent.rs`) still skips `to`, `from`, … as connectives in
  a plan statement it parses for its own tests; the shell's `plan` does not use that parser. It is
  reported as a finding rather than changed here.

## Alternatives considered

- Accept `copy file <from> to <to>` as ADR-0813 wrote it — rejected for the reasons above: a
  one-command connective, a verb reused as punctuation, and an ambiguity for a file named `to`.
- Accept both — two spellings of one command is the second representation of one concept the
  stabilization release forbids.
