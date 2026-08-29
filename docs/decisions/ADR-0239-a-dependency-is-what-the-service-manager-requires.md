# ADR-0239: A dependency is what the service manager requires

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.4 §13 (a service place's groups include dependencies and dependents), §42.3 (an
  edge reaches a known object, never a dangling id), §2.16 (providers own facts);
  v0.2 §22.2–§22.4, §28.3, §35.3; `docs/spec/spatial/relations.yaml` (`service.depends_on`);
  ADR-0135
- Decided by: agent (autonomous)

## Context

`docs/spec/spatial/relations.yaml` declares `service.depends_on` with the groups `dependencies`
and `dependents`, and adds the rule that "a provider must not claim a dependency it cannot
justify". No provider claimed it, so the exit was permanently empty and v0.4 §13's service place
was missing two of its groups (ADR-0135).

The reason on record was cost: `ListUnits` carries no dependency information, and
`Requires`/`Wants`/`After` need a `Get` per unit. That reason had stopped being true.
`SystemdProvider::snapshot` already calls `unit_properties` for **every** unit it emits — the
by-name path and the listing both go through it — and `unit_properties` is
`GetAll(org.freedesktop.systemd1.Unit)`, whose reply already contains `Requires`, `Requisite`,
`BindsTo`, `Wants`, `After` and the rest. The facts were being fetched and thrown away.

## Decision

**`ono.service/1` carries `dependencies`: the units the service manager says this one requires.**
For systemd that is `Requires`, `Requisite`, `BindsTo` and `Wants`, merged, sorted and
de-duplicated.

**Ordering is not a dependency.** `After` and `Before` say *when*, not *whether*, and a unit
ordered after another need not require it at all. Calling an ordering a dependency is exactly the
claim `relations.yaml` says a provider must be able to justify and cannot.

`null` means the provider has no notion of a dependency; an empty list means it has one and there
are none. systemd always has one, so its records never say `null` (§35.3).

**`ono-graph` gains `ServiceDependencies`,** which draws `depends-on` from the field to the unit
of that name among the units the same trace already enumerated. It composes; it does not observe
a second time (§2.16). A dependency naming a unit the manager does not hold draws no edge —
§42.3 wants every edge to reach a known object rather than a dangling id — and reports no
failure, because a unit file may name a unit that was never installed.

**The spatial bridge maps it**: `("linux.service-dependencies", "depends-on")` becomes
`service.depends_on`, so a service place's `dependencies` exit fills and its `dependents` exit is
the inverse `relations.yaml` already declares.

## Consequences

- `enter service systemd-journald.service; look` shows `dependencies  available  4` where it
  showed nothing, and `trace service <unit>` draws the units it requires beside the processes it
  owns.
- `get service | where "network-online.target" in dependencies` is a question that can be asked,
  because the fact is a field rather than an edge nobody can filter on.
- Nothing became slower. The properties were already read for every unit; four arrays of the
  reply are now kept instead of discarded.
- `ono.service/1` gains a nullable `list<string>`. Additive, no version bump; the default view is
  unchanged.
- The `dependents` direction is the relation's declared inverse and needs no second provider:
  systemd's `RequiredBy`/`WantedBy` would say the same thing twice, and two sources for one
  fact can disagree.
- Encoded by `should_report_every_schema_field_when_a_unit_is_running`,
  `should_report_no_dependencies_as_an_empty_list_rather_than_as_null`,
  `should_link_a_service_to_the_units_it_requires`,
  `should_draw_no_dependency_edge_to_a_unit_the_manager_does_not_hold`,
  `should_relate_a_service_to_the_units_it_requires` and
  `should_list_the_dependencies_of_a_unit_as_a_field_of_its_record`.

## Alternatives considered

- **Include `After`/`Before`.** It doubles the edge count of every service place with orderings
  that are not requirements, and it makes `dependencies` mean two different things at once.
- **A separate `ono.service-detail/1` read only by `inspect service`.** There is no
  `inspect service` in the registry, and inventing one to carry a fact the listing already pays
  for would make the dependency a second-class fact for no saving.
- **Draw the edge from a per-unit D-Bus call inside `ono-graph`.** It puts a second reader of the
  service manager in the crate that is meant to compose what providers say, and it would cost one
  round trip per unit on top of the one already made.
