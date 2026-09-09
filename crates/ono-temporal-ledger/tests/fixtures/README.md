# Committed store fixtures

`ledger-v1.sqlite3` is a real store written at **store version 1**, so §31.6's requirement that
migrations are "tested against fixtures from every shipped v0.5 store version" is exercised rather
than asserted. It holds three events, two evidence records, one causal link, one coverage interval,
one action whose command summary was redacted, and one checkpoint with an object and a relation.

It was produced by writing that history through `LedgerStore`, then rolling the store back to what
version 1 was — version 2 adds only `events.significance` and its index, so dropping both and
setting `metadata.store_version` to `1` yields exactly a v1 store holding this history. The
permanent definition of version 1 is `STEPS[0]` in `src/migrate.rs`, which is forward-only and
never edited.

`tests/migration.rs` reads the identities out of the fixture with plain SQL before opening it,
migrates it by opening it, and asserts that every `EventId`, `EvidenceId`, `ActionId` and causal
reference is unchanged (§31.6). Nothing regenerates the file: a fixture a test can rewrite proves
nothing about the version that shipped.
