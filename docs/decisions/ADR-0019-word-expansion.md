# ADR-0019: Word expansion — escaping, variables, tilde and globs

- Status: accepted
- Date: 2026-08-26
- Spec refs: §6.5, §12.1, §17.3, §19.2, §26.2, §38; ADR-0009, ADR-0010, ADR-0011
- Decided by: agent (autonomous)

## Context

ADR-0009 makes a words-mode argument "the maximal run of characters that are not whitespace and
not structural", retained verbatim, and leaves what happens to that text to the evaluator. Four
questions follow immediately, and every one of them is a place where shells have historically
made people's lives worse:

1. how a space, a `*` or a `$` gets into a word literally;
2. what `$NAME` expands to, and from where;
3. what `~` means;
4. what a glob does, and what it does when it matches nothing.

Spec §38 rules out POSIX *syntax* compatibility, so the answers are Ono's to choose. Spec §12.1
requires being a good Unix citizen, and spec §17.3 asks that destruction never be ambiguous.

## Decision

### Expansion order

Tilde, then variables, then globs — each applied to the result of the last, and each applied to
the word's **own source text only**.

### 1. Backslash escapes the next character

In words mode, `\` consumes the character after it and contributes that character literally,
whitespace included. `cd My\ Documents` is one argument; `echo \*` prints an asterisk; `echo \\`
prints a backslash. Inside a double-quoted string the escapes are ADR-0009's; inside a
single-quoted string there are none.

This requires the lexer to treat `\<any>` as part of a word rather than letting the whitespace
end it, which is a deliberate refinement of ADR-0009's WORD rule and is recorded in
`docs/spec/grammar.ebnf`. The alternative — quoting as the only mechanism — was rejected because
`cd My\ Documents` is muscle memory for every user this shell is meant to replace Bash for, and
a shell that silently splits that into two arguments has failed at the first directory with a
space in its name.

### 2. `$NAME` expands from shell variables, then the environment

`$NAME` and `${NAME}` expand mid-word. The lookup is the fixed order of ADR-0010: a `let`
binding in the innermost scope that has one, then the environment. `$env.NAME` names the
environment explicitly and skips shell variables. A name that resolves to nothing expands to the
empty string; whether that should be an error is a question the strict mode of spec §19.7 will
answer, and until then a shell that refuses to start a command because `$EDITOR` is unset would
be worse than one that does not.

### 3. **An expanded variable never undergoes word splitting or globbing**

`$files` is exactly one argument, whatever it contains. Spaces, newlines, asterisks and
semicolons in a variable's value are data, not syntax.

This is the single most consequential decision in this ADR. Implicit word splitting is the cause
of the whole `"$@"` versus `$@` genre of shell bug, of every filename-with-a-space data loss, and
of a large share of shell injection: it means a value's *content* can change a command's
*structure*. Ono has a list type (ADR-0009) for the case where several arguments are genuinely
wanted, and a list splices as several arguments because it *is* several values. Nothing else
does.

Spec §38 explicitly declines POSIX syntax compatibility, which is what makes this available; it
is also the direct application of spec §12.1's distinction between text and structure to the
argument list itself.

### 3a. A null becomes the empty string when it is interpolated, and `null` when it is shown

`echo "Hello $NAME"` with `NAME` unset prints `Hello `, not `Hello null`.

This is not a contradiction of spec §10.5, which requires that unknown data stay visible. That
rule is about *showing data to a person*: a table cell for an unknown value says `null`, and
`to text` writes `null`, and both are tested. An interpolated command argument is not a
rendering — it is a word being handed to a program — and putting the four letters `null` into it
would be a worse lie than putting nothing, because the program would then receive a value that
looks like data and is not.

So the boundary is exactly where the two rules meet: everything that renders shows `null`;
everything that builds an argument for a command turns it into an empty word.

### 4. A glob that matches nothing is an error

`*`, `?` and `[…]` expand against the filesystem, per path component, in sorted order. `**` is
not a pattern; it matches the same as `*` and is not a recursive descent, because a recursive
descent that looks like an ordinary glob is how people accidentally traverse a whole filesystem.
Recursion is `--recursive` on the command that means it.

A pattern that matches nothing is `io.not_found` (E0301) naming the pattern, and the command does
not run.

Bash passes an unmatched pattern through literally, which produces `ls: cannot access '*.txt'` —
an error message about a file nobody named, from a command that should not have been started. Worse,
`remove file *.tmp` in a directory with no `.tmp` files would ask a mutating command to operate
on a literal asterisk. Spec §17.3 asks that native commands receive resolved objects so that
"`remove file *.tmp` can know exact targets before mutation"; the same reasoning says an
unresolvable pattern must stop before the mutation, not travel into it as a filename.

Quoting is how a literal `*` is passed: `grep '*' file`, `echo "*"`, `echo \*`.

Hidden files are not matched by a leading `*`, as everywhere else; `.` and `..` are never
produced.

### What is deliberately absent

No command substitution inside a bare word — `( … )` is the construct, and it is a value rather
than text (ADR-0009). No brace expansion, no history expansion, no process substitution, no
implicit arithmetic. Each is a syntax that changes a command's structure from inside a word, and
each is available explicitly where it is genuinely wanted.

## Consequences

Easy: a filename with a space, a newline or an asterisk in it is safe everywhere without anyone
thinking about quoting; a typo'd glob fails before the command runs rather than inside it; a
variable holding user input cannot become syntax.

Hard: a Bash user's `for f in $files` habit does not work, and must become a list. That is the
intended trade, and the failure is immediate and legible rather than silent and occasional.
`ls *.txt` in an empty directory now errors where Bash printed a confusing message; the error
names the pattern, which is strictly more useful.

Encoded by: `crates/ono-cli/tests/expansion.rs` and the acceptance cases for quoting and globbing.

## Alternatives considered

- **POSIX word splitting** — rejected: see item 3. It is the mechanism by which data becomes
  syntax, and no amount of quoting discipline has ever made it safe in practice.
- **Unmatched glob passes through literally (Bash)** — rejected: it turns a pattern into a
  filename and defers the error to a command that cannot explain it.
- **Unmatched glob expands to nothing (`nullglob`)** — rejected: `remove file *.tmp` would become
  `remove file` with no targets, which is a mutation command with its object silently removed.
- **`**` as recursive descent** — rejected: it differs from `*` by one character and by several
  orders of magnitude of effect.
