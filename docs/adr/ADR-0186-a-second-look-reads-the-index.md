# ADR-0186: A second `look` reads the index, and says `cached`

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §33.1, §33.2, §33.3, §34, §25.3, §2.17
- Decided by: agent (autonomous, S4d/S4e)

## Context

§34 budgets a warm `look` at under 50 ms and §33.1 says where the answer comes from: an in-memory
spatial index holding identity, aliases, canonical parent, relation summary, freshness and
landmark state. Until this increment the index was filled on every command: `look` asked every
provider target behind the current place's exits, absorbed the records, and only then read the
index it had just refilled. The index was a *result*, not a cache, and twenty-one `look`s at the
root cost twenty-one sweeps of `/proc`, netlink, systemd over D-Bus and sysfs.

§33.2 constrains the fix from the other side — "The index is a cache. Providers remain
authoritative. Actions MUST resolve/revalidate live objects before mutation" — so the cache may
not become the truth, and it may not be unbounded in time. §33.3 gives the lifetimes, which
`ono_spatial_index::FreshnessPolicy::recommended()` already implements per place kind.

## Decision

**The session remembers what each provider target answered and when, and a command inside that
target's §33.3 lifetime reads the answer instead of asking again.**

- `SpatialSessionState::remember`/`recall` hold one `TargetObservation` per provider target: the
  timestamp, the places it produced grouped by kind, its §35.2 refusal where it refused, and
  whether it served at all. The observation is what `look`, `near` and `map` build their exits
  from, so all three agree by construction (§49.5).
- The lifetime is the index's own TTL policy — §33.3's table, in one place — taken over the kinds
  of place the target answered with, **shortest first**: a target that yields processes goes
  stale in five seconds even when it also yields something slower. A target that answered with no
  places at all takes the policy's default.
- **The cache is per session and per process.** It dies with the shell (§29.2), and a fresh `ono`
  observes everything again, which is what makes an id comparable across runs meaningful.
- **Mutation is untouched.** Nothing here changes the road an action takes: `ono-command`'s
  mutations resolve their subject through the provider, exactly as §33.2 requires, and never
  through the spatial index.
- **A view that was read says so.** §25.3's vocabulary has a word for it, and `look --json`'s
  `freshness` is `cached` when every target the view needed was recalled and nothing was asked.
  `polled` stays the word for a view that did ask, `stale` and `partial` outrank both.

## Consequences

- The marginal cost of a repeated `look` at the root drops from ~70 ms to ~44 ms in a **debug**
  build on a loaded machine, and no provider is asked at all in the repeat — which is the part
  §33.1 is about. What remains is the projection itself: ranking the neighbourhood and building
  the records. S11 owns the §34 budgets as release gates, measured against a release build.
- A place view can now be up to one TTL old, and it says which of the two it is. That is exactly
  the trade §33.3 describes, and §25.3's `stale` still fires for a place the index holds past its
  own lifetime.
- An object place (a process, a socket, a directory) still asks its relationship providers on
  every look: relationship edges are not in this cache, so those views honestly stay `polled`.
  Caching them is a later increment with its own test.
- Exit test:
  `crates/ono-cli/tests/spatial_contracts_missing.rs::should_answer_repeated_looks_far_inside_the_look_budget`.

## Alternatives considered

- **Cache the finished place view.** Cheaper still, and wrong: the view depends on the request
  (`--all`, `--changes`, `--type`), and a cached view would go on claiming exits that a later
  observation changed. The cache belongs at the provider boundary, where §33.3 puts it.
- **A single global TTL.** §33.3 is explicit that "different object classes require different
  freshness policies", and the policy already exists in the index; a second table would drift
  from it.
- **Refresh in the background after the prompt (§34.1).** The right eventual answer and a much
  larger change: it needs the update channel S7 builds for the live map. Recorded in
  `docs/STATE.md` rather than half-built here.
