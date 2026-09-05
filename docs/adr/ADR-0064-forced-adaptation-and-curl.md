# ADR-0064: Forced adaptation and the curl exchange

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.4, §1.18, §1.41, §1.65, §1.69 step 11; ADR-0052, ADR-0054, ADR-0057, ADR-0062
- Decided by: agent (autonomous)

## Context

ADR-0054 spelled `adapt <program> …` for spec v0.3 §1.18 — "a forced structured invocation
MUST fail rather than silently downgrade to raw text" — and left it to implement. curl is
the tool that needs it: its stdout *is* the body (§1.41), so no consumer-derived demand may
ever turn `curl … | from json` into a table, and only an explicit `adapt curl …` should
produce the exchange as an object.

## Decision

1. **`adapt` sets the demand to `Structured` whatever the consumer.** It is a stage keyword
   like `raw`; the program is the word after it; the stage is external and hands structure on,
   so `adapt ps aux | sort memory desc` binds `sort` as the transform. `explain` reports the
   demand as `` structured (`adapt` requires structure) ``.
2. **No plan is an error.** When no adapter answers, the stage fails with
   `adapter.required_for_structured_pipeline` (E0911); a refusal fails with its own
   `adapter.*` error; a bare `adapt` is not found (127), like a bare `raw`.
3. **curl answers `structured` only.** Its `output_demand` omits `interactive`, so `curl url`
   typed at a terminal still prints the body; `curl url | where …` is adapted because the
   consumer demanded it; `adapt curl url | inspect` is the §1.41 form.
4. **The exchange is one record with the body as bytes.** The plan appends curl's write-out
   after the body, preceded by a unit-separator byte (0x1f) that curl prints literally; the
   `curl-exchange-v1` decoder splits at the *last* such byte and keeps the body exact —
   builtin decoders may hand a raw byte field beside their JSON fields, which the record
   builder stores as bytes untouched. No marker means curl failed before its write-out,
   which is `adapter.decode_failed` (and usually a non-zero status first).
5. **Secrets never adapt.** `-H`, `-u`/`--user`, `-b`/`--cookie`, `-o`, `-O`, `-w`, `-D`, `-i`,
   `-I`, `-v` and `--config` are not allowed: the invocation runs raw, so a header or a
   credential never reaches provenance, history or an error's metadata (§1.41 rules).
6. **Buffering is the ask.** A structured demand means the body becomes a field, so it is
   buffered; §1.41's "do not buffer arbitrary response bodies merely to create an object" is
   honoured by the default — the plain `curl` streams — and stated as the adapter's first
   limit.

## Consequences

- `help adapt` documents the keyword beside `raw`; completion after both offers programs
  (the completion increment).
- Tests: `ono-cli/tests/adapters.rs` (bytes unless asked, the exchange, E0911, the specific
  refusal, `explain`, `help`), the conformance harness over `docs/contracts/adapters/fixtures/curl/`,
  acceptance case `082` (file scheme; networking is off in the container).
