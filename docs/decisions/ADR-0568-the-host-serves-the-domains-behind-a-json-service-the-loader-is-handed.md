# ADR-0568: The host serves the domains behind a JSON service the loader is handed

- Status: accepted
- Date: 2026-09-03
- Spec refs: §31.12, §31.13, §31.14, §31.15, §31.16, §31.20, §31.26, §31.37, §35.3; ADR-0015, ADR-0022, ADR-0040, ADR-0566, ADR-0567
- Decided by: agent (autonomous)

## Context

Issue #3 (c), after ADR-0567: `objects`, `relations`, `history`, `process` and `secrets` were
still absent from the supervisor. Every one of them is served by something the supervisor does
not have — the provider registry, the graph, the history file, process control, a secret store
— and all of it lives in the shell. The supervisor must not depend on those crates: it sits in
the capability layer and they are above it, and a supervisor that pulled the providers in could
not be the small, testable boundary the conformance suite runs.

## Decision

**1. One service, JSON in and out.** `LoadConfig.host` is an `Arc<dyn HostServices>` with one
method per host call — `object_get`, `object_query`, `object_resolve`, `object_snapshot`,
`object_subscribe`, `object_watch`, `relations_query`, `relations_contribute`, `history_query`,
`history_append`, `process_signal`, `secret_request`. Every method takes the wire's JSON and
answers with it, so the supervisor decodes nothing it does not need and the test host can hand
a fake host with two items and one edge. The shell implements the trait over its registry,
graph and history; `NoHost`, the default, answers `provider.unavailable` for every call, which
is the honest word for a domain this build does not serve (§35.3).

**2. The supervisor keeps the boundary; the service keeps the data.** Every domain call checks
the grant against the value the operation will use — the query's target, the object id, the
signal's name against the `signals` scope, the secret's name against the `secrets` scope —
audits it, and only then calls the service. What comes back goes on the wire as it came,
a stream where the contract says stream: a `LiveStream` is a channel of values the service
fills as its source produces them, and `streams.next` pulls from it with a deadline for the
first value and takes what is there for the rest, so a subscription that has produced nothing
answers an empty page rather than hanging the plugin.

**3. A secret handle names a secret; the material stays with the host.** `secrets.request`
asks the service whether the named secret exists for the package and, on yes, issues an opaque
handle the supervisor maps to the name; `secrets.release` drops it, and a handle the host never
issued is a protocol violation. Nothing in this increment hands the material anywhere: the
calls that would consume a handle — `network.request`'s `credential` — are later work, and the
shell of this build has no secret store, so its service answers `provider.unavailable` and
`get capability` still says `secret.use` honestly.

**4. `objects.resolve` takes a target.** The contract declares a selector alone, and a selector
alone is ambiguous across providers. The call accepts `target`, and derives it from an identity
selector's schema when the caller gives none; a field selector without a target is refused as
malformed rather than resolved against a guess.

## Consequences

- Five domains are served end to end under the test host — a query stream honouring its limit,
  a fetch by identity and a miss, edges and redacted history, a signal inside the granted list
  and one outside refused before it reaches the host, a secret handle inside the scope and a
  name outside it refused — and the shell's implementation over its own registry, graph and
  history is proven through the binary.
- `network`, `views` and `process.exec` remain: a brokered connection needs a byte stream in
  both directions and the operator's trust store for TLS, a view needs the view runtime the
  shell does not have yet, and `exec` needs the confined spawn of §16 turned inward. They are
  the next increments, and the issue stays open for them.

## Alternatives considered

- **Depending on `ono-provider-api` and `ono-graph` from the supervisor** and calling the
  registry directly. Rejected: the layering forbids it, and the conformance suite would need a
  machine behind every test.
- **Materialising every stream before answering with the handle.** Rejected for the live
  domains: a subscription never ends, and a watch that buffered the world before its first page
  would be the unbounded buffer §31.15 forbids.
