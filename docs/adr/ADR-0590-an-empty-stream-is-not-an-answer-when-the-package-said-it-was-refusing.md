# ADR-0590: An empty stream is not an answer when the package said it was refusing

- Status: accepted
- Date: 2026-09-06
- Spec refs: §31.14, §31.23, §31.68, §31.79; §21.4 of `docs/architecture/external-system-provider.md`;
  ADR-0588
- Decided by: agent (autonomous)

## Context

ADR-0588 gave a contributed target a way to declare that its answer does not end, and a package
that declares `answer: unbounded` is routed through `plugin_provider::stream_of` instead of being
collected. That path read the stream and nothing else:

```rust
None => return,
```

`None` from `RunningInvocation::next` means the output stream ended. It does **not** mean the
invocation succeeded. When a handler returns `Outcome::Failed`, the supervisor answers on the
result channel and removes the output stream *without* sending a `StreamEvent::Failed` on it — so
a refusal raised before the first value looks, from the host's side, exactly like a package that
finished with nothing to say.

The result: **every refusal from an unbounded target arrived at the prompt as a clean, successful
empty answer.** A bounded target was unaffected, because `snapshot` collects the events and then
reads the invocation result.

This is the substitution the provider contract's §21.4 exists to prevent — uncertainty converted
into emptiness — committed by the host rather than by the provider, and therefore invisible to
every test a provider can write. The Kubernetes provider found it the only way it could be found:
`get k8s-log` on a container that had printed nothing answered `[]`, while
`should_refuse_to_answer_an_empty_log_with_an_empty_stream` was green at the plugin boundary. The
package had refused, in words, with the bounds of the read attached; nobody saw it.

And behind that first bug it was hiding a second, which is the part worth remembering: with the
refusal restored, the very same command reported `406 Not Acceptable` — the provider's log reads
had never once worked against a real API server. One silent host-side substitution had concealed a
whole broken feature through every layer of green below it.

## Decision

**`stream_of` reads the invocation result before it calls the stream done.**

On `None`, the invocation is finished and a `Failed` status with an error is turned into a
`sink.fail(...)`, exactly as an explicit `StreamEvent::Failed` already was. A successful end stays
an end.

The fix is on the host side rather than in the supervisor because the supervisor is not wrong: the
result channel *is* where an invocation's outcome lives, and a stream that ends is a stream that
ends. What was wrong was a consumer treating one of the two as the whole answer.

`echo-tick` in the example package gains a `refuse` argument that ends the invocation in a
refusal before emitting anything — the one state an unbounded answer has that a bounded one does
not, since the host has already opened a stream and what arrives on it is nothing. Acceptance case
`129-kuang-unbounded-target` asserts the refusal reaches the prompt and that `[]` does not.

## Consequences

Any package with an unbounded target can now refuse and be heard. Before this, the only way for
such a package to report a failure was to emit at least one value first — which is to say, the
protocol quietly required a provider to lie a little before it could tell the truth.

The lesson generalises past this bug: a host that consumes half of a two-channel result will
silently convert one half into the other, and no test written on the far side of that host can
see it. The acceptance case is where it had to be caught, and it was found by running a real
command against a real cluster rather than by any suite.
