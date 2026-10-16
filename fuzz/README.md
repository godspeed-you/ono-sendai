# `fuzz/` — the fuzz targets of spec §35.6

> Fuzz parser, serializers, remote protocol, plugin protocol and procfs/netlink decoders. A shell
> consumes adversarial filenames and external output by nature. — spec §35.6

Nineteen targets and a small mutation engine around them: the parser, the serializers, the remote
protocol and handshake, the plugin protocol, the system and adapter decoders, and the formats v0.5
and v0.6 added — the temporal store, events, time selectors and causal candidates, change records,
plan grammar, persistence domains, storage-tool output, snapshot paths, provider messages, the
impact graph and recovery metadata. `cargo run -p ono-fuzz -- list` prints them. The decision,
its limits and what would lift them: **ADR-0313**; the coverage-guided tier beside it:
**ADR-0521**.

```sh
cargo run -p ono-fuzz -- list                     # the targets and how many seeds each has
cargo run -p ono-fuzz -- run                      # every target, the default budget
cargo run -p ono-fuzz -- run --target parser --iterations 100000 --seed 7
cargo run -p ono-fuzz -- repro parser fuzz/artifacts/parser/<sha256>.bin
cargo run -p ono-fuzz -- run --target parser --journal /tmp/in-flight.bin
```

`--journal` writes each input to a file *before* executing it. It is how an input that aborts
the process rather than unwinding is caught — a stack overflow cannot be caught by
`catch_unwind`, so the only way to see what caused one is to have written it down first. After
an abort, that file holds the culprit. Both parser overflows in ADR-0313 were found this way.

The gate runs `run --iterations 400`, which takes seconds. Longer runs are what a developer does
by hand, or what a CI job with time to spare does; the run is fixed by its seed, so anything
either of them finds reproduces here.

## When a run finds something

The runner writes the exact input to `fuzz/artifacts/<target>/<sha256>.bin`, prints the `repro`
command for it, and exits non-zero. `repro` installs no panic hook and catches nothing, so what
you get is the backtrace.

Fix the decoder, **commit the artifact**, and leave it there. `fuzz/tests/corpus.rs` replays every
seed and every artifact on `cargo test --workspace`, with no mutation and no budget: the crash you
fixed once cannot come back quietly.

## The corpus

`fuzz/corpus/<target>/` holds the shapes each decoder actually meets — the netlink messages the
kernel sends, the frames a remote agent sends, the envelopes and manifests a plugin sends, the
documents the codecs read, the `/proc` lines the kernel writes, the command lines a user types.
They are the same shapes the property and robustness suites build in Rust, written to files so
the mutator has somewhere to start.

Add a seed by dropping a file in. Nothing indexes them; the directory is the index.

## The coverage-guided tier

`fuzz/coverage-guided/` is a second driver for the same nineteen target bodies
(`fuzz/src/targets.rs`): `cargo-fuzz` with libFuzzer, which needs a nightly toolchain and is
therefore a workspace of its own, outside the gate's (v0.4.1 §41.2, ADR-0521).
`.github/workflows/fuzz.yml` runs every target daily on `main`, five minutes each by default
(§41.3), and a finding reproduces on the pinned toolchain with `repro`:

```sh
cargo +nightly fuzz run --fuzz-dir fuzz/coverage-guided parser
```

`fuzz/tests/corpus.rs` holds the workflow's target list against the targets themselves, so a
target added in one place and forgotten in the other fails the gate.

## What this does not do

The engine of `ono-fuzz` has no coverage feedback (ADR-0313 §2): it cannot tell that an input
reached a new branch, so it cannot keep it and build on it, and a bounded gate run that finds
nothing has found nothing — it has not shown there is nothing to find. The coverage-guided tier
above is what explores; the gate's run is a regression suite. A true hang is not detected by
`ono-fuzz` either: the runner measures how long an input took after it returns.

The gate's own run passes a loose per-input ceiling, because it may run on a machine several
other things are compiling on, and a red gate that means "the machine was busy" teaches people to
ignore it. Slow inputs are for a campaign on a quiet machine, at the default two seconds. Crashes
are what the gate is for.

`ono-fuzz` writes nothing outside `fuzz/artifacts/` and a `--journal` you name, and reads nothing
outside `fuzz/corpus/` — except `temporal-store`, which writes each input to a file in a fresh
temporary directory, because the decoder it exercises opens a ledger database by path. It is not a
sandbox: a target calls the real decoder, and a decoder that shells out or opens a file would do
so here too. None of the nineteen shells out.
