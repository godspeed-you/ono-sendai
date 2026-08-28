# ADR-0149: `enter`, `follow`, and where the working directory goes

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §6.3, §6.4, §11.1, §15.1, §27.1, §28.2, §29.3, §30, §40; v0.2 §14.3, §20.2
- Decided by: agent (autonomous, phase S4b)

## Context

§6.3 and §6.4 are two movements: `enter` walks the canonical hierarchy and reaches a place by
name; `follow` traverses a relationship edge and "MUST NOT" traverse a hierarchy one. §40 gives
fourteen conditions and the tests fix which applies when. §30 then says what either does to the
filesystem working directory, and §30.4 says what neither may do to `PWD`.

## Decision

### `enter` resolves a selector, and plans what it costs to

Resolution follows §27.1: what is declared and visible answers first, then what the current
space holds, and only then does the *selector* decide which provider targets could hold the
answer — the same planning `find place` does for a predicate (`ono_spatial_query::targets_for`).
So `enter 1842` reaches the process from anywhere, and a search that could only be a question
about processes does not walk the filesystem to find out (§34).

Three spellings resolve besides a name: an exact `SpatialId`, a canonical space id, and
`<type>/<key>` — §11.2 and §27.2 write a place as `process/1842` whenever the bare key would be
ambiguous.

A pipeline result is entered by §28.2, whether it arrives through the pipe (`… | enter`) or as a
value the argument expanded to (`enter @-1`). A `find place` row and a `near` neighbour are
already places and carry their `spatial_id`; anything else is projected. A value §7 gives no
place is `spatial.not_enterable` — §37.2's "raw command output never becomes a place".

`@-1` in a command argument now names what it names at the head of a pipeline. v0.2 §20.2 gives
the shell retained results, and the invocation scope carries them, so one reference means one
thing in both positions of the language.

### `follow`'s four refusals stay distinct

- the word names a **canonical space** — `spatial.no_relation`, with `enter` named as the way in
  (§11.1: hierarchy is not the graph, and saying so is more useful than "unknown word");
- the place **has** that relation and the provider refused it — `spatial.permission_denied`;
- the place has it and **nothing in this build serves** it — `spatial.unsupported`;
- the place has it and **has no edge along it** — `spatial.no_relation`;
- the word is a relation of **another kind of place**, or of none — `spatial.not_found`, because
  the name was understood but not here.

Several edges of one relation are `spatial.ambiguous_selector` with the candidates listed
(§6.4's "interactive selection is required", §29.3's "scripts never open pickers"). A selector
picks among them by closeness: an exact name outranks an alias, an alias outranks containment,
and among equals the shorter name is the more specific — so `follow socket :443` reaches the
listener `127.0.0.1:443` and not the connection `10.0.0.5:51722 -> 127.0.0.1:443` that merely
ends there.

### `enter` a path is the shell's, because only the shell has a working directory

§30.2: "Entering a directory place changes both spatial place and cwd to that directory. Entering
non-filesystem places MUST NOT change cwd." A word that can only be a path — absolute, `./`,
`../`, `~`, `.` or `..` — is therefore dispatched to the shell, which observes the filesystem
object through the provider, moves the place, and moves the working directory **only when the
object is a directory**. §53 settles the sharp case: a file has a path and is not a directory.

A bare name stays a place selector. §27.1 resolves a visible child before anything global, and a
file in the working directory must not shadow a canonical domain.

`cd` moves the place in the other direction, under §47's `spatial.follow_cwd`: `storage-only`
(the default) synchronises only while the place is already in the filesystem/storage family,
`always` everywhere, `never` nowhere. §30.3 gives the reason: a `cd` must not end a process
investigation.

`PWD` is the working directory and nothing else (§30.4). The session now states its own at
startup rather than inheriting whatever its parent left there, because every external command
reads it.

## Consequences

- `enter` reaches every place kind: a domain, a collection, a name, a pid, an exact id, a path,
  a pipeline result. `enter .` is the working directory, which is also §6.3's own spelling.
- A refused `enter` moves neither the place nor the working directory (§40).
- Tests: `spatial_navigation_missing::{should_move_into_the_hierarchical_child_…,
  should_move_into_the_selected_object_when_a_pipeline_result_is_entered,
  should_traverse_the_relationship_edge_when_following_the_parent_relation,
  should_answer_no_relation_when_following_an_edge_the_current_place_does_not_have,
  should_resolve_the_ambiguity_when_the_script_names_the_exact_spatial_id}`, the seven §30 tests
  of `spatial_storage_missing`, and
  `spatial_relationships_missing::should_refuse_the_traversal_with_no_relation_…`.

## Alternatives considered

- **Refuse an ambiguous `enter @-1` rather than entering the first.** §28.3 makes several
  results a collection place, not an error; the collection place is S5's, and until it exists
  entering the first ranked result is the answer §29.3 permits ("the script explicitly selects
  first/unique") and the tests expect.
- **Let the spatial `enter` command change the working directory.** A command implementation is
  handed an `Invocation`, not the shell; the working directory is session state for the same
  reason `cd` is a builtin.
- **Treat every word that names an existing file as a path.** A file called `compute` in the
  working directory would then shadow the COMPUTE domain.
