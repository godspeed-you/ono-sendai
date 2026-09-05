# ADR-0583: A loaded package is a provider in the registry

- Status: accepted
- Date: 2026-09-05
- Spec refs: §31.14, §31.23, §31.64, §31.68, §31.72, §31.80; v0.4 §35; ADR-0582
- Decided by: agent (autonomous)

## Context

ADR-0582 made a contributed target typeable. `get echo-item` resolves, loads the package that
declares it, reaches `provider.query`, and comes back with records carrying the schema the target
declared and the provenance the host stamped. It said plainly where it stopped:

> this wires the *read* path only. A contributed target reaches `provider.query` and no further —
> `ProviderRegistry` still holds no entry for it, so `resolve`, `subscribe` and `act`, and with
> them the spatial model's `enter`, `near` and `follow`, do not yet see contributed targets.

That boundary is not a missing feature so much as a missing *identity*. Everything in the shell
that asks about a noun rather than running a command asks `ProviderRegistry`: `for_target` decides
whether a target is answerable at all, `provider_for` decides who answers, `resolve` turns a
selector into objects, `schemas` decides which fields a predicate may name, `provider_of` routes
an action back to the system a record came from. A package that answers `get` and nothing else is
reachable by exactly one path, and the Kubernetes provider specification (v0.4 §35) needs the
others: a pod must be an enterable place with relationships and identity, and none of that hangs
off a command.

There is a lifecycle mismatch underneath, and it is the interesting part of the problem.

- The *command* registry is built once per process from the manifests on disk, before anything
  runs (§31.68, ADR-0282). A contributed target is typeable because reading a YAML file costs
  nothing and starts nothing.
- The *provider* registry cannot be built that way. A `Provider` must be able to answer
  `snapshot` when asked, and a contributed target can only be answered by a running instance of
  its package. There is no instance until a load, and a load is not something a `Provider` can
  perform for itself: it negotiates capabilities, re-verifies integrity and signatures against
  what is on disk now, records grants on the host and may write to the audit trail — all of which
  needs `&mut Session`, which `Provider::snapshot(&self, …)` does not have and must not have.
- The provider registry is itself built lazily, on first use, and cached on the session, so it
  is entirely possible for a package to be loaded before the registry exists — and equally
  possible for the registry to be built before any package loads.

So "register at startup" and "register a provider that loads itself on first query" are both
unavailable, and the third option has to be stated rather than assumed.

## Decision

**Registration follows the load, and the session re-syncs on its way to the registry.**

- `crates/ono-cli/src/plugin_provider.rs` holds `PluginProvider`: one provider per contributed
  target, wrapping a `LoadedPlugin`. It is the same shape `ono_remote::RemoteProvider` has, for
  the same reason — one target per instance is what lets `Provider::resolve`, which is handed a
  selector and no target, know what it is resolving.
- `Session::providers()` mounts every loaded package that contributes a target before it hands
  the registry out. That is the earliest moment the mount can honestly happen, and it is a
  moment every reader passes through, so `help`, the evaluator, the spatial commands and the
  KUANG host services all see the same registry.
- The lifecycle answer is therefore: **a package the session has never loaded claims no target.**
  `get <target>` still loads it, because the command placeholder of §31.68 is still there and
  still routes to `provider.query`; once that load has happened, the provider is in the registry
  for the rest of the session. A user who wants `enter` before `get` writes `load plugin` first.
  This is honest about what a placeholder is worth: it is a *declaration*, and a declaration
  cannot answer a query.
- The provider's `id` is `plugin:<package.id>` — the token the host already stamps into every
  record the package emits (§31.80). A provider whose id disagreed with the provenance of its own
  records would be two answers to one question.
- `Provider::targets` returns borrowed `&str`, and a package's targets are known only at its
  handshake. The names are interned into a process-wide table and leaked once each, so a reload
  costs nothing further and the total is bounded by how many distinct targets the installed
  packages declare. `ono_remote` makes the same trade and says so in the same words.
- Cancellation is delivered, not inferred (§31.14). The producing task selects `biased` on
  `sink.cancel_token()`, and on cancellation — or on a sink that closed because the consumer went
  away — it calls `RunningInvocation::cancel()` before it ends. This matters concretely rather
  than theoretically: a package emits under credit, so a consumer that stops taking values leaves
  the package blocked inside its handler waiting for demand. Dropping the invocation would leave
  it blocked there for good, and a package is one process with one dispatch loop, so the next
  query would never be answered.
- A reload replaces the instance behind an id (§31.72). `ProviderRegistry` has no removal, so the
  mount step re-points the entries it already made rather than registering a second set; an entry
  whose target the new instance no longer contributes reports itself `provider.unavailable` with
  that reason instead of answering out of a stopped instance.
- `LoadedPlugin::schemas()` is new: the supervisor already builds and validates the package's
  contributed schemas at the handshake, and a provider has to be able to say what it produces
  before it has produced anything. It is an accessor beside `commands()` and `targets()`, not
  registry wiring — the supervisor keeps saying, correctly, that wiring is the shell's job.

## Consequences

Easy: everything that goes through the registry sees a contributed target. `help get <target>`
names the package as the provider (spec §15.2). `ProviderRegistry::resolve` turns a selector into
`ObjectRef`s carrying the contributed schema, which is what the action and spatial paths consume.
`registry.schemas()` includes what the package declared, so a predicate over a contributed field
can be checked rather than guessed. Registration order is kept, so a package naming a target the
shell already answers extends it rather than displacing it (§31.23).

Hard, and deliberately left hard:

- **The spatial commands still do not reach a contributed target**, and this ADR does not claim
  they do. `enter`, `near` and `find` plan over `SpatialType::ALL`, a closed vocabulary in
  `ono-spatial-query`; a contributed noun is not in it, so the plan never asks for the target
  even though the registry would now answer. Opening that vocabulary to packages is its own
  increment, and it is the one v0.4 §35 needs next. What this change buys is the prerequisite:
  there is now something for the plan to ask.
- **`subscribe` and `act` are refused, honestly.** The KUANG/11 protocol has `provider.query` and
  nothing else; there is no `provider.subscribe` and no `provider.act`. The trait's defaults
  report `provider.unsupported`, which is the true answer, and `watch` needs an
  `ono.<target>-event/1` contract this build has for core targets only.
- **`get` still does not go through the registry.** ADR-0582's route collects the whole answer
  into a `Vec<Value>` before seeding the pipeline, and moving it onto the streaming provider path
  is a change to what that ADR delivered — a separate increment, and a worthwhile one, because
  the collecting route cannot answer an endless target at all.
- **Two paths now reach one package.** `plugins::query` and `PluginProvider::snapshot` both call
  `LoadedPlugin::query`. That duplication is the price of not touching ADR-0582 in this commit,
  and the previous point is how it is paid off.
- `capabilities()` is empty on purpose. What a package may do is decided by the KUANG/11 policy
  broker at every host call (§31.19), against the manifest's capability vocabulary; the provider
  capability list says what a *built-in* provider needs of the operating system, and restating
  the grants there would be a second answer to a settled question.

Which tests encode it — `crates/ono-cli/tests/plugin_provider.rs`:

- `should_register_a_loaded_packages_target_as_a_provider` — the real binary: after `load plugin`,
  `help get echo-item` names `plugin:dev.example.echo` as the provider of the target.
- `should_not_claim_a_contributed_target_before_the_package_is_loaded` — the same page before any
  load reports `none registered`. This is the lifecycle answer, pinned rather than assumed.
- `should_answer_a_contributed_target_through_the_registry` — `ProviderRegistry::snapshot` yields
  the package's records, carrying `dev.example.echo.item/1` and provenance `plugin:dev.example.echo`.
- `should_declare_the_schema_the_contributed_target_answers_with` — the contributed schema is one
  of the registry's.
- `should_refuse_a_target_the_package_does_not_contribute` — `resolve.target_not_found`, never an
  empty success.
- `should_resolve_an_object_of_a_contributed_target` — `ProviderRegistry::resolve` answers with an
  `ObjectRef` of the contributed schema.
- `should_cancel_the_packages_stream_when_the_query_is_cancelled` — the package returns from
  `busy` to `ready` and answers the next query after a cancelled one, which it could not do if
  the invocation had only been dropped.

The example package gained a second contributed target, `echo-tick`, which emits until it is
cancelled — the provider-side counterpart of the `count-forever` command it already had. A finite
target proves nothing about cancelling, because it stops on its own.
`crates/ono-kuang-sdk/tests/conformance.rs` asserted the fixture contributed exactly one target;
it now looks `echo-item` up by name, which is what it was checking.

ADR-0582's six tests in `crates/ono-cli/tests/plugin_targets.rs` are unchanged in what they
assert. One helper moved: the fixture manifest both suites lay out is now
`support::echo_package_manifest`, because v0.4.1 §39.1 allows a test helper only one definition
and the two suites must agree byte for byte anyway — the bytes on disk and the `Manifest` a direct
load parses are the same package or the two suites are testing two different ones.

## Alternatives considered

**Register a provider that loads its package on first query.** Rejected on evidence rather than
on taste: loading negotiates capabilities, re-verifies signature and integrity against the files
as they are now, records grants and writes audit entries, and every one of those needs
`&mut Session`. `Provider::snapshot` takes `&self` and is called from inside a pipeline task.
Making it possible would mean handing a provider a channel back into the session and performing a
trust decision underneath a `get`, which is precisely the thing §31.68 puts in front of the user
rather than behind a stream.

**Build the provider registry from installed manifests, with entries that report themselves
unavailable until loaded.** Rejected: it makes `help` say a provider exists when nothing can
answer, and `for_target` — which the spatial planner and completion use as "is this askable" —
would answer yes for a package that may fail to load at all. An unavailable provider is a
statement about a system that is missing, not about a package nobody has started.

**Register at the moment of the load, inside `load_plugin_with`.** Rejected because the registry
is built lazily: a package loaded before the first pipeline would register into a registry that
does not exist yet, and the code would need the same re-sync anyway. Doing it once, on the way to
the registry, is the same work in one place — and it is where `publish_jobs`, `publish_links`,
`publish_host` and `publish_env` already do the equivalent for the session's own tables.

**One provider for the whole package, answering every target it contributes.** Rejected:
`Provider::resolve` is given a selector and no target, so a multi-target provider cannot know
which noun it is resolving. `RemoteProvider` split per target for exactly this reason, and
matching it means the registry treats a package, a remote machine and a built-in provider alike.

**Return `Vec::new()` from `schemas()` and leave the supervisor untouched.** Rejected: it is a
lie in the one place the registry looks to decide whether a field exists, and the supervisor
already has the validated schemas in hand. An accessor is not the registry wiring the supervisor
declines to own.
