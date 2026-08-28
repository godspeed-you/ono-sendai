# ADR-0150: What a navigation step carries, and what `trail` answers

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §6.7, §20.1, §20.2, §6.4, §6.5, §3.2, §29.1, §46.1
- Decided by: agent (autonomous, S4c)

## Context

§20.1 schemas a `NavigationStep` with six fields — `timestamp`, `from`, `to`, `movement`,
`relation?`, `scope_crossing?` — and §6.7 lists what the trail MUST preserve. Both `from` and `to`
are `SpatialId`s, and a `SpatialId` in this build is an opaque digest (ADR-0129): `ono:lifetime:73d9…`
says nothing to a reader, and two of them say nothing about a path.

Three demands meet on the same record. §44.7 needs the trail to keep an exited process and its
replacement apart, which only opaque identity can do. §6.7 is a command a person reads, and a
history of digests is not a history. And §6.4 requires "the relation traversed" to be recorded,
while a relation has one declared id and two ends — `docs/spec/spatial/relations.yaml` gives
`process.owns_socket` both a `canonical_label` (`socket`) and an `inverse_label` (`owner`), so the
id alone does not say which way the movement went.

## Decision

**`ono.navigation-step/1` is the record `trail` answers with**, and it carries §20.1's six fields
plus four that make them readable, never instead of them:

| field | what it is |
|---|---|
| `from`, `to` | the opaque `SpatialId`s of §20.1. Identity, and the only fields §44.7 may be judged on. |
| `from_ref`, `to_ref` | the `<type>/<key>` spelling of §11.2 — `process/1842` — built from the first identity field of the place's `canonical_ref`. The spelling `enter` and `jump` take back. |
| `from_name`, `to_name` | `display_name`, for a person. |
| `relation` | the **word** the traversal was spelled with — `socket`, `parent`, `file`. |
| `relation_id` | the relation `relations.yaml` declares — `process.owns_socket`. |
| `movement`, `timestamp`, `scope_crossing` | §20.1, unchanged. `scope_crossing` is a record: the scope left, the scope entered, the kind of boundary, and whether it is remote. |
| `host` | the host scope the movement happened on, so a cross-host trail reads back unambiguously (§19, and what S8 needs). |

To carry the word, `ono_spatial_core::NavigationStep` gains `spelled(word)` beside `along(relation)`.
The word is what `follow` was given; the id is what the registry declares. Neither is derived from
the other, because deriving the word from the id needs the direction, and deriving the direction
needs the word.

**`trail` has three spellings.** Bare, it streams `ono.navigation-step/1` records that compose with
`where`/`take`/`count` like any other stream (§29.4). `--json` writes them as one JSON array, the
way `look --json` writes one document (§29.1). `--compact` writes the breadcrumb of §20.2 — the
canonical hierarchy path of where the session is standing, `local > compute > processes > 1842` —
because §20.2 says in as many words that full breadcrumbs "MAY … be shown by `trail`".

**The trail is not persisted.** §46.1 disables trail persistence by default and §53 settles it as
"session-only"; `spatial.trail.persist` exists in the settings catalogue and this build implements
its default and nothing else, so a new session starts with an empty trail while its pins survive.

## Consequences

- A script reads the trail as an ordinary object stream; `trail --json | from json` round-trips.
- `back` is itself a step the trail keeps (§20.3's "retain the original trail record"), so reading
  the trail shows the walk that happened, not a tidied version of it.
- A step whose place the session has since forgotten reports `null` for the ref and the name and
  keeps the id: unknown is visible (§2.17), and identity never degrades to a name.
- S5's map and S8's federation both read the same record. S8 will need `host` to become per-step
  rather than the session's, which is a one-field change at the point a remote place is entered.
- Tests that encode it: `spatial_navigation_missing::should_record_every_movement_with_its_kind_and_relation_when_the_trail_is_read_as_json`,
  `spatial_relationships_missing::should_record_the_relation_it_traversed_when_a_follow_enters_the_trail`,
  `spatial_identity_missing::should_not_confuse_the_old_and_the_new_process_when_a_place_is_replaced`,
  and case `docker/acceptance/cases/104-spatial-back-up-home-trail.case`.

## Alternatives considered

- **§20.1's six fields and nothing else.** Honest to the schema and useless to a reader: `trail`
  would print two columns of digests, and §6.7 exists to be read.
- **`from`/`to` as rendered names, ids in extra fields.** Reverses the invariant: §44.7 asks the
  trail to tell an exited process from its replacement, and names cannot.
- **Deriving the word from the relation id and the place's type.** Fails exactly where it matters:
  a relation between two objects of the same type (`process.parent_of`) has both its words
  available at both ends, so the derivation cannot say which one was taken.
