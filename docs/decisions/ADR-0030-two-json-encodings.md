# ADR-0030: Two JSON encodings, two jobs

- Status: accepted
- Date: 2026-08-26
- Spec refs: §12.2, §12.3, §33.5, §35.3; ADR-0016
- Decided by: agent (autonomous)

## Context

`to json` wrote the internal tagged encoding — `{"$record": {"schema": …, "fields": …,
"provenance": …}}`, `{"$bytesize": 1024}` — and two tests pinned it. Spec §33.5 shows what the
user-facing command must write instead: an array of plain objects, `memory` as the bare number
`1288490188`, no envelope. Its closing statement says it outright: "JSON is a serializer, not the
data model."

Both encodings are right, for different readers. The tagged one is lossless and round-trippable,
which the KUANG/11 protocol and remote links need — a `ByteSize` that came back as a bare `Int`
would have lost its meaning. The interop one is what `jq`, `curl`-fed services and every other
tool expect — a reader that has never heard of Ono.

## Decision

**`ono_value` carries both, named by their job.**

- `to_json` / `from_json` — the tagged, lossless codec, unchanged. For the protocol, remote
  links, and anywhere Ono talks to Ono.
- `to_json_data` / `to_yaml_data` — the interop encoding of §33.5. **This is what `to json` and
  `to yaml` write.**

Interop rules: a record becomes a plain object of its fields, extras merged in with declared
fields winning a name collision (dropping them would silently lose data, spec §35.3), no
`schema`, no `provenance` — provenance stays reachable through `inspect` (spec §10.7). Scalars
take their natural JSON form: `ByteSize` → integer bytes, `Percent`/`Port` → number, `Decimal` →
number where exactly representable, `Duration`/`Timestamp`/`Uuid`/`Ip`/`IpNetwork`/`Regex`/
`Path` → canonical string. `Bytes` and non-UTF-8 paths hex-encode, because JSON has no byte type
and §12.2 forbids losing undecodable bytes. An `Int` beyond `i64` and a non-finite `Float` become
their canonical string — a string the reader must parse beats a number that is wrong. An error
value becomes `{"error": {"code", "message"}}`, so a foreign reader can see a field failed
without learning Ono's diagnostic apparatus.

`to json | from json` is therefore deliberately one-way for schema identity: the data survives,
the Ono-ness does not. That is not a defect — it is the boundary spec §12.3 makes explicit, and a
test pins that numbers come back as numbers and strings as strings.

`to csv` and `to text` already wrote canonical text (`2048B`) and are pinned by a test rather
than changed.

## Consequences

- `get process | to json | jq .` works, which is the interop §12.3 promises.
- The two encodings can never drift into each other unnoticed: separate functions, separate
  tests (`crates/ono-value/tests/data_codecs.rs`, 17 cases including §33.5 verbatim).
- Known and accepted: object keys serialise alphabetically, not in schema order, because
  `serde_json`'s map is a BTreeMap. Deterministic, but not §33.5's `pid, name, cpu…` order.
  Fixing it means enabling `preserve_order` workspace-wide, which also reorders the protocol;
  deferred to its own increment on `docs/STATE.md`.

## Alternatives considered

- **Make `to_json` itself untagged.** Rejected: breaks `from_json`, the KUANG/11 protocol and
  the remote link in one move, and loses the only lossless representation.
- **An option on `to json` choosing the encoding.** Rejected: the wrong shape reachable by
  default, and surface the spec never asked for.
- **An `extra` sub-object instead of merging.** Rejected: an envelope is the defect, smaller.
