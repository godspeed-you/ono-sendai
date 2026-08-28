# ADR-0129: A `SpatialId` is an opaque digest over a named identity

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.8, §3.1, §10.1, §10.2, §10.3, §20.4, §33.4, §42.1, §42.2, §45.1;
  v0.2 §27.3 (object identity), ADR-0015 T13 (pid + start time)
- Decided by: agent (autonomous)

## Context

§3.1 requires a `SpatialId` to be "opaque to users and stable for as long as the implementation
can truthfully identify the same conceptual object", and says nothing about what it looks like.
Everything else in v0.4 leans on it: `pin` stores one (§20.4), `back` compares them (§20.3), a map
node carries one (§22), `jump` resolves to one (§6.5), and §42.1 makes "two observations of one
object resolve to the same id" a provider conformance test.

Three properties are in tension. It must be **stable** — the same object, twice, in two sessions,
is one id. It must be **opaque** — §3.1 says so, and an id a user could compose by hand would be a
way around the identity rules of §10. And it must be **explainable** — §40's
`spatial.identity_conflict` has to be able to say *which* fact two providers disagreed about.

§33.4 additionally asks `inspect` to reveal source freshness, and gives no vocabulary for it.

## Decision

1. **Identity is a named, ordered list of components**, held as `SpatialIdentity`: the tier
   (§10.1), the spatial type, and the facts that make the object that object. The display name is
   never one of them (§3.1). A process's are the four of §10.2 — boot identity, pid, start time,
   pid namespace — and never the pid alone (§2.8).
2. **`SpatialId` is a digest over that identity**, rendered `ono:<tier>:<32 hex>` (a SHA-256
   prefix). Equal identities give equal ids, which is §42.1; different identities give different
   ids, which is §42.2's reuse safety.
3. **The tier stays legible in the rendering.** §10.1 forbids implying stable persistence for a
   Tier C object, and a renderer that can read the tier off the id can keep that promise without
   asking anyone.
4. **`SpatialId::parse` accepts only what this crate produced.** A hand-written `process/1842` is
   not an id, so a selector cannot become a place by being spelled like one.
5. **The pre-image stays on the `SpatialIdentity`**, not on the id. A conflict diagnostic reads it
   from the object; an id in a pin file or a protocol frame carries nothing but itself.
6. **Freshness vocabulary** (§33.4, which fixes none): `live` (a subscription is delivering
   changes), `fresh` (within the class's TTL, §33.3), `stale` (observed, but older), `unknown`
   (never observed, or no observation time stated). The last is the one that matters: §2.17 makes
   "nobody has looked" a different fact from "it is current", and a three-valued vocabulary would
   have collapsed them.
7. **Identity is scoped.** Every non-process identity carries the scope chain it was observed in,
   so the same uid or unit name in two containers is two objects (§16.2), and the same object on
   two hosts is two places (§19).

## Consequences

- Comparing, hashing and storing ids is cheap and total, which `back`, the index and a live map
  all need.
- An id cannot be reverse-engineered into the object. That is the point, and it means every
  diagnostic that wants to name an object must carry the object, not only its id — the code is
  written that way (`SpatialObject::identity`).
- A change to what goes into an identity changes every id of that type. That is correct — it is a
  change to what "the same object" means — but it invalidates stored pins, so §20.4's rule that a
  pin stores "a resilient selector and identity metadata rather than only a rendered path" is what
  makes such a change survivable. The index re-resolves a pin whose id no longer matches.
- The digest is truncated to 128 bits. Collisions are not a practical concern at the scale of one
  host's objects, and a collision would be a wrong answer rather than a crash — which is why the
  index reconciles against the provider's own `ObjectRef` before acting (§33.2).

## Alternatives considered

- **A structured, readable id** (`process:testbox/boot-a:1842:…`). Rejected: §3.1 says opaque, and
  a readable id invites users and scripts to compose one, at which point the identity rules of §10
  are advisory.
- **Reusing `ono_provider_api::ObjectId`.** Rejected: it is the schema plus the schema's identity
  fields, which for a process is `(pid, started)` — correct for spec v0.2's purposes and short of
  §10.2's four parts, and it carries no scope, so the same uid in two containers would be one
  object. `ObjectRef` is kept *beside* the spatial id, as the thing every action resolves through.
- **A monotonic counter assigned by the index.** Rejected: it would not be stable across sessions,
  so §42.1's own test could not be written, and a pin could not survive a restart.
