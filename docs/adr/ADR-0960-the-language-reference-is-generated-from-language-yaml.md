# ADR-0960: The language reference is generated from language.yaml

- Status: accepted
- Date: 2026-10-16
- Spec refs: spec §6, §19, §36.2, §46.3; ADR-0009, ADR-0011, ADR-0571, ADR-0951, ADR-0958
- Decided by: agent (autonomous)

## Context

Spec §46.3 lists "language reference sections" among what the grammar's registries generate, and
the head of `docs/contracts/language.yaml` says that "the language reference [is] generated from
this file plus `grammar.ebnf`". No page was. `docs/reference/` rendered commands, verbs, targets,
schemas and errors; the language surface — keywords, operators, argument modes, literals, the
resolution order and the statement forms — reached a reader only through `help` and completion,
which embed the file (ADR-0571).

The v0.6.3 documentation audit found the consequence: functions as pipeline stages (ADR-0951),
blocks in functions and `&` jobs (ADR-0952, ADR-0958) were in no reference page at all, and the
Wiki pages that described them had nothing generated to be checked against.

## Decision

`cargo xtask docs` writes `docs/reference/language.md` from `docs/contracts/language.yaml`, and
the reference index links it.

1. **One section per top-level key of the file, in the file's order**: keywords, operators
   (expression, pipeline, redirection), argument modes, escapes, unit suffixes, literals, types,
   names (variables, the current value, namespaces), resolution order, statements, blocks and
   records, comments, built-in functions. What a key says is rendered from its `doc` fields; the
   page adds a sentence of framing per section and no rule of its own.
2. **`grammar.ebnf` is not rendered.** The file is the authority on syntax and stays readable as
   it is; `language.yaml` already restates the lexical and resolution surface it implies, which is
   what a reader looks up.
3. **The page is generated like every other**: the gate compares the committed page with the
   generator's output (spec §36.2), so a change to `language.yaml` that is not regenerated is red.
4. YAML reads `true`, `false` and `null` as non-strings; the page prints them as the words the
   language spells.

## Consequences

- The behaviour of `fn` after a stage, `&` and the resolution step for functions now has a
  generated home, and documentation outside the repository can link to it rather than restate it.
- A key added to `language.yaml` without a section here is missing from the page; that is visible
  in review rather than enforced. A check that every top-level key is rendered was weighed and not
  added: the file has fifteen keys and changes rarely.
- Encoded by `xtask/tests/reference.rs::should_render_the_language_surface_into_its_own_reference_page`
  and, for this repository, `::should_find_this_repositorys_committed_reference_docs_up_to_date_when_checked`.

## Alternatives considered

- **A hand-written language page under `docs/guides/`.** It would restate the contract and drift
  from it, which is what spec §36 generates reference pages to prevent.
- **Render `grammar.ebnf` as syntax diagrams.** §46.3 names diagrams too, but they need a
  renderer this repository does not have, and the audit's gap was the semantics in
  `language.yaml`, not the productions.
- **Fold the language into `commands.md`.** Keywords and operators are not commands; a reader
  looking up `&` would not look there.
