# ADR-0034: `watch` is a polling diff first, and says so

- Status: accepted
- Date: 2026-08-26
- Spec refs: §4.4, §18.2, §18.3, §31.14, §34; ADR-0024
- Decided by: agent (autonomous)

## Context

ADR-0024 fixed the semantics of live streams; this records the decisions made implementing the
first one. The provider API has carried `subscribe` since Phase C, but no built-in provider
implements it yet — netlink and D-Bus can push events, procfs never will — and Phase F cannot
wait for the best transport to deliver the capability.

## Decision

### The runtime polls, and every event admits it

`watch <target>` snapshots at the configured interval, diffs by object identity (ADR-0024's
sameness rule), and emits `snapshot`, `added`, `changed` and `removed` events in the envelope of
spec §31.14 — with `source: poll` on every one, because spec §18.2 requires polling to be
explicit rather than a cost invisible until someone profiles it. When a provider grows
`subscribe`, the same command switches transport and the field says `subscription`; nothing a
consumer parses changes shape.

The default interval is **2 seconds**: fast enough that a live table feels alive, slow enough
that watching every process on a loaded machine costs nothing anyone notices (spec §34).
`--every` overrides it.

### The diff is deterministic

Changes are emitted in identity order, not provider order, so two runs over the same transitions
produce the same stream. `changed` events name the fields that moved. A schema with no declared
identity keys each value by its whole self — appended, never "the same object changing"
(ADR-0024).

### The live table is a presentation with a frame budget

At a terminal, events fold into a table keyed by identity and the screen repaints **at most
every 250 ms**, over the previous frame. The frame deadline is fixed rather than restarted per
event — the first implementation restarted it, and a stream busier than the frame rate painted
nothing at all, which failed exactly on the fastest watches. Changes faster than a frame
coalesce (ADR-0024's default policy); a tick that changed nothing repaints nothing (spec §4.4).

Anywhere that is not a terminal, an unbounded unserialised stream is refused with the fix named:
`watch process | to json`, or bound it with `take` (spec §18.3's own piped form). An endless
stream into a table renderer would be a table that never learns its widths, and buffering it
forever would be worse than saying so.

### Event schemas are per target and arrive with their targets

The contracts declare `stream<ono.<target>-event/1>` per watch command; `ono.process-event/1` is
written and embedded, the rest stay in `docs/spec/schemas/deferred.yaml` where spec-check tracks
them. A watch whose event schema has not landed reports exactly that.

## Consequences

- `watch process --every 1s | where process.cpu > 20` works today against every provider, and
  provider-native subscriptions are an optimisation to make later, not a prerequisite.
- The polling diff holds the previous snapshot per watch: memory proportional to the population,
  which is the same order as the table showing it.
- Tests: ono-command/tests/watch.rs drives a mutating fixture through a full poll cycle;
  ono-cli/tests/watch_live.rs pins the piped refusal, the bounded serialised form, and — through
  a real PTY — the in-place repaint and Ctrl-C ending the watch with 128+SIGINT.

## Alternatives considered

- **Wait for provider subscriptions.** Rejected: Phase F's criterion is live semantics that
  work, and a procfs provider will never push events anyway — the poll path is permanent, not
  scaffolding.
- **One generic `ono.object-event/1` schema.** Rejected: the contracts already promise per-target
  event schemas, and a generic envelope would make `where process.cpu > 20` impossible to check
  against a declared field list (spec §11.3).
- **Rendering the event envelope in the live table.** Rejected: the table a person watches shows
  the objects; the envelope is for pipes and programs (spec §18.3 draws exactly this line).
