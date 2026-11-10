# ADR-0987: A selector sweep tells the service manager the name it is looking for

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §27.1, §27.3; v0.4.1 §34.2, §36.1; ADR-0497, ADR-0576, ADR-0970, ADR-0980, ADR-0986; issue #187
- Decided by: agent (autonomous)

## Context

v0.4.1 §36.1: "A selector miss MUST not be substantially more expensive than a hit solely
because the system scans an unnecessarily complete global candidate set", with a Profile M
target of 250 ms p95. ADR-0497 made the sweep cheapest-first and ADR-0970 made each class
concurrent; the miss still cost 988 ms at Profile M, and #187 says where: "the last class is
the systemd enumeration".

Measured on this host with a release build, a miss costs 0.54 s, and `get service` alone 0.39 s
of it: `ListUnits` names every unit in one answer, and each of the 550 units then costs two
`GetAll` round trips for properties a miss never looks at. Every other target answers in
10–70 ms from one kernel dump or one file.

A sweep cannot stop early — an `enter` that did would say `not_found` about something that is
there (ADR-0576). But a sweep is looking for a *name*, and a service place answers to its unit
name and to nothing a name does not contain.

## Decision

**A query may say which name its caller is looking for** — `Query::seeking(text)`. It is
advice, in the sense ADR-0980 gave a pushed comparison: no part of what the query asks for, and
never read by `Query::matches`. A provider whose source lists names more cheaply than objects
may leave out every object whose name does not contain the text, compared without regard to
case, or ignore the advice and answer in full. An answer to a seeking query is never the whole
target, and the caller does not keep it as one.

**The service manager's provider takes the advice**: it filters `ListUnits` by the sought text
and reads the properties of the units that remain.

**The selector sweep gives it** to the targets `ono_spatial_query::discovery::sought_by_name`
names — `service`, and nothing else today. Their answer is absorbed into the index and not
remembered as the target's observation, so a later `look` at `compute/services` still reads
the collection. Every other target is swept as before.

A target qualifies when both hold: its places answer to nothing but their name, so an object
left out for its name could not have resolved the selector; and its source lists names far
more cheaply than objects. A process answers to its executable as well, and the process table
is one read either way.

## Consequences

A miss no longer reads any unit's properties, and a hit by a unit's name reads the units whose
name contains it. Measured on this host, release build, `enter no-such-place-1a2b3c`:
0.54 s before; the figure after, and the Profile M measurement the issue names, are in
`docs/contracts/hardening/performance_baseline.json` and the release's run record.

Hard: §27.3's "did you mean" offers come from the index, so a mistyped unit name is offered a
correction only where the misspelling is contained in the right name's neighbours already
indexed — `enter ngin` is offered `nginx.service`, `enter ngnix` is not unless the services
were looked at. A substring is what a listing can be narrowed by; an edit distance over every
unit name would need every name on this side, which the provider contract does not hand over.

The advice does not cross a link: the remote query carries selectors and a limit, so a far
side enumerates as before.

Encoded by `crates/ono-provider-systemd/tests/service.rs::should_read_only_the_units_whose_name_matches_when_a_name_is_searched_for`
and `crates/ono-cli/tests/spatial_topology.rs::should_say_a_name_is_no_place_without_reading_every_service`.

## Alternatives considered

**A `Selector::Contains` on `name`.** A selector is binding and case-sensitive, and resolution
compares names without regard to case: `enter networkmanager.service` has to find
`NetworkManager.service`.

**Read fewer properties per unit.** The service manager offers all of an interface's
properties or one of them per call.

**Skip the service class when nothing cheaper matched.** That is the early stop ADR-0576
removed: a unit nobody looked at would be `not_found`.

**Index every unit name at startup.** A background enumeration behind every prompt for a
question most sessions never ask.
