# ADR-0313: The fuzz targets of §35.6, on a stable toolchain

- Status: accepted
- Date: 2026-08-29
- Spec refs: §35.6, §49; ADR-0245 standing rule 3, ADR-0001 (toolchain pin), ADR-0159
- Decided by: agent (autonomous)

## Context

Spec §35.6 is one sentence and it is normative:

> Fuzz parser, serializers, remote protocol, plugin protocol and procfs/netlink decoders. A shell
> consumes adversarial filenames and external output by nature.

ADR-0245 turned that into standing rule 3 — "A decoder without a fuzz target is not finished" —
and until now no fuzz target existed. What stood in were seeded property and robustness suites
(`ono-parser/tests/robustness.rs`, `ono-value/tests/codec_{fuzzing,properties}.rs`,
`ono-protocol/tests/{fuzz_protocol,framing}.rs`,
`ono-provider-netlink/tests/malformed_messages.rs`), and `docs/ACCEPTANCE.md` §4.4 ticked its box
against them. They are good suites. They are not fuzzing: each one hammers its decoder with
inputs assembled from a fixed alphabet at a fixed seed, and the set of inputs never grows. No ADR
recorded that substitution, so the project's own standing rule said one thing and the tree did
another.

The obvious answer — `cargo-fuzz` with `libFuzzer` — collides with a decision that is not this
ADR's to reverse. `rust-toolchain.toml` pins stable 1.94 (ADR-0001); `cargo-fuzz` needs nightly
for `-Z sanitizer`, and a coverage-instrumented ASan build is not something
`scripts/gate.sh` can run in seconds. A fuzz target that only runs when somebody remembers to
install a second toolchain is the dead check of ADR-0159 in a new costume: present in the tree,
green by never executing.

## Decision

### 1. A `fuzz/` workspace member, five targets, one per area

`fuzz/` is an ordinary member of the workspace — compiled by `cargo test --workspace`, linted by
`cargo clippy --all-targets`, documented under `cargo doc -D warnings`, and walked by
`xtask`'s unfinished-work scan, which already reserved the directory name. `ono_fuzz::TARGETS`
holds exactly five entries, and `fuzz/tests/corpus.rs` asserts that their areas are exactly the
five §35.6 names, so the list cannot drift from the specification without turning the gate red.

Each target is `fn(&[u8])` whose contract is "returns, without panicking, whatever it is handed",
plus the invariants the decoder promises beyond not crashing — the parser's spans stay inside the
source; the netlink decoders answer fewer records than a kernel could have sent.

### 2. A mutation engine, not a property generator, and no coverage feedback

`ono_fuzz::run` executes every corpus seed unmutated, then `iterations` mutated inputs. The
mutator is byte-level and generic — flip, interesting byte, interesting 32-bit word, insert,
delete, duplicate, splice between two corpus entries, truncate, repeat a chunk 8 to 16 384 times,
swap — with the interesting values and the repeat operator aimed at the length fields and the
nesting bombs these five decoders exist to refuse. Findings are panics, and inputs that took
longer than the per-input budget.

**This is the limit, stated plainly: there is no coverage feedback.** The engine cannot tell that
an input reached a new branch, so it cannot keep it and build on it. It finds what a large number
of structured-random inputs derived from a good corpus finds, which is less than libFuzzer finds
in the same wall-clock time. It is what a pinned stable toolchain and a seconds-long gate step
allow. What would lift it: a nightly toolchain in CI running `cargo fuzz run` for minutes rather
than the gate running it for seconds, against these same target functions — the targets are plain
functions precisely so a `libfuzzer-sys` shim over them is a wrapper and not a rewrite.

A bounded run that finds nothing has found nothing. It has not shown there is nothing to find,
and neither this ADR nor the gate's green line should be read as saying so.

### 3. The corpus is committed data, and so is every finding

`fuzz/corpus/<target>/` holds seeds — the valid netlink messages, protocol frames, manifests,
signature documents, procfs lines, command lines and codec documents the existing suites already
build, dumped to files. `fuzz/artifacts/<target>/` holds every input that ever caused a finding,
named by its SHA-256.

`fuzz/tests/corpus.rs` replays all of it on every `cargo test --workspace`, with no mutation and
no budget, so a crash found once is a regression test for ever after and a fixed one stays fixed.
That is what makes committing a crash artifact worth doing, and it is the part of a fuzzing setup
that survives the fuzzer.

### 4. A bounded run in the gate

`scripts/gate.sh` gains one step between the tests and the contracts:

```sh
cargo run --quiet --package ono-fuzz -- run --iterations 400
```

Fixed iterations rather than a time budget, so a loaded machine gets the same answer as an idle
one and a finding in CI reproduces on a developer machine from the same seed. A finding fails the
step, writes the input to `fuzz/artifacts/`, and prints the command that replays it:

```sh
cargo run -p ono-fuzz -- repro <target> fuzz/artifacts/<target>/<sha256>.bin
```

`repro` installs no panic hook and catches nothing, so what the developer gets is the backtrace.

### 5. `ono-provider-linux`'s procfs decoders become public

`parse_stat`, `parse_status_ids`, `parse_cmdline`, `service_unit`, `parse_mountinfo` and
`parse_fstab` were `pub(crate)`, reachable only by writing a temporary directory tree and driving
a provider through tokio — which is not a shape anything can fuzz at thousands of inputs a
second. They are re-exported through `ono_provider_linux::decoders`.

This is not "making an item public to test it". They are pure functions from bytes the kernel
wrote to a struct, each with a contract of its own that AGENTS.md §11 already admits as
unit-testable, and §35.6 names them by name as a thing that must be fuzzed. A decoder the
specification requires to be fuzzed must be callable by a fuzzer.

## What the first campaigns found

Five things, in about two million executions over nine seeds. Recorded here because a fuzz
harness that reports nothing is indistinguishable from one that finds nothing, and the
difference is the whole point.

| # | Target | What | Where it went |
|---|---|---|---|
| 1 | parser | `"[1".repeat(5_000)` — a chain of index suffixes — aborted the process | fixed: the depth counter was released before the postfix loop recursed |
| 2 | parser | `"- ".repeat(16_000)` — a chain of prefix operators — aborted the process | fixed: `parse_unary` recursed into itself with no guard at all |
| 3 | serializers | 100 kB of `{e: {e: {…` took seven seconds to be **refused** | fixed: the nesting is counted first, in one linear scan, and the same refusal is now instant. The bound reached `Manifest::parse`, `PackageSignature::parse`, the adapter pack reader and both KUANG/11 stores, which had the same shape |
| 4 | parser | `words_arguments("[f" + ".5".repeat(16_000))` takes **32 seconds** on 32 kB | **open** |
| 5 | plugin protocol | `Manifest::parse` on 50 kB of `{` behind an unbalanced quote took **13 seconds** — the depth guard of finding 3 tracked quoting, and a document whose quote never closes read as one long string with no nesting in it at all | fixed: the count ignores quoting, which can only over-count |

Finding 5 is the one worth reading twice. The fix for finding 3 tracked YAML quoting so that a
`{` inside a string would not be counted, and a fuzz target found the input where that model and
the parser's disagree — an unbalanced quote — inside a day. Two models of a language's quoting is
one too many; the count is naive now, which can only over-count, and over-counting refuses a
document rather than stalling on one. That is the kind of thing a fuzz target is *for*, and a
property suite with a fixed alphabet would not have produced it.

Findings 1 and 2 are the reason this ADR exists rather than the one that records a substitution.
Both are stack overflows — not panics, not errors: the process dies, and the parser in question
runs on every keystroke in the editor. Neither of the two nesting tests that already existed
reached them, and both say so in their own comments now: a repeated `[` never enters the postfix
loop, because an index needs an operand in front of it.

### Finding 4 is open

`ono_parser::words_arguments` is quadratic on a long dotted word inside a bracket. Measured, on
a quiet machine:

```text
n        2 000      4 000      8 000     16 000
time     70 ms     423 ms     1.46 s     32.5 s        for "[f" + ".5" * n
```

`parse` on the same input is linear, and `words_arguments` on the same input *without* the
leading `[` is linear, so it is the list-element loop re-entering word-mode lexing over a long
run that never shortens. It is a real denial-of-service shape — `words_arguments` is what reads
a command line — and it is not fixed here because the fix is a change to how the parser decides
whether a list element is a stage, which is a parser design question and not a drive-by.

It is not in the corpus. A seed that reproduces it would make the gate's own run red at a size
the gate cannot afford, and suppressing a finding to keep a check green is the one thing this
harness must not learn to do. The reproducer is the line above, and it belongs in `docs/STATE.md` as
its own task.

## Consequences

- `docs/ACCEPTANCE.md` §4.4's fuzzing line is now backed by fuzz targets rather than by suites
  that stand in for them, and the substitution is no longer undocumented.
- The gate grows a step. It is bounded and deterministic; if it becomes slow it is tuned by
  `--iterations`, never by deleting a target.
- The existing property and robustness suites stay exactly as they are. They assert *behaviour*
  — that a bomb is refused with a particular error — which a fuzz target cannot, because a fuzz
  target does not know what the right answer is. The two cover different things.
- `ono-testkit` becomes a normal dependency of a `publish = false` crate, for its `Rng`. No
  second PRNG enters the tree.
- A true hang is not detected. The harness measures how long an input took after it returns; an
  input that never returns hangs the runner, and the operator sees a gate step that does not
  finish. Detecting it properly needs a watchdog thread per execution, which costs more per
  input than the executions themselves. Recorded rather than pretended.

## Alternatives considered

- **`cargo-fuzz` + `libfuzzer-sys`, nightly-only.** The better fuzzer, and it cannot run in this
  gate on this toolchain. The targets are shaped so this can be added beside what is here — as a
  CI job with its own toolchain — without touching a target body.
- **An ADR recording the substitution and no targets** — the other option `docs/STATE.md` C-2
  offered. It would have made the deviation honest and left every decoder unfuzzed, against a
  normative MUST and the project's own standing rule.
- **`arbitrary`-driven structured generation.** Generates well-formed inputs, which is what the
  existing property suites already do. The interesting inputs here are the malformed ones.
- **Fuzzing procfs through `ProcessProvider::rooted` and a temp tree.** No API change, and two
  or three orders of magnitude slower per input — a fuzzer that executes a hundred inputs a
  second is a fuzzer that finds nothing.
