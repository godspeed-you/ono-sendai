# ADR-0153: What `pin` stores, and what `jump` takes

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §6.5, §6 (the verb table), §20.4, §26.4, §27.1, §46, §46.1
- Decided by: agent (autonomous, S4c)

## Context

§20.4 gives `pin` three spellings — `pin`, `pin --name edge-proxy`, `jump @edge-proxy` — and one
rule: "Pins MUST store a resilient selector and identity metadata rather than only a rendered path.
If the target cannot be resolved later, the pin remains but reports unresolved state." §6's verb
table adds `unpin`, which §20.4 does not spell. S3 built the store and the resolution
(`PinStore`, `PinRegistry`); what a pin is *made of* when a user types `pin` was left open.

§6.5 gives `jump` three spellings of its own: a place selector, `<link>/<selector>` (S8) and
`@<bookmark>`.

## Decision

**`pin` marks the place the session is standing on.** `--name` names the pin; without it the pin
takes the place's own display name. It answers with the place it marked, as `ono.spatial-place/1`
with `pinned: true` — a command that changed something a user can see says what it changed, and the
answer composes with the pipeline like every other place record (§28.1).

**A pin stores three things**: the `SpatialId` it had, the *name it answers to* as its selector, and
its `SpatialType`. The name rather than the `<type>/<key>` spelling, because the name is the half
that survives what §20.4 is about — a restarted process, a service moved into a container — while
`process/1842` is a pid, and §2.8 says a pid is an attribute. The type is the identity metadata that
keeps `nginx` the service from being re-bound to `nginx` the process. For a canonical space the
selector is the space's own id (`compute`), which resolves at §27.1's step 3 in every session.

**`unpin [<name>]` removes one.** Without a name it removes the pin on the current place, mirroring
the `pin` that had no name either; with no such pin it refuses with `spatial.not_found` rather than
succeeding silently.

**`jump @<name>` resolves a pin, and pins resolve when the store is read.** Every spatial command
that touches pins already reloads and re-resolves them (`with_pins`), so by the time `jump` looks
one up it points at a place rather than a spelling to guess at again. A pin whose place is gone is
`spatial.destination_gone` and **stays in the store** — §20.4's "the pin remains but reports
unresolved state", which is what makes a pin on a host that is merely offline survive the outage.

A pin on a canonical space needs nothing special: the space's id is its selector, §27.1's step 3
resolves a canonical identifier, and `PinRegistry::resolve` re-binds the pin to the identity that
answers. The liveness test stays "the index holds it", exactly as S3 wrote it — a pin whose stored
identity is not an observation is *supposed* to fall through to its selector, and that fall-through
is what `spatial_pins::should_rank_a_pinned_place_first_and_say_so_when_a_search_answers` asserts.

## Consequences

- A pin survives the session and the trail does not, which is exactly the pair §46.1 draws and
  `spatial_contracts_missing::should_keep_the_trail_session_local_while_a_pin_survives_the_session`
  asserts in one test.
- Two places with one name make a pin unresolvable rather than re-bound: §27.3 forbids an
  approximate answer from acting and §29.3 forbids a silent choice between exact ones. The pin
  stays, and the user is told.
- `pin` needs a state directory. A session with neither `XDG_STATE_HOME` nor `HOME` is refused
  rather than given a pin that would vanish.
- §26.4 ("user pins are always landmarks") is already served by the query layer, which ranks a
  pinned place first; nothing here changes that.

## Alternatives considered

- **Store the `<type>/<key>` spelling as the selector.** Resolves more precisely today and breaks
  on exactly the events §20.4 names.
- **`pin <selector>` to pin a place one is not standing on.** Not in §20.4, and `jump <selector>;
  pin` already does it in two words that both exist.
- **Answer nothing from `pin`, like `enter`.** A landmark the user cannot see they created is a
  landmark they will not trust; §50's own bar for a delivered capability is that its output is
  inspectable.
