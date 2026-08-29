# ADR-0163: The landmark engine — which of §26.2's rules the core actually runs

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §26, §3.7, §2.11, §2.17, §24.3, §25.4, §47
- Decided by: agent (autonomous, phase S5)

## Context

§50 assigns the landmark engine to no phase: S7 has "landmark updates" and S9 has a "landmark
API", but nothing delivers the rules of §26.2. S5 cannot avoid it — `map --json` must report
landmarks and their reasons, and ranking needs them — so S5 delivers it.

§26.2 lists rules under four headings; §3.7 fixes fourteen reasons and closes the set for
built-ins; §26.3 requires the thresholds to be inspectable and configurable and warns that "Ono
MUST avoid pretending that a local heuristic is an incident".

Three of §26.2's rules name no §3.7 reason at all — "interface down", "route change", "unusually
high traffic", "new remote peer" — and several others name evidence no installed provider serves.

## Decision

`ono_spatial_query::landmark` is the engine. It is handed one `SpatialObject`, the record its
provider last answered with, and `LandmarkThresholds`, and answers with the reasons that object
deserves attention for. It runs in `SpatialSessionState::absorb`, so `look`, `near` and `map` all
see the same landmarks and ranking has them before it chooses what to show.

**Implemented, each reading a field a shipped schema declares:**

| §3.7 reason | Rule | Evidence |
|---|---|---|
| `high_cpu` | `ono.process/1.cpu` at or above `spatial.landmarks.high_cpu` | `cpu 87%` |
| `failed` | the provider's own `state` is `failed` | `state failed` |
| `recently_changed` | `started`/`since`/`created` inside `spatial.look.change_window` | the timestamp |
| `public_listener` | a socket in `listen` state whose local address is not loopback | `listening on 0.0.0.0:443` |
| `storage_pressure` | `used`/`size` at or above `spatial.landmarks.storage_pressure` | `93% used` |
| `privileged` | uid 0, **as an attribute of an object promoted for another reason** | `running as root` |
| `security_boundary` / `remote_boundary` | a scope boundary between the current place and the object | the boundary |
| `user_pinned` | the user's own pin (§26.4) | the pin |

**Not implemented, and why — each is an absence, never an approximation:**

- `high_memory`. §26.2 asks for memory "relative to host/cgroup budget". No provider serves a
  budget: `ono.process/1` has `memory` in bytes, `ono.system/1` has no total, `ono.cgroup/1` has
  no limit. A share cannot be computed without one, and the spatial layer may not read the system
  itself (§2.16). The threshold setting stays; the rule waits for a provider.
- `restarting`. §26.2's restart loop needs a restart count; `ono.service/1` declares none.
- `connection_spike`, `new_object`, `removed_object`. All three are differences between two
  observations, which is §25.4's snapshot comparison — phase S7.
- §26.2's "interface down", "route change", "unusually high traffic", "new remote peer". §3.7
  closes the built-in vocabulary, and none of the fourteen reasons names any of them. A core
  landmark may not invent a reason, so these four surface as changes when S7 can see a change.

**`privileged` never promotes an object on its own.** §26.2 asks for it "when context makes it
relevant", and on an ordinary Linux host most processes run as root; promoting all of them is the
alert board §26.3 forbids. It is therefore an attribute added to an object something else already
promoted — a root-owned public listener, a root process at high CPU.

## Consequences

`map`, `look` and `near` report real landmarks with real evidence. A null field yields no
landmark, because "A landmark whose evidence is unavailable is not a landmark; it is an unknown"
(`docs/spec/spatial/landmarks.yaml`). The thresholds are read from the user's configuration in
`crate::spatial::configure_from`, so §26.3's "configurable" is true rather than advertised.

Five of the fourteen reasons are never produced by this build. That is visible here rather than
hidden behind a rule that silently never fires.

## Alternatives considered

- *Approximating `high_memory` from resident bytes against a guessed budget* — rejected: §2.17
  and §35.3 make an invented number worse than a missing one.
- *Promoting every root-owned object* — rejected by §26.3, and it would flood the ranking.
- *Running the engine inside each view* — rejected: three views would then disagree about what
  deserves attention, and ranking would have to recompute it per projection.
