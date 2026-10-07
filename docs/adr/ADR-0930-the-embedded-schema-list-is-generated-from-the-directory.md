# ADR-0930: The embedded schema list is generated from the directory

- Status: accepted
- Date: 2026-10-07
- Spec refs: §27, §28, §36.4, §36.5; v0.4.1 §52.2; ADR-0571
- Decided by: agent (autonomous)

## Context

`ono_value::builtin_schemas()` is the registry every provider, command and pre-flight type check
reads. Its documents were embedded through a list typed by hand in `crates/ono-value/src/builtin.rs`,
one `include_str!` per file, beside the directory `docs/contracts/schemas/` that is their source.
`limit.v1.yaml` (ADR-0461) was written into the directory and never into the list, so `ono.limit/1`
was a schema the registry could not answer for although its contract existed (issue #159).

ADR-0571's fidelity test compared what was embedded with what was on disk in one direction only —
every embedded document matches a file — and left completeness to `spec-check`, which never asked.
A hand list beside a directory is a second truth; a completeness check would catch its drift, but
generating the list removes the drift altogether (§36.4: one source of truth).

## Decision

**`crates/ono-value/build.rs` writes the list.** It already transcodes every `*.yaml` in
`docs/contracts/schemas/` to JSON (ADR-0571); it now also writes `$OUT_DIR/schema_contracts.rs`, one
`include_str!` per **schema document** in name order, and `builtin.rs` includes that file as
`CONTRACTS`.

A schema document is a file named `<stem>.v<N>.yaml`. The directory's only other file,
`deferred.yaml`, is the register of schemas not yet written (ADR-0012) and declares no schema
itself, so it is transcoded but not embedded as one.

The fidelity test gains its missing half: `should_embed_every_schema_document_the_directory_holds`
reads every schema document on disk and fails naming any whose id the binary does not carry. With
the list generated it cannot fail through forgetting; it guards the naming rule and the build
script themselves.

## Consequences

- A schema document dropped into the directory is embedded by the next build; there is no list to
  remember. `ono.limit/1` reaches the registry, so `get schema` and the pre-flight check know it.
- Nothing is added to `spec-check`: the inventory is not a parallel truth any more, so there is
  nothing for a gate rule to compare.
- A file in the directory that is neither a schema document nor `deferred.yaml` is still transcoded
  and ignored; a schema document whose name does not follow `<stem>.v<N>.yaml` would not be
  embedded. Every one of the 116 documents follows it, and `spec-check`'s schema-id rule already
  ties the file name to the declared id.

## Alternatives considered

- **Keep the hand list, and have `spec-check` compare it with the directory both ways.** Catches
  the drift a build later than generating avoids it, and keeps two lists to edit for every new
  schema.
- **Embed with a procedural macro or `include_dir`.** A new dependency for what twenty lines of the
  existing build script do.
