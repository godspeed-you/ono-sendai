# ADR-0094: Configuration is a typed catalogue, layered with provenance, and `set config` is quiet

- Status: accepted
- Date: 2026-08-27
- Spec refs: §4.1, §13.3, §16.5, §30, §43; ADR-0006, ADR-0010, ADR-0068, ADR-0070, ADR-0093
- Decided by: agent (autonomous)

## Context

ADR-0010 fixed the five configuration layers, the `ONO_*` mapping and the rule that a bad
setting never stops the shell, and `config-setting.v1` promised a typed value with the layer,
file and line that set it. What existed was a `set config` builtin that bound `config.<key>` as
a string in the session's variable scope, and nothing read it: `get config` was unbound,
`set config render.table.max_rows = 2` changed nothing anyone could see, and a config file could
set `no.such.key` without a word said. Spec §30 asks for configuration that is "declarative,
layered and inspectable", and the RED suite `meta_config_missing.rs` spells out what that means
at the prompt.

## Decision

1. **A setting is declared or it does not exist.** `crates/ono-cli/src/settings.rs` holds the
   catalogue: key, declared type (`int`, `bool`, `string`, `bytesize`), built-in default and one
   line of description. This build declares the five settings spec §30's example writes —
   `prompt.path`, `render.table.max_rows`, `history.result_cache`,
   `safety.confirm.remote_destructive`, `safety.confirm.bulk_threshold`. Each description says
   whether a component reads it; today only `render.table.max_rows` has an effect, and the
   others are recorded for the components that will read them, so a config file written from
   the spec is accepted rather than refused. A new setting is a new catalogue entry, in the
   increment that reads it.
2. **Every layer's value is kept, typed, with its provenance.** Per key the session holds the
   stack `default → system → user → environment → invocation`; the last entry is the effective
   value and `get config` reports it with `layer`, `source` (the file, for the two file layers)
   and `line`. `--overridden` shows every layer's row, effective last. `--problems` returns the
   load diagnostics as `ono.error/1` values. The one file `ONO_CONFIG` or `--config` names is
   the **user** layer; `config-setting.v1` has no sixth layer and the file stands in for the
   user's.
3. **Typing is ADR-0070's rule for typed parameters.** A bare word is read as the declared
   type (`2` → int, `64MiB` → bytesize, `true` → bool); a value — `"many"`, `$x`, `(…)` — must
   already have the type, except that an int is accepted where a bytesize is declared and
   means bytes. An undeclared key is `type.unknown_field` (E0202) with the nearest declared key
   as a suggestion; a value the type does not admit is `type.mismatch` (E0201) naming the key.
   Neither changes anything: the earlier layer's value stays in force, in a file and at the
   prompt alike. An `ONO_*` variable is read as a word, and a variable that does not parse is
   the same E0201, reported at startup and kept for `--problems`.
4. **`set config` is answered by the shell and is quiet on its own.** It runs through the seam
   of ADR-0093 rather than the builtin table. Alone — at the prompt, in a script, in a config
   file — it prints nothing, exactly as `set env` does: a settings file with twenty lines must
   not print twenty tables at every startup, and §4.1's quiet startup would otherwise be lost.
   When something consumes it (`set config … | to json`, `let r = set config …`) it emits one
   `ono.action-result/1` row, `operation = ono.config.set`, `changed` true when the effective
   value differs, `target` the setting's identity. A failed assignment is a structured error
   with exit status 1, not a `failed` row: nothing was attempted against the system.
5. **Config mode admits `set config` alone and nothing else of this seam.** `get config` in a
   config file would print, and `set config … | to json` would run a stage; both are refused
   as ADR-0010 refuses every non-declarative statement.
6. **`render.table.max_rows` defaults to 1000, and 0 means every row.** The sink applies it to
   every table it renders — at a terminal, into a pipe, into a file — and `format table`
   without `--max-rows` reads the same value; the omitted rows are announced by the renderer's
   `... N more` line (spec §13.3). Serialisation (`to json`, `to csv`) is never truncated.

## Consequences

Easy: `get config` answers spec §30's requirement from data; a typo in a config file is an
E0202 at startup that names the key and suggests the nearest one; `ONO_RENDER_TABLE_MAX_ROWS=20`
is enough for a container; a script can read a setting through `| to json` in its own type.

Hard: a setting declared but not read is a promise — its description says so, and the
increment that makes a component read one must drop that sentence. A quiet `set config` means a
user who wants confirmation writes `| to json` or `| format table`. The default of 1000 rows
truncates a very large table that used to print whole; the marker says how many were left out,
and `set config render.table.max_rows = 0` restores the old behaviour.

Encoded by: the `get config`/`set config`/truncation cases of
`crates/ono-cli/tests/meta_config_missing.rs`, and acceptance case `041-config-and-resolve`.

## Alternatives considered

- **Untyped settings, any key accepted** — rejected: `config-setting.v1` declares `type`, and
  an accepted typo is a setting the user believes is in force.
- **Failing startup on a bad setting** — rejected by ADR-0010; the diagnostic and the
  earlier layer's value are the answer.
- **A `set config` that prints its ActionResult alone** — rejected for the startup noise
  above; the row is still there for whoever asks.
- **A sixth layer `explicit` for `ONO_CONFIG`** — rejected: the schema's enum is closed and the
  file replaces the user's; reporting it as `user` with its own path is exact.
- **No default row limit** — rejected: spec §13.3 makes truncation a first-class rendering
  concern, and a setting nothing applies by default would stay untested until a user set it.
