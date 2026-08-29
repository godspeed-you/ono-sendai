# ADR-0142: The shape of a place view, and what the root place is

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §3.1, §3.3, §3.6, §6.1, §7.1, §24.1, §29.1, §30.2, §35.2, §46; v0.2 §33.5;
  ADR-0129, ADR-0140, ADR-0141
- Decided by: agent (autonomous)

## Context

§6.1 fixes `look --json -> PlaceView` and nothing else about it: neither the field names, nor
whether the place and its neighborhood are one record or two, nor which schema the root place
reports as its `object_type`. §7.1 gives the root a conceptual `SystemPlace` with `hostname`,
`os`, `kernel`, `uptime`, `domains`, `landmarks`, `links` and `generated_at`, but the shell has no
provider that answers for an operating system, a kernel release or an uptime — there is no
`get system` — and §2.16 forbids the spatial layer from becoming one.

`ono.host/1` already exists and means something else: an entry of the link table, a host Ono can
reach (v0.2 §14). Reporting it as the type of the place a session stands in would make the root of
*this* machine indistinguishable from a remote host record.

## Decision

**One document, three readings of the same facts.** `look --json` writes exactly one
`ono.place-view/1` record, serialized as v0.2 §33.5 serializes any record, on one line:

- `id`, `type`, `label`, `hostname` — the identity and state §24.1 puts first;
- `place` — the full `ono.spatial-place/1` record of §3.1, the same one `find place` streams and
  `near` embeds, so a search result, a neighbour and the current place never read differently;
- `groups` — the exits of §24.2, and `neighborhood` — the §3.6 projection they came from, whose
  `groups` is the *same list*. A reader that found two different answers would have found a bug;
- `landmarks` — §3.7, present and empty rather than absent when there are none (§2.17);
- `domains` — §7.1's list, present at the root, where the six domains *are* the exits, and null
  anywhere else, because a domain is not a neighbour of a process;
- `system` — §7.1's `SystemPlace` in full, carried by `look --all` only (see below);
- `changed` — §24.3's section, null unless `--changes` asked for it.

**The root place's `object_type` is `ono.system/1`**, a schema this increment declares from §7.1
field for field. The object a session stands in when `home` answers is the system itself. Its
`os`, `kernel` and `uptime` are nullable and are null in this build: no provider answers for them,
and §2.17 and §35.3 require the unknown to be visible rather than fabricated or read behind the
providers' backs. Every other canonical space reports `ono.spatial-place/1`, as ADR-0140 already
decided: a domain is declared geography, and a collection is not one of its members.

**`--all` widens the neighborhood, not the properties.** §24.1: `look` "MUST NOT default to
dumping all properties of the underlying object", and `inspect` remains the exhaustive view. So
the default carries counts and states, and `--all` carries the places behind every exit
(`members`) and, at the root, the `SystemPlace` the default only names.

**A place carries its identity, not its properties.** `ono.spatial-place/1` gains `identity` — the
named components §3.1 composes the opaque `spatial_id` from (a boot and a pid, a uid, a unit name)
— and `permission`, the §35.2 state of what the place holds. Neither is a property of the object:
`pid` is how you recognise the place, `cpu` is one `inspect` away.

**`enter` moves the place, in every spelling.** §30.2 is one sentence — "`enter` changes the
spatial place" — and it does not exempt the target forms v0.2 already had. `enter process 1842`
now pushes the v0.2 context frame *and* moves the session's place; §30.4 keeps them separate
pieces of state. A first word that names no declared `enter <target>` is a place selector and
reaches the v0.4 command, which is what lets `enter compute` and `enter service nginx` keep their
own meanings without a second vocabulary.

## Consequences

- `look --json`, `near` and `find place` all emit places built by one function
  (`crate::spatial::view::place_record`); a field added for one is added for all.
- `ono.system/1` is declared and mostly null. The phase that adds a system producer fills it; until
  then `look --all` at the root says "not known", which is a real answer and a visible gap.
- The change section can be asked for and never lies: with no event source and no comparison
  snapshot, `changed.state` is `unsupported` and `entries` is empty (§24.3).
- Tests that encode it: `spatial_navigation_missing::should_describe_the_current_place_as_a_structured_view_when_look_runs_without_a_tty`,
  `…::should_read_back_into_the_pipeline_when_look_json_is_parsed_by_from_json`,
  `spatial_topology_missing::should_describe_the_current_place_with_an_id_kind_name_scope_and_permission_when_looking`,
  `spatial_map_missing::should_describe_identity_state_exits_and_landmarks_when_look_json_reports_a_place`,
  `…::should_not_invent_a_change_section_when_no_snapshot_or_event_source_exists`,
  `spatial_relationships_missing::should_keep_the_current_place_when_trace_projects_the_relationship_graph`.

## Alternatives considered

- **`object_type: ono.host/1` for the root** — rejected: `ono.host/1` is the link table's record
  for a host Ono can reach, and the root of the local machine is not one.
- **A flat `PlaceView` without `place`/`neighborhood`** — rejected: §3.1 and §3.6 are two contracts
  and both are asserted; carrying them as named members costs nothing and keeps each nameable.
- **Reading `/proc/sys/kernel/osrelease` for `os` and `kernel`** — rejected: §2.16 forbids the
  spatial layer from becoming an undocumented source of system truth. `local_scope()` reads the
  hostname because identity needs it (ADR-0141); a display field is not that.
- **`look --all` dumping the object's properties** — rejected by §24.1, which reserves that for
  `inspect`.
