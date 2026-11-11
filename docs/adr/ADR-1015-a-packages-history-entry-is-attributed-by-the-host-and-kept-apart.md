# ADR-1015: A package's history entry is attributed by the host and kept apart from the operator's

- Status: accepted
- Date: 2026-11-10
- Spec refs: §17.5, §20.1, §31.7 (`history.write: {scope: plugin}`), §31.12, §31.20;
  K11P §7.1; `docs/contracts/kuang/protocol.v1.yaml` (`history.append`, `history.query`);
  `docs/contracts/kuang/capabilities.v1.yaml` (`history.write`); issue #203; ADR-0015 T8,
  ADR-0022, ADR-0568
- Decided by: agent (autonomous)

## Context

`history.append` answered `provider.unavailable` because the history record had no field for a
package's authorship (issue #203). The contract fixes what must hold — "Attribution to the package
is set by the host and cannot be supplied", "A package cannot write an entry attributed to the
operator or to another package" — and leaves the record, the store and the bounds open.

Two facts make the store a security decision rather than a filing one:

- The operator's history is what the prompt recalls from. An entry a package wrote, sitting in that
  file, would come back under the arrow key as if the operator had typed it — `rm -rf ~/work` one
  keystroke from running, attributed to nobody on the screen.
- `history.write` is extension-local authority, included without a question (K11P §7.1,
  `permissions.v1.yaml`). Every package that declares it holds it. A package that could append
  to the operator's file without bound could trim the operator's own history out of existence,
  because the file keeps its newest 20 000 entries.

## Decision

1. **The entry records its author.** `ono_history::Entry` gains `package`: the id of the package
   that wrote it, absent for the operator's own. Its `cwd` becomes optional, absent for a package's
   entry, which ran in no directory of the session's; nothing is invented to fill it (§35.3). Both
   fields are optional on disk, so every line written before them reads as the operator's own,
   exactly as it was written.
2. **The host sets the author; the package cannot.** The entry a package sends is
   `{command, status?, duration_ms?}`. `package`, and every field the host sets (`id`, `at`, `cwd`,
   `session`), is refused with `runtime.schema_violation` and nothing is kept — rather than
   overwritten — because a package that names an author is confused or forging, and a silent
   rewrite would let it believe something false. Any other field is refused the same way; a
   missing or empty `command`, one over 4096 bytes, a `status` outside 0–255 or a `duration_ms`
   that is not a count is malformed.
3. **A package's entries are kept apart.** Each package has its own file,
   `<state>/ono/history-packages/<package-id>.jsonl`, beside the operator's `history.jsonl` and
   never in it. The prompt recalls only the operator's file, so a package's entry is never offered
   as a command to run again, and no package can crowd the operator's history out. Each package
   keeps its newest 1000 entries, so one package rolling its own log loses only its own oldest
   lines.
4. **Redaction applies before the write.** The text is scrubbed with the history's own patterns
   and the package boundary's wider net (ADR-0015 T8) before it reaches the file, and a leading
   space hides nothing: a package's entry is kept or refused, never dropped quietly.
5. **`history.query` reads both.** A reader with `history.read` gets the operator's entries and
   every package's, oldest first, each with `package` (null for the operator) and `cwd` (null for
   a package), redacted again at delivery as before.
6. **Unloading or removing a package does not rewrite history.** History is the record of what
   happened; the package's entries stay, attributed, until its own bound drops them.

## Consequences

- A package can leave a durable, attributed trace of what it did, and the operator's history is
  exactly as trustworthy as it was before packages could write.
- `Entry::cwd` now answers `Option<&Path>`; its one caller outside the crate changed with it.
- Two shells writing one package's file at once share the multi-writer behaviour the operator's
  file already has: appends interleave, and a rewrite after trimming keeps what that process read.
- Tests: `crates/ono-history/tests/history.rs` (`should_attribute_an_entry_to_the_package_…`,
  `should_read_an_entry_written_before_attribution_existed_…`,
  `should_redact_a_packages_entry_…`); `crates/ono-cli/tests/host_services.rs` (the six
  `history.append` cases: attribution to a reader, kept out of recall and across a restart,
  forged authors, malformed entries, redaction on disk, a denied permission); acceptance case 177.

## Alternatives considered

- **One file, a `package` field on the line.** Rejected: the prompt would recall package lines
  unless every reader filtered them, an earlier build reading the file would recall them as the
  operator's, and an extension-local grant could trim the operator's history away.
- **Overwriting a supplied author, as `temporal.contribute.events` overwrites `source`.** Rejected
  here: that contract says the host *sets* the source; this one says attribution *cannot be
  supplied*, and a refusal tells the package so.
- **A per-call quota that refuses.** Rejected for history: a log that rolls is what an append-only
  record of recent activity is, and the bound is per package, so rolling never reaches anyone
  else's lines.
