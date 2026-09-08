# ADR-0612: The six temporal crates, the store beneath them, and the version the tranche carries

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §31.1, §31.2, §31.4, §39, §42, §43.1; v0.4.1 §56; ADR-0001, ADR-0178, ADR-0610
- Decided by: agent (autonomous)

## Context

v0.5 §39 opens by forbidding one outcome — "v0.5 MUST NOT turn `ono-cli` into the temporal
engine" — and then fixes the split that prevents it, crate by crate. §55.7 repeats it as a named
failure mode. The repository already enforces crate layering as data
(`docs/contracts/hardening/module_architecture.yaml`, checked by `xtask/src/architecture.rs`), so
the split is expressible where the gate can see it rather than only in prose.

Three questions had to be answered before any temporal code could be written, because ten
parallel work packages compile against the answers.

## Decision

### 1. Six crates, and the layer each one sits in

| crate | layer | what it owns |
|---|---|---|
| `ono-temporal-core` | capability | event, evidence, coverage, gap, causal link, time selector, temporal context, the ledger contract, the value bridge, the error constructors |
| `ono-temporal-ledger` | capability | the bounded in-memory ledger and the SQLite store: append, query, retention, migration, integrity, source sequences |
| `ono-temporal-reconstruct` | capability | checkpoints, event replay, object and relation reconstruction, coverage composition, gap propagation |
| `ono-temporal-query` | capability | timeline planning, changes, event search, causal graph planning, relevance ranking |
| `ono-temporal-render` | runtime | timeline text, causal explanation, coverage-gap frames, the temporal HUD, the full-screen timeline |
| `ono-recorder` | surface | provider subscriptions, bounded ingestion, checkpoint scheduling, downtime gaps, redaction |

`ono-temporal-render` is in the **runtime** layer and depends on `ono-value` alone, which is the
shape `ono-spatial-render` already has. §39.3 requires that a renderer query no provider, no
ledger, no network. Placing it below the crates that hold those things makes the requirement a
property of the dependency graph: the code that would violate it does not compile.

`ono-recorder` is in the **surface** layer because it subscribes to concrete providers, which is
what that layer is for. Nothing depends on `ono-cli`; that is §30.4's architecture inversion and
the layering already refuses it.

### 2. The ledger contract lives in `ono-temporal-core`

`LedgerRead` and `LedgerWrite` are declared in `ono-temporal-core`, and `ono-temporal-ledger`
implements them. §39.4 says `ono-temporal-core` must expose no SQLite type and no SQL semantics;
this satisfies it in the strong direction — the trait names no connection, no path and no
statement — and buys something the split alone would not: `ono-temporal-reconstruct` and
`ono-temporal-query` depend on the contract rather than on the store, so they are tested against
an in-memory ledger with no file and no database, and a future alternative backend is a second
implementation rather than a second set of callers.

The bounded in-memory session ledger of §10.7 lives beside the contract, in
`ono-temporal-core`, because a session that never enables recording still has temporal features
and must not link a database to have them.

### 3. The reference store is bundled SQLite, and the payload is CBOR

§31.1 makes SQLite in WAL mode normative for the reference implementation, so that recovery,
migration and corruption behaviour are deterministic wherever they are tested. `rusqlite` with
the `bundled` feature compiles the amalgamation in rather than linking whatever the host
distribution ships: the store's format, its WAL semantics and its integrity check are inputs to
the acceptance suite, and a distribution's SQLite is not the same input twice. The cost is a C
compilation the builder image already has a compiler for.

§31.4 asks for a versioned binary payload beside the indexed relational scalars; `ciborium` reads
back a document whose unknown fields survive a schema evolution instead of failing the row, which
is what §31.4's last sentence asks for.

### 4. The workspace declares 0.5.0 while the tranche is being built

`xtask spec-check` requires `docs/releases/v<workspace version>.md` to exist and to name every
open box of `docs/ACCEPTANCE.md`, so that a reader learns what is unfinished from the notes or
from nowhere. `docs/ACCEPTANCE.md` §4.11 is the v0.5 definition of done and its boxes open the
moment it is written. Leaving the workspace at 0.4.4 would have put v0.5's unfinished work into
the notes of a release that shipped, which is the one thing those notes must not say.

So the workspace declares `0.5.0` and `docs/releases/v0.5.0.md` opens by stating that the tranche
is in progress and enumerating every open box. The version is a statement about what is being
built on the `implementation` branch. It is not a release: no tag is created, nothing is
published, and nothing is promoted to `main` — AGENTS.md §12.1 leaves all three to the user.

## Consequences

- A crate added to the tranche must be placed in `module_architecture.yaml`'s layering in the
  same commit, or the gate refuses it.
- `ono-temporal-render`'s test suite can construct its input by hand, because its input is a
  `RecordValue` and nothing more. Renderer tests need no ledger and no provider.
- Two dependencies enter the graph, both MIT, both already permitted by `deny.toml`'s licence
  allowlist. Neither is cryptographic, so neither needs a `supply-chain` register entry.
- The bundled amalgamation adds a C compile to a cold build of `ono-temporal-ledger`. It is paid
  once per build directory.
- `docs/releases/v0.5.0.md` shrinks as boxes are ticked, and is the document that becomes the
  actual release note if the user chooses to promote the work.

## Alternatives considered

- **One `ono-temporal` crate.** It is what §55.7 warns about one level up: with no boundary, the
  renderer reaches the ledger because it is in scope, and the rule that forbids it becomes a
  convention nobody checks.
- **The ledger contract in `ono-temporal-ledger`.** Then `reconstruct` and `query` depend on the
  crate that owns SQLite in order to name a trait, and their tests link a database to exercise
  pure replay logic.
- **A system SQLite.** The version differs by distribution, and §31.7's corruption behaviour and
  §31.6's migration matrix are tested against exactly one implementation or against none.
- **Keeping the workspace at 0.4.4.** It puts unfinished v0.5 work into a shipped release's
  notes, or it demands that §4.11 not be written until it can be written entirely ticked — and a
  definition of done that arrives after the work is not one.
