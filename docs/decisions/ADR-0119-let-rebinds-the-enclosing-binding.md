# ADR-0119: `let` rebinds an enclosing binding; a new name stays block-local

- Status: accepted
- Date: 2026-08-27
- Spec refs: §19.2, §19.4, §19.5
- Decided by: agent (autonomous)

## Context

ADR-0009 fixed `let` as the only binding form: "declare or rebind a binding in the current
scope". Blocks — `if`, `while`, `for`, `each`, function bodies — each open a scope of their own
(ADR-0070), and the evaluator read "current scope" as "innermost scope", so a `let` inside a
block always declared a fresh, block-local binding. The consequence, found by the wiki
verification pass: `let i = 0; while $i < 3 { echo $i; let i = $i + 1 }` never terminates,
because the body's `let i` shadows the `i` the condition reads and is dropped when the body
ends. Spec §19.5 lists `while` in the minimum useful set of control flow; a `while` that cannot
advance its own condition is not useful.

The spec is silent on scoping (§19.2 shows only top-level `let`; §19.4 says only that the
current item token should be explicit). The rule is the agent's to decide.

## Decision

`let name = …` resolves `name` innermost-scope-first, as a `$name` read does:

1. **If an enclosing scope already binds `name`, `let` rebinds that binding in place.** The
   value is visible after the block ends. This is how every shell's assignment behaves, and it
   is what makes counters, accumulators and flags set inside `if`/`while`/`for`/`each` bodies
   and function bodies work.
2. **If no scope binds `name`, `let` declares it in the innermost scope**, where it stays until
   that scope ends. A name first bound inside a block does not leak out of it.

What a block *declares* rather than assigns is unchanged and always fresh in the block's own
scope: a `for` loop variable, `each`'s `@`, a function's parameters, a `catch` name and a
`match` binding shadow an outer binding of the same name. So `let n = …` inside
`fn f(n: Int) { … }` rebinds the parameter, not the caller's `n`.

There is no `local` keyword and none is added: spec §19.5 says shell ergonomics SHOULD avoid
turning Ono into a general-purpose language too early, and the rule above needs no new syntax.

## Consequences

- `crates/ono-cli/src/session.rs`: `Session::assign` implements the rule; `Session::bind`
  remains the declaring form the block constructs use.
- Encoded by `crates/ono-cli/tests/language_missing.rs`:
  `should_rebind_an_enclosing_binding_when_let_names_it_inside_a_loop_body`,
  `…_inside_an_if_branch`, `should_keep_a_name_first_bound_inside_a_block_local_to_that_block`,
  `should_let_a_function_body_rebind_a_binding_of_the_calling_scope`.
- A function that wants a private working variable of a name the caller also uses must take it
  as a parameter. If that turns out to bite, a `local`-style form is the ADR to write; it would
  be additive.

## Alternatives considered

- **Keep `let` block-local everywhere and add an assignment operator** (`i = $i + 1`) — rejected
  by ADR-0009: a second assignment form collides with `--option=value` and with the `set` verb.
- **Function bodies as a hard scope boundary** (`let` inside a function never reaches the
  caller) — rejected for now: it would make the same statement mean two things depending on the
  enclosing construct, and shells do not do it without an explicit `local`.
