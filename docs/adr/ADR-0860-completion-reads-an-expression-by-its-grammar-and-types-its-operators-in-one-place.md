# ADR-0860: Completion reads an expression by its grammar, and the operators a field type offers are typed in one place

- Status: accepted
- Date: 2026-09-12
- Spec refs: spec §15.1, §15.2, §26; v0.6.1 §9–§21; issues #133, #134, #135, #136
- Decided by: agent (autonomous)

## Context

Completion inside an expression-mode argument read the line as whitespace-separated words and
bound each word to the next declared selector (`next_selector` in `crates/ono-command/src/complete.rs`).
`where` declares one selector, so after its first word nothing was left to complete: no operator
after a field, no `and`/`or` after a comparison, no field after `and`. The REPL then fell back to the
working directory, because its guard tested whether the registry had *answered*, not whether a
path could *stand* there (#133). `where size>1GB` was a single word, so none of it could apply to
compact syntax either (#134).

Answering those positions needs a relation no source held. The grammar
(`docs/contracts/grammar.ebnf`, the parser's `BinaryOp`) fixes the operator vocabulary;
`docs/contracts/language.yaml` documents each operator and each unit suffix; the evaluator
(`crate::expr`, and its twin in `ono-cli`) decides what executes. None of them says which
comparisons are worth offering after a field of a given declared type. The check
(`crates/ono-command/src/check.rs`) does not type operators at all.

v0.6.1 §13 requires that completion never advertise an operator the evaluator cannot execute, and
§21 forbids a second grammar, a duplicate type system or a duplicate operator registry.

## Decision

1. **Position before provider.** `ono_command::complete` first decides what can stand at the
   cursor and only then asks the source that knows it. In an expression-mode stage the position is
   read from the canonical lexer's tokens (`ono_parser::tokens`) — the same tokens the parser and
   the highlighter use — by a small state machine that follows the grammar's expression chain:
   operand → operator → comparand → connector → operand, with `not`, `not in`, `in [ … ]`,
   parentheses and `.` into a record field. There is no completion grammar of its own; the state
   machine only walks the token classes the lexer already produces. The token under the cursor is
   the lexer's, joined with what a user types in pieces (`!~` on the way to `!~=`, `5G` on the way
   to `5GiB`), so `size>5G` and `size > 5G` read alike.

2. **Path eligibility is syntactic.** `ono_command::accepts_path` answers from the same position:
   no path in command position, in a verb's target position, for a closed set, or in a command's
   expression-mode argument; a path wherever an argument of an undescribed program, an open
   selector type or a statement inside a block can stand. The REPL offers file names only where it
   says yes. An empty answer from the registry is never a reason on its own.

3. **The operators a field type offers are typed once**, in `ono_command::comparisons_for`
   (`crates/ono-command/src/operators.rs`):

   | Field type | Offered |
   |---|---|
   | `int` `float` `decimal` `bytesize` `percent` `duration` `timestamp` `port`, `enum` (ordered, ADR-0222) | `==` `!=` `<` `<=` `>` `>=` `in` `not in` |
   | `string` `path` | `==` `!=` `~=` `!~=` `in` `not in` |
   | `ip` `ipnetwork` `uuid` `ref` | `==` `!=` `in` `not in` |
   | `bool`, and `bytes` `regex` `list` `map` `record` `error` | `==` `!=` |
   | `any` | every comparison and membership operator |

   After a `bool` or `any` field, `and` and `or` are offered too, because the field is a predicate
   on its own. The relation is a deliberate **subset** of what the evaluator executes: ordering
   text is executable but is a byte order nobody means when filtering, so it is not offered.
   `crates/ono-command/tests/operator_typing.rs` evaluates every offered pair on a sample value of
   the type, so the table cannot promise an operator the evaluator refuses.

4. **Docs and units come from the language contract.** Operator docs are `language.yaml`'s
   `operators.expression[].doc`; unit suffixes and their docs are its `unit_dimensions`. The file is
   embedded with `include_str!` and parsed with `serde_yaml_ng` the first time completion asks —
   not transcoded at build time like the registries (ADR-0571), because it is never read at startup
   and one of its maps has keys JSON cannot carry. A unit test holds every listed suffix against
   the lexer's reading of `5<suffix>`.

5. **Values are offered only from closed or dimensional domains.** After a comparison: an enum
   field's declared variants and `true`/`false`, matched without regard to case and inserted as
   declared (`ESTA` finds `established`); after a number compared with a `bytesize`, `duration` or
   `percent` field, that dimension's units; after `in`, the opening `[`, and inside it the same
   element values. Open domains — text, addresses — offer nothing; that would be a provider's
   answer, which v0.6.1 §14 leaves out of scope.

6. **Unreadable input fails conservatively.** A token sequence the state machine cannot place
   (an unknown field, arithmetic, a literal on the left, an open string) yields no candidates and
   no paths (v0.6.1 §20).

7. **Candidates carry their doc to the screen and not into the line.** `ono_editor::Completion`
   gains `docs`, index for index with `candidates`; on the second Tab a documented listing shows
   one candidate per line with its doc in a column, flattened to one line, control characters
   escaped, the whole line shortened to the terminal width. Insertion reads `candidates` only
   (#136).

## Consequences

- `select` reads field paths one after another and offers another field after one; `sort` and
  `group` offer the fields where their key starts and nothing after it.
- The evaluator's acceptance and the completion table can drift only in the safe direction: a
  new evaluator capability goes unadvertised until the table names it, and a table entry the
  evaluator cannot execute turns `operator_typing.rs` red.
- The check still does not type operators. Typing it would reject scripts that run today
  (`where name > "m"`), which v0.6.1 §32 rules out for a patch release. If a later release types
  the check, `comparisons_for` is the table it should read.
- A plugin-contributed schema completes fields and operators exactly like a built-in one once the
  REPL's planned upstream schema reaches the context (`StageContext::with_schema`).

## Alternatives considered

- **Keep word counting and add a "repeatable predicate" selector.** Rejected: it cannot see
  `size>1GB` as three tokens, and it still has no notion of operator versus comparand.
- **A completion-specific expression grammar.** Rejected by v0.6.1 §21; the lexer already
  classifies every token the state machine needs.
- **Put the type–operator table in `language.yaml`.** Considered; rejected for now because the
  table's authority is the evaluator, not the documentation, and a contract entry would need a
  second parser for field type names. The Rust table sits beside the evaluator it is tested
  against.
- **Offer everything the evaluator accepts.** Rejected: it would offer `<` for strings and uuids,
  which is executable and misleading.
