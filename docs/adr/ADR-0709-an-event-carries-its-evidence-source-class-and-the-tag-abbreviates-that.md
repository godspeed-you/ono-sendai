# ADR-0709: An event carries its evidence source class and the tag abbreviates that

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §7.1, §11.5, §35.1, §39.3; v0.2 §25.2; ADR-0620
- Decided by: agent (autonomous)

## Context

§11.5 ends with one sentence about the tag on a timeline row:

> Source tags SHOULD be abbreviated but inspectable.

and shows `[systemd]` and `[ono]`. §7.1 fixes the nine evidence source classes the tag abbreviates:
`ono.session`, `ono.recorder`, `linux.procfs`, `linux.netlink`, `linux.systemd-dbus`,
`linux.journald`, `adapter:<id>`, `remote:<link>/<provider>`, `kuang:<package>/<provider>`, and
requires them to have "stable inspectable identity".

`ono.temporal-event/1` declared no field for that class. The renderer derived the tag as
`provenance.source ?? provenance.provider`, abbreviated by a rule of its own. The first half of
that derivation is wrong. `ono_value::Provenance::source` is documented as "what the observation
was read from, such as a list of procfs paths", and the tree fills it accordingly:
`org.freedesktop.systemd1.Manager` in the systemd record path,
`org.freedesktop.login1.Manager.ListSessions + Session properties` in logind, a procfs path
elsewhere. Abbreviating it prints `[Manager]` for a systemd row.

The second half is right, and had been all along: a temporal event's `provenance.provider()` is the
§7.1 class by convention across this tree — `linux.procfs`, `linux.systemd-dbus`, `ono.recorder`,
`linux.journald` — and `EventId::of` already digests it under the name `source` (ADR-0620). So the
class was carried. It was carried in a field whose type says nothing about §7.1, beside a field of
free text that a renderer was preferring to it.

## Decision

**1. `ono.temporal-event/1` declares a nullable `source` carrying the §7.1 evidence source class.**
The tag is then an abbreviation of a declared, closed-vocabulary fact rather than of free text.

**2. `value::event_record` fills it by reading the provenance's provider back through
`EvidenceSource::parse`.** §7.1's list is closed, so a name outside it — `linux.sock-diag`,
`ono.runtime` — leaves the field null rather than being waved through as a class it is not.

**3. A null `source` draws no tag from `provenance.source`.** The renderer prefers the declared
field, falls back to `provenance.provider` for a record from an older producer, and never consults
`provenance.source`. §11.5's tag is a SHOULD; printing `[Manager]` is worse than printing nothing.

**4. The abbreviation rule stays in the renderer and stays documented** — the table in
`abbreviate_source`. Abbreviation is presentation, and `source` keeps the unabbreviated class one
`inspect` away, which is what "inspectable" asks for.

## Consequences

- A systemd row reads `[systemd]` whatever the D-Bus interface name in its provenance is.
- An event whose producer names no §7.1 class renders untagged. That is visible, and it is a signal
  that the producer should name one.
- `EventId` does not move: the digest already covered the provider name, and this adds a field to
  the record rather than to `EventSeed`.
- `TemporalEvent` gains no field. The class stays where the digest already reads it from, and the
  record is where it becomes typed.

## Alternatives considered

- **Add `source: EvidenceSource` to `TemporalEvent`.** Rejected for this increment: it would move
  the same datum into a second place beside `provenance.provider`, which `EventId::of` digests, and
  every ingest path, the ledger codec and every fixture would have to state both and keep them
  equal. The record-level field gets the type where the type is needed.
- **Keep the derivation and document it.** Rejected: the derivation prefers the wrong field. A
  documented wrong answer is still wrong.
- **Have the renderer resolve the event's evidence ids to their sources.** Rejected: §39.3.
