# Contributing to Ono-Sendai

Contributions are welcome. Ono-Sendai is developed **specification-first and test-first**, and
this page is the short version of what that means in practice. The long version — the contract
the implementation agents work under, and the authoritative rules for this repository — is
[`AGENTS.md`](AGENTS.md). It applies to human contributors too.

## Building and verifying

You need Rust 1.94 or newer; `rust-toolchain.toml` pins it and Cargo picks it up automatically.

```bash
cargo build --release -p ono-cli

scripts/gate.sh            # format, lint, test, contract check, docs — the definition of done
scripts/acceptance.sh      # build a container, run every case against the real `ono` binary
scripts/release-check.sh   # both, plus the release checklist in docs/ACCEPTANCE.md
```

`scripts/gate.sh` is what every commit must pass: `cargo fmt --check`, `clippy -D warnings`, the
whole test suite, `cargo xtask spec-check` (contract-to-implementation drift, including the
checksums over the immutable specifications and the `ono` examples in `README.md`), and
`cargo doc` with warnings denied.

`scripts/acceptance.sh` is the interesting one. It builds a clean Debian image, installs `ono` as
the login shell of an unprivileged user, cuts the network, and asks the binary to prove each
advertised capability against a process table nobody tuned for the test — from "a person can do
ordinary work in ono instead of bash" to hostile filenames, live watches, remote links against a
real child agent, and a KUANG/11 package loaded under the broker.

> **A capability without a passing acceptance case is not delivered.** Write the case in the same
> change as the feature, not afterwards.

## The rules that are not negotiable

- **No production code without a failing test first.** RED → GREEN → REFACTOR → GATE → RECORD.
- **Tests assert outcomes, not structure.** A pure refactor must leave the suite green *and
  unchanged*. A test that breaks when nothing observable changed is itself the defect.
- **Never weaken a test or the harness to get green.** Not by deleting a case, not by loosening a
  match, not by removing `-D warnings`. If a case is wrong, fix the case in its own commit and say
  why.
- **The narrative specifications are immutable** — `docs/ono_sendai_*spec_v*.md` are never edited, not
  even for a typo. They are checksummed, and the gate fails if one changes. Where a specification
  is ambiguous, silent or wrong, write an ADR that records the deviation and implement your
  decision (see below).
- **One kind of change per commit.** `feat`, `fix`, `refactor`, `perf`, `test`, `docs` are
  different work and never share a commit. Conventional Commits, English, green tree.
- **`main` is never written to directly.** Implementation happens on the `implementation` branch;
  promoting it is a deliberate, separate act once the release gate passes.

## Changing behaviour, contracts or architecture

The public contract is the machine-readable registry set under `docs/spec/` — commands, verbs,
targets, schemas, errors, capabilities, providers. It is written **first**, implemented second,
and `spec-check` fails on drift between the two. Adding or changing a user-visible command means
touching the contract in the same change as the code, plus help text, completion metadata, an
inspectable output schema, structured errors and a doc example that runs.

An **architecture decision record** is required whenever a change is architectural, cross-cutting,
hard to reverse, resolves an ambiguity in a specification, or picks between real alternatives.
ADRs live in `docs/decisions/ADR-NNNN-kebab-title.md`; the format, including the mandatory
`## Spec deviation` heading for anything that departs from specified behaviour, is in
[`AGENTS.md`](AGENTS.md) §8. Read a few neighbouring ADRs before writing your first one.

Design questions the specification leaves open are answered by an ADR, not by an issue thread.

## The recordings

The GIFs in the README are recorded from the real binary over a pty and rendered frame by frame —
nothing is drawn, retouched or re-timed. If you change output that a recording shows, re-record
it: `scripts/demo/make.sh` (see [`scripts/demo/README.md`](scripts/demo/README.md)).

## Where things live

| | |
|---|---|
| [`AGENTS.md`](AGENTS.md) | the authoritative development contract — read it before your first change |
| [open issues](https://github.com/godspeed-you/ono-sendai/issues) | the backlog: one problem, one issue, with the evidence that closes it |
| [`docs/STATE.md`](docs/STATE.md) | the work board: what is in progress, what is found but not yet filed, what is deferred |
| [`docs/ACCEPTANCE.md`](docs/ACCEPTANCE.md) | what "finished" means, in boxes a script can check |
| [`docs/spec/`](docs/spec/) | the machine-readable public contract |
| [`docs/decisions/`](docs/decisions/) | every recorded decision and deliberate spec deviation |
| [`docs/reference/`](docs/reference/) | generated reference — never hand-edited |

If you are an AI implementation agent: your entry point is [`AGENTS.md`](AGENTS.md), and you
should read it in full before your first action.
