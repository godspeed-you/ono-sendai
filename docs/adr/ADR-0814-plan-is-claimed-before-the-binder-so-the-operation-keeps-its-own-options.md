# ADR-0814: `plan` is claimed before the binder, so the operation it wraps keeps its own options

- Status: accepted — corrected by ADR-0833
- Date: 2026-09-09
- Spec refs: v0.6 §5.1, §5.2, §5.3, §6.1; v0.2 §27.2, §50; ADR-0803, ADR-0813
- Decided by: agent (autonomous)

## Context

`plan` is a command whose argument is another command. Nothing else in the shell is quite that:
`at` takes a time, `each` takes an expression, `present` takes an external program.

`CommandContract::bind` refuses an option the contract does not declare, and it is right to —
that refusal is what makes `get process --nope` a typo caught at the prompt rather than a silently
ignored flag:

```text
$ ono -c 'get process --nope'
ono: Ono-Sendai-E0202 type.unknown_field  `get process` has no option `--nope`
```

`plan` declares `--protection`, `--strategy`, `--accept-risk`, `--accept-irreversible`,
`--opaque`, `--expires` and `--one-per-object`. So `plan copy file ./nginx.conf /etc/nginx/nginx.conf
--overwrite` refuses on `--overwrite`, which belongs to `copy file`. Almost every plannable
operation has an option of its own, so almost every plannable operation would be unplannable.

## Decision

**`plan` is claimed in `crates/ono-cli/src/eval/pipeline.rs` before the registry path**, the way
`at`, `now` and `present` already are, and it is reachable after a pipe through
`crates/ono-cli/src/piped.rs` so §5.3's `get service | where state == failed | plan restart service`
works.

**`plan`'s own options come first and end at the first word that is not an option.** Everything
from that word onward is the operation, resolved through `CommandRegistry::resolve` and bound
against *its* contract:

```text
plan --protection require copy file ./nginx.conf /etc/nginx/nginx.conf --overwrite
     └────── plan's ──────┘ └──────────────── the operation's ─────────────────┘
```

`sudo -u x cmd --flag` and `timeout 5 cmd --flag` already read this way, so the rule needs no
teaching.

**`plan` keeps its registry contract.** `help plan`, completion, `explain` and §27.2's binding
check all read it, and `xtask/src/bindings.rs` records that `ono-cli` binds it. What changes is
where the dispatch happens, not whether the command is declared.

The other twelve v0.6 commands are ordinary registry-bound `CommandImpl`s and need none of this:
`apply`, `impact`, `protect`, `verify`, `recover`, the four `plan` sub-verbs and the three
`recovery` ones all take a reference rather than another command.

## Consequences

The block form and the single-action form become one code path. `plan { … }` reaches the same
handler with a block expression, and each statement inside is resolved through the registry with
its own options — which is what ADR-0813 asked for, and it now costs nothing extra.

An option `plan` and its operation both declare is read as `plan`'s when it comes before the verb
and as the operation's when it comes after. There is exactly one such collision today —
`--overwrite` is not one of `plan`'s — and if a future option collided, the position is what
decides, which is stateless and explainable.

A word that is not an option and is not a known verb ends `plan`'s options and then fails to
resolve, producing `resolve.command_not_found` naming the word rather than a confusing option
error. That is the better failure of the two.

## Alternatives considered

**A separator, `plan -- copy file a b --overwrite`.** Rejected: nothing else in the shell needs
one, and §64's interaction shows `plan` reading as prose.

**Declare every option of every plannable operation on `plan`.** Rejected as absurd on its face,
and it would break the moment a plugin contributed an operation.

**Make `bind` tolerant for `plan` alone.** Rejected: the tolerance would have to live in the
binder, where it would be a hole in the check that catches every other typo in the shell.
