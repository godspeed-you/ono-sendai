# ADR-0984: The map searches beyond what it drew, and a collection is read behind the prompt

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §6.8, §9.4, §23.3, §23.4, §33.1, §34, §34.1, §43.7; ADR-0176, ADR-0497, ADR-0576; issue #212
- Decided by: agent (autonomous)

## Context

Two things a user reaches for to find a place answered only from what was already on the screen
or in the index (#212):

- §23.3 lists `/` as "search visible/global map". The full-screen map searched the lines it had
  drawn and said "nothing drawn here matches" about everything else — thirty nodes of a system
  with hundreds.
- §9.4 calls completion "a lightweight local map". It asks no provider while Tab is held (§34's
  50 ms), so `enter <TAB>` in a collection nobody had looked at offered the declared geography
  and no members. And the offer was bounded to twenty-four names *before* the typed prefix was
  applied, so in a large collection the name being typed towards was usually not among them.

## Decision

**A query nothing drawn answers is searched for beyond the map.** The view searches its own
lines first and moves the cursor to a match, as before. Where there is none it hands the text
to the shell, which runs the search `find place <text>` runs — the planned targets are asked,
the index is searched (§6.8) — and lists what it found over the map: name, kind and place path,
twelve at most and a count for the rest. Nothing moves: searching is a view action (§23.4), and
`jump <name>` is how one goes there.

**The collection the session stands in is read behind the prompt.** After every command of an
interactive session, when the current place is a canonical collection the session has arrived
at and not yet read, the shell asks the collection's targets whole, on its runtime, while the
prompt is already back (§34.1). It is the sweep `enter <name>` makes on an index miss
(ADR-0497), made once per arrival and ahead of the question. The session is held to learn where
it stands and to register each answer, never across a read, so a slow provider delays its own
answer and no command. An answer that arrives after the session moved on is dropped (§43.7).
A target read whole inside its lifetime is not asked again.

**Completion applies the typed prefix before it bounds the offer.**

## Consequences

Easy: `/postgres` in the map finds the process whatever the map drew; `enter ngi<TAB>` in
`compute/processes` completes without a `look`; the `enter <name>` that follows resolves from
the index.

Hard: walking into a collection costs its whole read once, whether or not the user completes
or enters anything — the price §34.1 means by "expensive discovery ... after prompt
availability". Scripts and `ono -c` never pay it: only the interactive loop discovers. Object
places and domains are not read ahead; their neighbours are edges and declared children.
The search beyond the map asks every planned target, which is `find place`'s cost, on Enter.

Encoded by `crates/ono-spatial-render/tests/view.rs::should_hand_the_query_to_the_shell_when_nothing_drawn_matches_it`,
`crates/ono-cli/tests/spatial_interactive.rs::should_search_beyond_the_drawn_map_when_nothing_on_it_matches`,
`::should_offer_the_members_of_a_collection_nobody_looked_at_when_completion_runs` and acceptance
case `141-search-beyond-the-map-and-discovery-behind-the-prompt`.

## Alternatives considered

**Ask the provider while Tab is held, with a deadline.** The first Tab in a large collection
would wait out the deadline and offer nothing, every time; §34 budgets the offer at 50 ms from
local metadata.

**Re-centre the map on the first place found.** Search would then be navigation of the view's
centre, and a mistyped query would throw away the map the user was reading.

**Read every collection ahead at startup.** The whole system's enumeration behind the first
prompt, for collections the user never enters.
