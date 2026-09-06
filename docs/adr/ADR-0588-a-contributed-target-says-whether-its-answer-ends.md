# ADR-0588: A contributed target says whether its answer ends

- Status: accepted
- Date: 2026-09-06
- Spec refs: §12.4, §12.6 and §33.2 of `docs/architecture/external-system-provider.md`; §31.14,
  §31.23, §31.64, §31.68, §31.80, §36.5; `docs/specs/ono_sendai_shell_spec_v0.2.md` §18.2, §18.3,
  §18.5; `docs/contracts/kuang/contributions.v1.yaml`;
  ADR-0024, ADR-0059, ADR-0481, ADR-0582, ADR-0583, ADR-0584, ADR-0586, ADR-0587
- Decided by: agent (autonomous)

## Context

`PluginProvider::snapshot` carried a comment that was also a bug report:

> Spec §12.4 and §12.6 of the external-system-provider architecture let a provider stream an
> enumeration and declare that it is expensive; `docs/contracts/kuang/contributions.v1.yaml` gives
> a target contribution neither field, so nothing a package declares says whether its answer ends.

The consequence was not a missing optimisation. It was that **a package could not contribute a
live answer at all.** Everything a contributed target produced was read to its end — `plugins::query`
does `invocation.collect().await`, and `snapshot` spawned a `Boundedness::Bounded` stream — so a
target whose answer has no end never returned to the prompt. The SDK's own example package has had
such a target since ADR-0586 (`echo-tick`, "items emitted until the query is cancelled"), and the
only way to ask it anything was through `Query::limit`.

The external-system provider building against this — the Kubernetes package — hit it as a
specification requirement rather than as an inconvenience. Its §41.1 is a `MUST`: *"The provider
MUST use the inherited Ono live-view contract rather than creating a Kubernetes-specific TUI
subsystem."* The inherited contract is shell specification §18.3 — rows updating in place at a
terminal, fed by an unbounded `ValueStream`. A watch that the host collects reaches none of it.

The host cannot discover this by reading. A package that has not sent a record yet and a package
that will never stop sending them look identical from the outside, and the difference has to be
known *before* the first record, because it decides whether the answer is collected at all.

A second, smaller thing was wrong on the same path and is fixed here because it is the same call:
`snapshot` passed `serde_json::Map::new()` — an empty argument map — so every contributed target
answered a `resolve`, an `enter` or a re-read of a place as though nothing had been asked of it.
That is not a visible failure. It is an unfiltered answer, which is worse.

## Decision

### 1. `TargetContribution.answer` — `bounded` (default) or `unbounded`

A target declares which kind of answer it has, in the document the host reads before any of the
package's code runs (§31.68) and across the handshake. `bounded` is the default and the wire skips
it, so every package that exists keeps working unchanged and every package that says nothing gets
the behaviour it had.

The vocabulary is deliberately two words rather than a boolean named after one of them. `unbounded:
false` reads as a denial of something; `answer: bounded` is a statement about the answer, which is
what it is.

### 2. The host branches on the declaration, in one place

`stream_of` in `plugin_provider.rs` is now the single door for one invocation of a contributed
target, used by `PluginProvider::snapshot` and by the shell's own `get <target>` path. The two used
to differ in ways nobody had decided — one passed the query's arguments and the other passed an
empty map; one could be collected and the other could not — and a package that answered differently
depending on which door the question came through would be a package with two behaviours and one
declaration.

- **`get <unbounded target>`** returns `Answered::Live(ValueStream)` with `Boundedness::Unbounded`,
  and the evaluator continues the pipeline with it (`run_piped`) instead of seeding it with a
  materialised `Vec`. From there everything is inherited: a terminal shows it in place (§18.3), a
  pipe must choose a representation, `take` bounds it, and Ctrl-C cancels it. No new rendering, no
  new verb and no plugin-shaped special case anywhere in the pipeline.
- **`snapshot` of an unbounded target with no `max` is refused**, naming the declaration.
  `Provider::resolve` collects what `snapshot` returns, so reading one there would hang the
  registry rather than answer it — and hanging is the one outcome that is never an answer. A query
  that bounds itself gets its prefix, which is what `find place --limit` already asked for.

### 3. A head-only stream is shown rather than dropped

`run_piped` seeds a run at stage 1. With a one-stage pipeline — `get k8s-change`, and nothing after
it — there was no stage to run, and `run_from` returned success having shown nothing. Two places
now treat a stream seed as something to *show* rather than as something a following stage consumes:
`run_from`'s empty-segment branch, and `run_native_segment`'s early return for a segment that binds
no contract. Both already made the exception for `Seed::Stream`, which is the same situation
arriving from a reader thread.

### 4. `snapshot` carries the query's options to the package

`Query::options()` become the invocation's arguments, in the tagged encoding the protocol already
uses. This is what lets a contributed provider answer *which* namespace, context or kind a place
belongs to when the registry re-reads it.

## Consequences

- A KUANG/11 package can contribute a live answer — a watch, a followed log, a subscription — and
  it reaches the shell's existing live view rather than a subsystem of its own. That is the
  capability the Kubernetes provider's §41.1 requires, and nothing about it is Kubernetes-shaped:
  the proof in this repository is the example package's `echo-tick`.
- `resolve` and `enter` over an unbounded contributed target now refuse instead of hanging. That is
  a behaviour change in the direction of an answer.
- `Provider::subscribe` is deliberately **not** implemented for a contributed target. `watch
  <target>` still requires a builtin `ono.<target>-event/1` schema, and inventing one per
  contributed target is a larger decision about the event vocabulary than this increment needs.
  What a package gets today is the same thing a `journalctl -f` stream gets (ADR-0059): a growing
  table of the records as they arrive. An unbounded target that wants fold-in-place rendering emits
  event-shaped records, which `ono_cli::live::apply` already folds.
- `TargetContribution` gained a field, so every construction site names it. The Kubernetes package
  pins a revision of these crates and takes the change when it moves the pin.

## Alternatives considered

**Let the host discover boundedness by reading.** Impossible, and not marginally: the two cases are
indistinguishable until the answer ends, and the decision has to be made before the first record.

**A timeout — collect until the package goes quiet.** Rejected. It turns a quiet watch into a
finished one and a busy watch into a hang, and neither is a fact about the answer. It is the shape
§18.2 forbids: polling that is not explicit in metadata.

**Make every contributed target unbounded and let the pipeline sort it out.** Rejected. `resolve`
collects, so this would make `enter` over any contributed place hang, and the overwhelming majority
of contributed targets are enumerations that end.

**Put the declaration on the *query* rather than the target — `get <target> --follow`.** Rejected.
Whether an answer ends is a property of the target, not of how it was asked for, and a flag would
have to be understood by the host *and* honoured by the package, with nothing holding the two
together. A package that ignored it would hang the shell exactly as before.

**Implement `Provider::subscribe` for contributed targets in the same increment.** Deferred, with
its reason above: it needs an event schema per contributed target and a decision about `is_watchable`
that is not this change's to take. Nothing here forecloses it.
