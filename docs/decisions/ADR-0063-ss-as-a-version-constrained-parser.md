# ADR-0063: `ss` as a version-constrained parser

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.9 (tier C), §1.10, §1.25, §1.32, §1.46, §1.69 step 10; ADR-0055, ADR-0056, ADR-0062
- Decided by: agent (autonomous)

## Context

`ss` is the spec's archetypal adapter (v0.3 §1.32) and has no machine format: `-H -O` gives
one socket per line with columns ss documents but pads by width, the Netid column appears
only when more than one socket type is listed, and the process, user and inode live in
`key:value` words after the endpoints. Its own spelling is `ss -tunap`, one word for five
flags. Tier C means: pinned versions, fixtures, locale forced, brittle-parser metadata, fast
refusal on unknown shapes (v0.3 §1.9).

## Decision

1. **`ss-text-v6`**, a builtin decoder marked `version-constrained` and pinned to iproute2
   `>=5.0 <7`, reads columns by position and vocabulary — a State keyword in the first
   column says the Netid column is absent — and never by width. A line that starts with
   neither a socket type nor a state ss prints, or that has no endpoints, is
   `adapter.decode_failed`; no field is shifted to make it fit (v0.3 §1.10).
2. **Three adapters share it.** `ss-tcp` (requires `-t`) and `ss-udp` (requires `-u`) set the
   protocol as a literal because ss omits the column for a single type; `ss` (bare, or `-t -u`
   together) reads it. Ranking prefers the adapter with the required flag: invocation
   specificity now counts required flags as well as matched words (v0.3 §1.25 criterion 4).
3. **`match.flags.combined`** reads `-tunap` as `-t -u -n -a -p`; every letter must be allowed
   and an undeclared one is named in the refusal. The plan always adds `-n` and `-e`, so ports
   are numbers and uid/inode are present whatever the user typed.
4. **Endpoints are nested `ono.endpoint/1` records.** Coercion into a `record` field builds
   the nested record against its schema with the socket's provenance; `*` is a null address
   or port, never zero; a `%scope` on the address goes to the extension map; unix sockets
   have null endpoints and their inode from the peer column.
5. **`state` for an unconnected UDP socket is null**, because the schema's vocabulary has no
   such state and `unknown` would be a claim (spec §10.5). `process` is the owning pid where
   `-p` and permissions allow, with the name in the extension map, and null otherwise.
6. **Out of scope and raw**: `-x`, `-w`, `-0`, `-A`, filters (`state …`, `dport …`), `-r`.

## Consequences

- `ss -tunap | where state == "established" | select local remote process` composes
  (v0.3 §1.32); `ss | grep` stays bytes; `explain` and provenance say the parser is
  version-constrained; iproute2 7 will be refused until a fixture proves it.
- Tests: the conformance harness over `docs/spec/adapters/fixtures/iproute2/ss*/` (with and
  without the Netid column, IPv6, unix, scope, a header left in, garbage),
  `ono-adapter/tests/{negotiation,decode}.rs`, `ono-cli/tests/adapters.rs`, acceptance case
  `081`.
