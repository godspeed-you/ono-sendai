# ADR-0977: The edges of an object place are recalled inside their lifetime

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §10.3, §25.3, §32.1, §32.2, §33.1, §33.2, §33.3, §33.4, §34; ADR-0186, ADR-0190, ADR-0495; issue #216
- Decided by: agent (autonomous)

## Context

ADR-0186 gave the canonical spaces an observation cache — a repeated `look` at `compute` reads
the index — and left the object places out: a process, a socket or a file expanded its
relationship providers on every `look`, `near` and `map`, and honestly said `polled`. §33.1
lists the "known relationship summary" among what the index holds, and §34 budgets a cached
`look` at 50 ms. Five looks at one process cost 0.54 s in a debug build on this host, each one
reading `/proc/<pid>/fd` and the socket table again.

## Decision

**The session remembers each expansion of an object place's relationships**: when it was made,
how much it asked for — everything (`--all`), one named relation, one named kind of neighbour,
or the default — and the kinds of place the answer is about.

**An orientation view recalls it** — `look`, `near`, `map` — when both hold:

- *It is fresh.* Every kind of place the answer is about is inside its lifetime under §33.3's
  policy, shortest first: the openers of a file go stale as fast as processes do, whatever the
  file's own lifetime is.
- *It covers the request.* A complete expansion covers any request. Short of that, an expansion
  covers the default view and the same named relation or kind again. A request it did not pay
  for asks the providers, as §32.2 requires of an exit that was left unloaded.

**The place itself is asked about every time.** §33.2 keeps the provider the authority on
whether an object exists, so a recalled view still puts the one pinned query for the object, and
a place that has gone is found gone on the next look (§10.3). What is recalled is the
neighbourhood.

**A view whose neighbourhood was recalled says `cached`** (§25.3, §33.4); one that asked says
`polled`, and `stale` and `partial` outrank both, as for a canonical space.

**`follow` and change planning always ask.** Traversing needs a live place and a change
revalidates what it is about (§10.3, §33.2); neither recalls. A live view drops what the session
remembers before each projection, as ADR-0190 decided for targets.

## Consequences

Easy: five looks at one process cost 0.17 s where they cost 0.54 s (debug build, this host);
five `near --type process` at a file cost 0.12 s where they cost 0.36 s. A full-screen view that
repaints an object place stops re-reading descriptor tables per frame.

Hard: an object place's exits can be up to one lifetime old — five seconds for a process — and
say so. A socket a process opened two seconds ago is on the next look after the lifetime, or on
the next `follow`, `near <relation>` it did not cover, or `--all`.

Encoded by `crates/ono-cli/tests/spatial_relationships.rs::should_answer_a_repeated_look_at_an_object_place_from_the_edges_it_already_read`,
`::should_ask_again_when_a_later_view_wants_more_than_the_earlier_one_read`, the tombstone tests
of `crates/ono-cli/tests/spatial_identity.rs` (a visited process that exited is reported gone on
the next look) and acceptance case `137-object-place-edges-are-recalled`.

## Alternatives considered

**Recall the place as well** — no provider asked at all. A process that exited would keep
answering `running` for five seconds on the place the user is standing on, which is the claim
§33.2 forbids; the pinned query costs a few milliseconds.

**Cache per relationship provider.** Finer, and every caller would have to reason about which
providers an interest reaches; the interest already says what was asked, and one record per
place is what `covered` needs.

**Merge the coverage of successive expansions.** A later narrower expansion would inherit the
age of an earlier one, and the lifetime would be claimed from the wrong instant.
