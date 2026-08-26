# ADR-0009: The concrete Ono grammar

- Status: accepted
- Date: 2026-08-26
- Spec refs: §6.1–§6.5, §12.5, §19, §26, §26.1, §26.2, §38
- Decided by: agent (autonomous)

## Context

Spec §26 is "intentionally a sketch, not a frozen grammar", and §26.1 names the hard problem
without solving it: bare words are convenient in command position and ruinous in expression
position. `get file ./src` wants `./src` to be a path word; `where cpu > 20` wants `>` to be a
comparison; `cat log > out.txt` wants `>` to be a redirection. A single uniform grammar cannot
have all three, and every ambiguity left open here becomes a class of user-visible surprise.

The parser must also parse a *partial* line for the editor (§24.4), so the resolution cannot
depend on anything the parser does not know at keystroke time — in particular it cannot depend
on a loaded command registry, provider schemas or `PATH`.

## Decision

### The two argument modes

A command's head word selects how its arguments are lexed and parsed. The choice is a static,
syntactic property of the head, known before any registry is loaded:

| Mode | Heads | `>` `<` mean | bare words are |
|---|---|---|---|
| **Words** | everything not listed below — external commands and native verb-target commands | redirection | word atoms (`Word`) |
| **Expression** | `where select sort group take skip each reduce count measure join diff if elif while until let return match` | comparison | field paths |

`to`, `from` and `format` are **Words** mode: their arguments are format names and options, never
expressions. This is what makes `get process | to json > out.json` parse, and it agrees with
spec §12.5, which requires a serializer before a structured pipeline may reach a byte sink —
so a redirection after `where` is a semantic error in any case and loses nothing by being a
syntax error too.

Consequences that fall out, and are intended:

- redirection appears only on Words-mode stages, which are exactly the stages that produce bytes;
- comparison appears only in Expression-mode arguments and inside `( … )`, which are exactly the
  places a comparison can mean anything;
- neither construct needs whitespace heuristics such as "`>file` redirects, `> file` compares",
  which are unteachable and break on paste.

An Expression-mode argument may still contain a word: an identifier that is not a keyword and is
not followed by `(` parses as a field path, so `select pid name cpu` and `sort cpu desc` work
without quoting.

A Words-mode argument may still contain an expression, by parenthesising it:
`echo (get process | count)`.

Registered commands may declare their mode in `docs/spec/commands.yaml` (phase D); the table
above is the parser's built-in default and the only thing it needs at keystroke time.

### Lexical rules

- **Word** — the maximal run of characters that are not whitespace, not a structural character
  (`| ( ) [ ] { } ; , ' "` and, in Words mode, `< >`) and not a comment start. So `a-b`, `./src`,
  `--recursive`, `-la`, `/usr/bin`, `1.2.3`, `user@host` and `*.tmp` are each one word. A word is
  reinterpreted as a number, unit literal, keyword or field path only where the grammar asks for
  one; its exact source text is always retained so an external command receives what was typed.
- **Comment** — `#` to end of line, when `#` begins a token.
- **Strings** — `"…"` supports `\\ \" \n \r \t \0 \e \xNN \u{…}` and interpolates `$name`,
  `$name.field` and `$( … )`; `'…'` is raw: no escapes, no interpolation, and the only way to
  write a literal `'` is to use the double-quoted form. Unterminated strings are `parse.incomplete`
  (E0002), never `parse.syntax`, so the editor can tell "still typing" from "wrong".
- **Numbers and units** — `42`, `-7`, `1.5`, `0x1f`, `0b1010`, `1_000_000`, and a numeric literal
  immediately followed by a unit suffix is a semantic scalar (§10.6): byte sizes
  `B KiB MiB GiB TiB PiB KB MB GB TB PB`, durations `ns us ms s m h d w`, and `%`. Adjacency is
  required: `512 MiB` is two tokens, `512MiB` is one `ByteSize`.
- **Regex** — `/…/flags` is a regex literal only in Expression mode at operand position (start of
  an operand, or directly after an operator, `(`, `[` or `,`). In Words mode `/etc/passwd` is a
  path word. Flags: `i m s x`.
- **Variables** — `$name`, and `$name.field.field` for record access. `$env` is the environment
  record, so `$env.PATH` is the environment variable. A bare `$NAME` resolves the shell variable
  if one is bound and otherwise the environment variable of that name; the lookup order is
  fixed and inspectable via `explain`.
- **Current value** — `@` is the current value: the item bound by the enclosing block (§19.4),
  or, at the prompt with no enclosing block, the interactive selection (§6.4). `@-1`, `@-2` are
  previous pipeline results and `@3` is item 3 of the current result (§6.4).
- **Namespaces** — `ono:get`, `exec:ls` force resolution namespace (§6.5).
- **File descriptors** — a redirection may be prefixed by a decimal fd with no space: `2>`, `2>>`,
  `2>&1`, `<&0`.

### Grammar

The authoritative form is `docs/spec/grammar.ebnf`, committed with this ADR and checked by
`cargo xtask spec-check`. In outline:

```ebnf
program    = statement* ;
statement  = ( pipeline | let | fn | control ) terminator? ;
pipeline   = and_or [ "&" ] ;
and_or     = stage_list { ( "&&" | "||" ) stage_list } ;
stage_list = stage { "|" stage } ;
stage      = head argument* redirection* ;
```

`&&` and `||` chain *pipelines* on exit status; `and`/`or` combine *expressions* on truth. They
are different operators at different levels and never collide. Spec §38 rules out POSIX *syntax*
compatibility, not the two operators every user's fingers already know for status chaining.

### Statement forms

```text
let name = pipeline            declare or rebind a binding in the current scope (§19.2)
fn name(param: Type = default, …) -> Type { … }      (§19.3)
if expr { … } else if expr { … } else { … }
for name in expr { … }
while expr { … }
match expr { pattern => { … }, … }
try { … } catch name { … }
return expr? | break | continue
use module                     (§19.6)
```

`let` is the only binding form; rebinding is a further `let` in the same scope, which keeps the
language free of a second assignment operator that would collide with `--option=value` and with
the `set` verb (§7), whose targets are system objects rather than variables.

### Blocks and records

`{ … }` is a **record** when the first significant token after `{` is an identifier or string
followed by `:`, and `{}` is the empty record; otherwise it is a **block**. This is a one-token
lookahead the incremental parser can always perform, and it puts the common cases —
`{name: "x", port: 80}` and `each { restart service @ }` — on opposite sides of a rule a user can
state in one sentence.

### Recovery

The parser always returns a tree. Every construct has a recovery point (end of stage at `|`, end
of statement at newline/`;`, closing delimiter for bracketed forms). Errors are collected, not
thrown, and the tree carries error nodes with spans, so the editor can highlight a line that is
half-typed. Input that is well-formed but unfinished yields `parse.incomplete` (E0002) with the
span of the construct still open; input that cannot become valid yields `parse.syntax` (E0001).

## Consequences

Easy: highlighting and completion work from the same tree the evaluator runs, with no registry
loaded; the two rules a user must learn (`>` redirects on byte stages, compares in transforms;
`{k: v}` is data, `{stmt}` is code) are each one sentence; external commands receive their
argv byte-exact.

Hard: `where` cannot be redirected directly — `| to text > f` is required, which spec §12.5
wanted anyway. A native command whose arguments really are expressions must be added to the
Expression-mode table, which is a parser change until `docs/spec/commands.yaml` drives it in
phase D.

Must be revisited in phase D, when the registry supplies argument modes for contributed commands
(§31.22): the built-in table becomes the default rather than the whole truth.

Encoded by: `crates/ono-parser/tests/` — the lexer corpus, the golden AST snapshots and the
diagnostic snapshots; `docs/spec/grammar.ebnf`.

## Alternatives considered

- **Uniform expression grammar everywhere** — rejected: `ls -la`, `cat a-b` and `git log --oneline`
  become arithmetic, and no amount of quoting advice repairs the first impression.
- **Whitespace-sensitive `>`** (`>file` redirects, `> file` compares) — rejected: invisible
  semantics, breaks on reformatting and on paste, and cannot be explained in a help page.
- **Nushell's `o>`/`e>` redirection spelling** — rejected: it costs every user their muscle memory
  for the single most common shell construct, to buy an ambiguity the mode split already removes.
- **Registry-driven argument parsing** — rejected as the *primary* mechanism: the editor must
  parse a line before the registry is consulted, and startup must not depend on it (§4.1, §34).
  It remains available as an extension of the built-in table.
- **`$(…)` as the only command substitution** — kept as an interpolation form inside strings,
  where it is familiar, while `( … )` is the general value form, so there is one construct to
  learn rather than two with different value semantics.
