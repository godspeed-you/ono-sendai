# ADR-0171: A detached link stays in the map, and what is behind it is `stale`

- Status: accepted
- Date: 2026-08-28
- Spec refs: §19.1, §35.2, §35.4, §53, §25.3, §2.17; v0.2 §9.1
- Decided by: agent (autonomous, phase S8)

## Context

§19.1 puts the links into the local root's view with a state beside each — `connected`,
`disconnected  last seen 3h ago` — and §35.2 requires the six permission states to stay distinct,
with §53 restating it: "Unknown/denied data? Distinct from empty."

v0.2 §9.1 defines `detach link` as "detach active link/context … without stopping target". The
implementation pops the link's frame and keeps the connection, and three green v0.2 tests pin
that: `should_pop_the_link_frame_when_detaching`, `should_keep_the_link_when_detaching` and
`should_answer_again_from_a_detached_link_when_it_is_entered_again` — the last of which requires
`enter link` after a detach to answer from the far side again, so the socket must survive.
`should_detach_the_piped_link_when_detach_link_follows_get_link` pins the answer for a link that
was never entered: `{"changed":false,"status":"success"}`.

S8 requires two things of the same `detach link testbox` on a link that was never entered: the
link stays in the §19.1 map with a state that is not `connected`, and a place behind it reports
`stale` rather than an empty neighbourhood. Under the v0.2 behaviour alone, `detach link` would
have changed nothing observable and the two views before and after it would be identical.

## Decision

`detach link` has two effects, and they answer two different questions.

1. **The v0.2 one, unchanged.** It pops the link's frame. `changed` reports whether there was one,
   so a link that was never entered still answers `changed: false`, and the link is never torn
   down.
2. **The v0.4 one.** This session stops *following* the link's space. Nothing is keeping the
   places behind it current any more, so §35.2's word for them is `stale`.

`changed` stays `false` in case (2) because nothing about the *link* changed: it is still held,
still connected, still enterable. What changed is this session's relationship to it, which is
session state and not a property of the target.

The §19.1 link map therefore reports its own state, in §35.2's vocabulary, and it is not
`ono.link/1`'s:

| link | §19.1 state |
|---|---|
| established and followed | `connected` |
| established, detached from | `stale` |
| defined but never negotiated, or torn down | `disconnected` |

A link is never dropped from the map to say it is not connected — §19.1's own example keeps
`home/nas01 disconnected` in it.

Standing on a host whose link is not reachable, `look` and `near` ask **nothing**: every exit is
withheld with state `stale` and a detail naming the link, the place's `permission` is `stale` and
its `freshness` is `stale`. That is not only honesty about age. Provider calls fall back to the
local registry when there is no reachable link (§14.4), so asking would answer a question about
`testbox` with this machine's objects — which §35.4 and §2.16 both forbid.

## Consequences

- New contract `docs/contracts/schemas/link-place.v1.yaml` (`ono.link-place/1`), and a nullable `links`
  field on `ono.place-view/1`, present at the root of a host and null anywhere else.
- `enter link` and `jump <link>` follow the link again, so its places stop being stale;
  `link host` starts a freshly negotiated link as followed.
- The follow flag is session state kept beside the spatial state (`crates/ono-cli/src/spatial/links.rs`),
  not a field of `ono.link/1`: `get link` still answers exactly what v0.2 declares.
- Encoded by `spatial_remote_missing::should_list_a_linked_host_among_the_places_when_looking_at_the_local_root`,
  `…should_keep_a_detached_link_visible_with_its_state_in_the_link_map` and
  `…should_report_a_place_behind_a_detached_link_as_stale_rather_than_empty`.

## Alternatives considered

- **Make `detach link` hang up.** Breaks `should_answer_again_from_a_detached_link_when_it_is_entered_again`
  and contradicts v0.2 §9.1's "without stopping target".
- **Report `ono.link/1`'s own state in the link map.** Then a detached link reads `connected`,
  which promises a freshness nothing delivers, and the §19.1 map cannot express "disconnected"
  at all for a link `link host` created.
- **Add `following` to `ono.link/1`.** A change to a v0.2 contract for a v0.4 view's benefit; the
  spatial layer's own record is the right place for the spatial layer's own question.
