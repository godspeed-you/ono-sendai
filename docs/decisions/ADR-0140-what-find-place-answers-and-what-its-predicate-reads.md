# ADR-0140: What `find place` answers, and what its predicate reads

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §3.1, §3.2, §3.3, §6.8, §9.3, §27.2, §27.4, §28, §29.4, §35.3, §42.3, §43.3;
  v0.2 §28 (object schemas), §33.5 (stream serialization); ADR-0124, ADR-0138, ADR-0139
- Decided by: agent (autonomous)

## Context

§6.8 spells the spatial search four ways — `find <query>`, `find <type> <query>`,
`find --where <expression>`, `find --near <place-selector> <query>` — and says two things about
its answer: it "MUST search the spatial index and provider registries rather than blindly grep
rendered text", and "Results MUST include enough path/scope information to disambiguate identical
names". §29.4 adds that it "returns normal structured streams and can participate in object
pipelines".

That leaves three questions the spec does not answer.

**1. What is a result?** A place, or the object behind it? §3.3 makes a `Place` "the current
spatial interpretation of a `SpatialObject` or a canonical aggregate space" — a distinct thing
from the `ono.process/1` record it was projected from.

**2. What does `--where` read?** Every predicate in the RED suites and in §6.8's own examples
names a field of the *object*: `state == "running"`, `pid > 1`, `local.port == 8080`,
`cpu > 50`, `ppid == N`, `filesystem == "tmpfs"`. None names a field of a place.

**3. What about the places a matching record merely mentions?** Absorbing a batch also places the
objects those records *name* but no provider serves — the pid namespace a process reports, the
cgroup it belongs to, the far end of a connection (§42.3, ADR-0135). Those are real places, and
nothing tested them against the predicate.

## Decision

### 1. A result is a place, of the schema `ono.spatial-place/1`

`find place` streams `ono.spatial-place/1` records: `spatial_id`, `name`, `display_name`,
`object_type`, `spatial_type`, `place_path`, `scope`, `parent`, `freshness`, `observed_at`,
`identity_tier`, `capabilities`, `pinned`, `provenance`. The same schema is the `place` of a
`PlaceView`, so `look --json` and `find place` describe a place the same way.

Three of those fields answer §6.8 and §27.4 directly:

- `place_path` is §27.2's third column — `local/compute/processes` — from the host down, and it
  is what tells two objects with the same name apart;
- `scope` is the §3.2 boundary, `host:web01`;
- `freshness` and `provenance` say how current the answer is and where it came from, which §27.4
  requires of anything that may come from a cached index.

`object_type` is the **v0.2 schema of the object itself** — `ono.process/1` — read from the
object's canonical reference rather than from whatever record named it, so a pid namespace
composed from a process's `/proc/<pid>/ns/pid` reports `ono.namespace/1`, which is what it is.

`name` and `display_name` carry the same text on purpose: §3.1 requires `display_name`, and every
v0.2 object names itself `name`, so `find place x | select name` reads like
`get service | select name` rather than like a parallel world (§28, §29.4).

### 2. `--where` is a predicate over the objects, not over the places

The predicate is evaluated against each object **as its provider described it**, before the object
becomes a place. `find place --where state == "running"` means "the places of the objects whose
state is running" — which is how §6.8's own `find process --where cpu > 50` reads, since `cpu` is
a field of a process and of no place.

This also makes the search cheap in the right way: the fields a predicate reads decide which
provider targets are asked at all (ADR-0139), so `--where local.port == 8080` never enumerates a
process.

### 3. A predicate answers only about the objects it was evaluated against

When `--where` is given, the result is restricted to the places the matching records *are*.
The places those records merely mention are not an answer to a predicate nobody applied to them.
Without a predicate the restriction does not apply, and a composed place is findable by name and
by type like any other (§42.3).

### 4. The type is `--type`, and it is matched case-insensitively

ADR-0124 makes the spatial type an option rather than a second target word. The registry spells
the types `Process`, `Listener`, `BlockDevice`; a user types `--type process`. Case is not
information here.

### 5. The search is bounded by default

§34 budgets a search at 100 ms. `find place` answers a ranked, bounded stream — 100 places unless
`--limit` says otherwise — and `--all` removes the bound. The bound is on the work, not on what a
pipeline may see: the stream composes with `take`, `where` and `count` unchanged (§29.4).

## Consequences

- `find place --where state == "running" | take 5 | to json` answers rows carrying `spatial_id`,
  `scope` and `provenance`; `find place sleep | count` counts the same stream `to json`
  serialises. Both are outcome tests in `spatial_navigation_missing.rs`.
- A place is not the object, so a place stream has no `pid`, `cpu` or `target` field. Two
  acceptance lines written before this decision selected one — `090`'s
  `select pid` and `092`'s `select target` — and now select `name`, which is the field the place
  carries and the one those scenarios are actually about ("a real object from a property, with no
  name typed", "a mount this user can enter, discovered by type"). Where a caller wants the
  object's own fields, the object is one `enter`/`inspect` away and the place carries the identity
  to reach it.
- The `state` a §6.2 `near` view shows in its STATE column is the object's, not the place's, and
  therefore comes from the object rather than from this schema (§45.4's renderer, S4).
- `ono.spatial-place/1` is registered in `docs/spec/schemas/`, embedded in `ono-value`, and named
  by `docs/spec/targets.yaml`'s `place` target and `docs/spec/commands/spatial.yaml`.

## Alternatives considered

- **Answer the provider records themselves.** Rejected: §6.8 requires path and scope information
  on every result, and §3.3 makes the place a different thing from the object. A stream of
  `ono.process/1` records would answer "which processes", not "which places", and `find place`
  could not answer for a canonical space at all.
- **Flatten the object's fields onto the place record.** Rejected: the field sets collide
  (`name`, `state`, `scope`, `parent` all mean different things on the two), and a record whose
  shape depends on what it happens to describe cannot have a schema (v0.2 §28).
- **Evaluate `--where` against the place record.** Rejected: it would make every predicate in the
  specification and in the suites — `cpu > 50`, `pid > 1`, `local.port == 8080` — refer to
  nothing, and would leave no way to search by what an object *is*.
- **Carry the source record in a nested `object` field.** Rejected as speculative: nothing in the
  RED suites reads it, it doubles the size of every result, and `inspect` already answers the
  question it would answer.
