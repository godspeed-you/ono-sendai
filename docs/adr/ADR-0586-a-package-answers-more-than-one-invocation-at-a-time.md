# ADR-0586: A package answers more than one invocation at a time

- Status: accepted
- Date: 2026-09-06
- Spec refs: §31.11, §31.12, §31.14, §31.15, §31.34, §31.63, §31.73, §31.74;
  `docs/contracts/kuang/protocol.v1.yaml`, `docs/contracts/kuang/manifest.v1.yaml`,
  `docs/contracts/kuang/errors.v1.yaml`; ADR-0022, ADR-0040, ADR-0572, ADR-0584
- Decided by: agent (autonomous)

## Context

A KUANG/11 package could answer exactly one question at a time, and the way it failed to answer
a second one was the worst of the available failures.

`Plugin::run_io` read a frame, matched it, and — for `command.invoke` or `provider.query` — ran
the handler to completion on that same stack. While the handler ran, the only reader of the pipe
was `Io::pump`, reached from `Ctx::emit` waiting for credit and from `Io::call` waiting for a host
call's answer. `pump` knew four inbound requests: `stream.demand`, `stream.cancel`,
`health.probe`, `lifecycle.shutdown`, plus the view notifications ADR-0572 added. Everything else
fell to its `_` arm, which replied `Json::Null`.

A second `command.invoke` is everything else. The host received `{"result": null}` for it, tried
to read an `InvokeResult` out of `null`, and quarantined the instance with
`runtime.protocol_violation`: *invalid type: null, expected struct InvokeResult*. A package that
was asked two questions did not answer the second one badly — it was declared broken and unloaded.

Two pieces of real work found this rather than a reading of the code.

**A Kubernetes provider cannot prove context isolation as specified.** Its Gate J requires two
kubeconfig contexts to be queried *concurrently* and shown not to cross over. Two queries against
one loaded package is precisely the shape the protocol could not carry, so the strongest statement
that gate could make was "sequentially, in one session, the answers do not mix" — which is a
weaker claim about a weaker property, and the specification asks for the stronger one.

**Latency is the whole cost of a remote provider.** A package built around an external system
spends its time waiting for round trips. Serialising unrelated waits is an odd default for a
provider model whose §31.15 vocabulary is entirely about *bounded* resources rather than about
one.

The host was never the constraint. `ono-kuang-supervisor` already keys its pending calls by
`seq`, its streams by output handle and its invocations by invocation handle; `invocation_label`
already reads "one running invocation labels the trail precisely; anything else is
session-scoped". The supervisor was written for several and the SDK could serve one.

## Decision

**A package may have more than one invocation open at once, one worker each, under a ceiling the
operator agreed to. The default is one, and reaching the ceiling is a refusal rather than a
protocol violation.**

### 1. The reading loop never runs package code

`run_io` now reads frames and routes them, and nothing else. `command.invoke` and
`provider.query` start an invocation on a worker of its own inside a `std::thread::scope`;
the loop returns to reading immediately. Everything else it answers itself: init, shutdown, the
health probe, demand, cancellation, view notifications, and the protocol violation an unknown
method still earns.

This is the property the whole design rests on. `stream.demand` for one invocation reaches it
while another invocation is mid-emit, because the thread that delivers demand is not the thread
that is blocked waiting for it. Under the old loop, demand for stream A could only be read by
whatever handler happened to be pumping, which worked only because there was never more than one.

**Alternatives are recorded below**; the short reason for threads is that the alternative that
kept one thread requires handlers to be suspendable at their blocking points, and
`Fn(&mut Ctx) -> Outcome` handlers block inside `emit` and `host_call` with a live stack. Making
them re-entrant means making them `async`, which changes every handler ever written and asks a
package author to reason about a state machine where they had written a `for` loop.

### 2. The ceiling is declared in code and capped by the contract

Two different people know two different things, so both say one:

- **The author** states, in code, that the handlers are safe to run beside each other:
  `Plugin::concurrent_invocations(n)`. The default is **1**. A package that says nothing behaves
  as it did, and no existing package needs an edit.
- **The operator** states, through the manifest and host policy, how much of the machine one
  instance may have: `runtime.max_concurrent_invocations` in `manifest.v1.yaml`, capped by
  `HostLimits`, landing in `EffectiveLimits::max_concurrent_invocations` in the negotiated
  contract of §31.63. A null declaration accepts the host default (4), exactly as its sibling
  `max_concurrent_calls` already reads its null.

The effective ceiling is the smaller of the two, floored at one. Both numbers are visible: the
author's is in the source, the operator's is in the contract `lifecycle.init` delivers and in
`inspect plugin`.

Splitting it this way is the point rather than an accident. Thread-safety is a property of the
code and no manifest can assert it; a resource budget is the operator's and no package may
declare its way past it (§31.15). A single knob would have had to be one or the other, and
whichever it was would have been silently wrong for the other reader.

### 3. Beyond the ceiling is `runtime.concurrency_limit`, not a queue

`Ono-Sendai-K11207`, new in `errors.v1.yaml`. The plugin answers the invocation it cannot take
with `{status: failed, error: runtime.concurrency_limit}`; the instance stays `Loaded`, nothing of
the refused invocation ran, and the operator sees a bound they can act on.

Queueing was rejected. A queue turns a ceiling into a latency, and a latency into a mystery: the
package looks like it accepted the work and the operator has nothing to look at. §31.15's own
sentence about in-flight host calls — "a further call waits; it does not queue without bound" —
is about a bounded internal wait, not about an invocation the shell is waiting on.

The refusal is also, for every package that exists today, strictly better than what it replaces:
the second concurrent invocation used to quarantine the instance and now it is answered.

§31.79 calls its 27 codes "proposed stable categories" and has no name for a package that is
merely busy. The new code extends that list the way v0.4.1 §16.3's three `plugin.*` confinement
codes already did, and `error::tests::should_expose_all_27_codes_of_spec_31_79_when_enumerated`
counts the additions apart from the 27 so the specified list stays checkable as the closed thing
it is.

### 4. Responses are routed by `seq`, and the transport applies what is its

The plugin's outgoing calls share one `AtomicU64` and a `HashMap<seq, PendingCall>`; each waiting
worker holds the receiving end of a channel. The reading loop looks the `seq` up and hands the
answer to the worker that asked. Answers may arrive in any order, which they now do.

Two response kinds carry something that belongs to the transport rather than to the handler, and
the reading loop applies it **before** forwarding the answer, in frame order:

- **`streams.emit`** answers with the stream's new credit, which is absolute. Letting the waiting
  worker apply it would order it by thread wakeup instead of by frame, and a `stream.demand` that
  arrived after the reply could be overwritten by a stale absolute number. Applied on the reading
  loop, credit accounting is exactly the order the wire had.
- **`views.open`** answers with a view handle, and that is how the loop learns which invocation
  owns the view. Registering ownership when the answer is *routed* rather than when the handler
  gets it closes the race against the `view.mount` the host sends straight afterwards.

### 5. Everything else an invocation owns is keyed by its output handle

One `Invocation` record per open invocation: its credit window, its cancelled flag, its queue of
view events, and one condvar. `stream.demand` and `stream.cancel` name a handle and find exactly
one of them; `Ctx::cancelled` reads its own; `Ctx::next_view_event` drains its own. Two streams
cannot starve each other because neither waits on the other's condvar and neither is served by a
thread the other can block. The host's queue depth is untouched: credit is granted per stream, as
it always was, and an invocation that has none blocks itself and nothing else.

### 6. A handler that panics fails its own invocation

The worker runs the handler inside `catch_unwind`, and a panic becomes
`{status: failed, error: runtime.trap}` for that invocation. Its siblings keep running, and the
instance keeps serving. Every mutex the transport owns is taken through a `lock` helper that
recovers from poisoning, because a poisoned lock would let the one failure take the others with
it — which is the thing §31.34 exists to forbid.

### 7. Where there are no threads, the handler pumps its own frames

This is the constraint that shaped the implementation, and it was found by the gate rather than
by design: the same example package is also built for `wasm32-wasip1` for the component tier, and
that target has one thread and no way to make another. `Scope::spawn` aborts there.

So the SDK has one model with two ways of waiting. `WORKERS` is `cfg!(not(target_family =
"wasm"))`. Where it holds, an invocation runs on a worker and a waiting handler sleeps on its
condvar. Where it does not, the invocation runs on the reading loop's own stack and a waiting
handler reads and serves the next frame itself — which is what the old `pump` did, and what the
component tier needs in order to receive credit at all. The ceiling on such a target is one, and a
second invocation arriving while a handler pumps is refused with `runtime.concurrency_limit`
rather than left unanswered. The old bug is therefore fixed on the component tier too, without
threads.

The routing, the registry, the credit accounting and the refusal are one implementation across
both; only the two lines that block differ.

## Consequences

**What a package author must do differently: nothing**, unless they want concurrency.

- `Plugin::new(...).command(...).provider(...).run()` is unchanged, and a package built on it
  answers one invocation at a time as before.
- The one compatibility cost is a tightened bound: a handler is now
  `Fn(&mut Ctx) -> Outcome + Send + Sync + 'static`, because a worker runs it. A closure that
  captures nothing — which is what the SDK's example package and every documented handler is —
  satisfies it without saying so. A handler capturing an `Rc` or a `RefCell` no longer compiles,
  and it fails at compile time with the bound named, rather than at run time in a way an author
  would have to reproduce.
- `run_io` takes `impl Read + Send` and `impl Write + Send`; `Plugin::run` passes
  `std::io::stdin()` and `std::io::stdout()` rather than their locks, because a `StdinLock` is
  not `Send` and a handle is.

**Gate J's "concurrently" is now satisfiable.** Two `provider.query` invocations against one
loaded package run at the same time, each with its own options, its own credit window, its own
cancellation and its own answers.
`should_answer_two_concurrent_queries_of_one_target_with_their_own_options` is that shape in the
conformance suite, held open by a credit window of one so that "at the same time" is a fact rather
than a hope about scheduling.

**What is bounded and what is not.** The ceiling bounds invocations, and therefore workers, per
instance. It does not bound the *host calls* one invocation makes — that is
`max_concurrent_calls`, declared and still unenforced, and it stays out of this increment.
Memory, CPU and state quotas are per instance and are unchanged: four invocations share one
instance's ceilings, which is what §31.15's "quotas are per plugin instance, not per package"
already says.

**What this increment does not fix, and is now reachable.** The supervisor closes *every* open
view when *any* invocation ends (`close_all_views` in `handle_envelope`'s `Pending::Invocation`
arm). With one invocation that was correct by construction; with two it is a crossover, and it is
written into `docs/STATE.md` under *Found, not yet filed* with its reproduction rather than fixed
here, because fixing it is a host-side change with a test of its own (AGENTS.md §4).

Which tests encode it — `crates/ono-kuang-sdk/tests/conformance.rs`, over the real example plugin
under the deterministic test host of §31.73:

- `should_complete_two_invocations_that_were_open_at_the_same_time` — two commands, each blocked
  in `emit` with no credit when the other starts, both completing with their own values. This is
  the case that used to quarantine.
- `should_answer_two_concurrent_queries_of_one_target_with_their_own_options` — two queries of one
  contributed target, one asking for one record and one for three, neither answering the other's
  question.
- `should_deliver_a_host_calls_answer_to_the_handler_that_made_it` — two invocations with a
  `schemas.get` each, in flight together, each receiving the schema it asked for.
- `should_keep_the_other_invocation_running_when_one_is_cancelled` — one of two endless commands
  is cancelled, ends `Cancelled`, and the other keeps delivering.
- `should_refuse_an_invocation_beyond_the_negotiated_concurrency_ceiling` — a manifest declaring
  two, a third invocation answered `runtime.concurrency_limit`, and the instance still `Loaded`.
- `should_keep_serving_a_package_that_answers_one_invocation_at_a_time` — the `--serial` fixture,
  which never opted in: a concurrent second invocation is refused, the first completes, and a
  later sequential one works.

The example package gained `concurrent_invocations(4)`, a `--serial` mode that takes the default
instead, and one command, `relay`: it emits a marker, then makes a host call, then emits what the
host answered. The marker is where it blocks, on credit, which is how a test holds two
invocations at one point without a clock.

## Alternatives considered

**Interleave on the one existing thread.** Handlers would have to be re-entrant across their own
host calls, which means suspendable, which means `async` handlers. Rejected: it rewrites every
handler signature, pulls an async runtime into an SDK that has none, and asks a package author to
hold a state machine in their head where a `for` loop with an `emit` in it is the whole program.
The threadless component tier already shows what interleaving costs: it is the one place the SDK
still does it, its ceiling is one, and that is exactly the limit this ADR removes elsewhere.

**A bounded pool of worker threads, invocations queued for a free one.** Rejected on the
visibility argument of §3: with a pool, the N+1st invocation is accepted and then waits, and
nothing in the answer says it waited. A thread per invocation under the same ceiling costs one
spawn per invocation — invocations are coarse, a spawn is microseconds, and a provider's round
trip is milliseconds — and buys an answer the operator can read.

**Let the host enforce the ceiling and refuse before sending.** Tempting: the operator would get
the error without a round trip. Rejected because it puts the check in the place that does not own
the resource. The SDK is what spawns the worker, and a ceiling only the host enforced would be no
ceiling at all against a buggy or hostile host. Enforcing it in the plugin also makes it
observable in the conformance suite, which drives the real supervisor and could otherwise never
send the invocation that proves the limit.

**Reuse `runtime.backpressure_failure` for the refusal.** Rejected: that code means a stream chose
to fail rather than lose data, its `kind` is `stream`, and its help talks about overflow policy.
A busy package is not a failing stream, and a code whose help does not describe the situation is a
code that sends the reader somewhere else.

**Make `max_concurrent_invocations` the only declaration and derive the code's willingness from
it.** Rejected: it lets an operator raise a number and thereby assert something about somebody
else's code. The manifest would then be able to make a single-threaded package concurrent, which
is the one thing a manifest must not be able to do.

**Make `Plugin::concurrent_invocations` the only declaration and drop the manifest field.**
Rejected the other way: unbounded concurrency inside a package is a resource the operator did not
agree to, and §31.15 requires the effective value to be the smaller of the package's declaration
and host policy for every quota it lists. This one is now one of them.
