# ADR-0719: The one historical adapter in the tree is declared rather than special-cased

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §6.8, §7.4, §21.4, §21.5, §22.3, §23.1, §23.2, §23.3, §23.4, §25.5, §35.3;
  v0.2 §50; v0.3 §1.8, §1.44; ADR-0055, ADR-0085, ADR-0710
- Decided by: agent (autonomous)

## Context

v0.5 §22.3 makes journald "a provider-owned historical event source for service/system events" and
adds a condition: the implementation "SHOULD query structured journal fields rather than scrape
human-formatted `journalctl` text where a native API is practical. If v0.3's `journalctl` adapter
is used instead, provenance MUST identify the adapter path." ADR-0085 already took that second
road for good reasons — `journalctl --output=json` is a machine format systemd documents, it needs
no C library, and the v0.3 systemd pack already decodes it — and `reshape` already carries the
adapter trace into every record's provenance.

What was missing was the declaration. §23.1 lets an adapter contribute temporal evidence *only*
where its manifest declares it, and forbids inferring temporal structure from arbitrary output
merely because a command has been adapted. §23.2 then fixes what such a declaration contains:
`historical_query: true`, coverage semantics, the source timestamp mapping, the identity mapping,
and the deduplication key where the tool has one.

None of that was in the manifest. `crates/ono-provider-systemd/src/journal.rs` built
`--since=@<seconds>` in Rust, borrowing the pack's decoder and nothing else. The one historical
adapter in the tree was a special case in a provider rather than a declared plan.

## Decision

**The adapter pack contract gains a `temporal:` block, and the `journalctl` adapter declares one.**
Its presence is the claim, so a block declaring `historical_query: false` is a contradiction the
validator refuses rather than a way to opt out; an adapter with no history declares no block. The
five keys are §23.2's five requirements, one each:

- `coverage: retained-window` — the window is `journald.conf`'s and rotation's, which Ono neither
  controls nor reads, so a coverage record from this plan covers what the answer actually spanned
  and claims nothing about what came before the earliest entry (§7.4).
- `source_time: timestamp` — `__REALTIME_TIMESTAMP`, journald's own microsecond wall clock for the
  entry, which the pack's `fields:` already maps.
- `identity: [boot_id, unit]` — a journal entry is not an object with a lifetime; what it is about
  is a unit, inside a boot. §25.5 makes the boot the clock domain the instant belongs to, so an
  entry from a previous boot is never silently ordered against one from this boot.
- `deduplication_key: cursor` — `__CURSOR`, journald's own unique opaque handle for one entry.
  Two reads of one journal agree on it exactly, which is what lets an overlap between two windows
  be recognised rather than ingested twice (§6.8).

**A sixth key, `plan`, makes the declaration executable.** It names the adapter's own invocation
and the argv templates a bound is written into — `--since=@{seconds}` and `--until=@{seconds}`.
Without it a caller asking about a window would have to know journalctl's flags and its time
formats, which is exactly the knowledge the adapter exists to hold; and `@<seconds>` rather than
one of the human forms journalctl also accepts, because a locale-dependent spelling is what §50
forbids a provider to rely on.

`JournalProvider::history` now asks the contract how a bound is spelled, and refuses with
`temporal.unsupported_source` where the pack declares no plan. That refusal is §23.1's rule read
from the calling side: no manifest declaration, no temporal evidence.

**The validator holds the plan to its own declaration.** Every mapping must name a canonical field
the adapter actually produces, the invocation must be one the adapter declares, and each bound
template must carry `{seconds}` for the instant to go in. A mapping onto a field that does not
exist is a declaration nothing can act on.

**No other adapter in this tree declares a block, and a test says so.** §23.3 is explicit: `ps`,
ordinary `ss`, `ip address` and `lsblk` "remain current observations unless their underlying tools
expose history". §23.4 stands untouched — raw fallback stays raw, and nothing here parses its
text.

## Consequences

- `history journal --since … --until …` reaches journald as a bound rather than being filtered
  afterwards, so the journal does the narrowing and an answer is what the journal had.
- A second historical adapter is a manifest edit rather than a provider edit: declare the block,
  and the validator states what it must contain.
- `docs/reference/adapters/` is generated from the packs and now has a plan to describe.
- **The provider's `historical_query` claim and the adapter's are two statements, and they can
  disagree.** `JournalProvider::temporal()` says `historical_query: true` because the pack
  declares a plan today; a pack edited to drop the block would make the provider refuse at
  runtime while still advertising the capability. The refusal is honest and the advertisement
  would be stale. Tying the provider's claim to the pack it loads is the obvious next step and is
  not taken here, because a `temporal()` that reads a bundled contract is a claim that varies with
  what a package installed — which is a KUANG/11 question, not a v0.5 one.
- **`retained_history` stays `None`.** journald states its retention in `journald.conf`, and this
  provider does not read it. §35.3: unknown stays unknown rather than becoming forever.

Encoded by `crates/ono-adapter/tests/contracts.rs` — six cases over the declaration and its
validator — and by `crates/ono-provider-systemd/tests/journal_history.rs`, which asks a real
journal about a real past window and checks that every entry is inside it, names its boot, and
carries a cursor two reads agree on.

## Alternatives considered

**Use `sd-journal` through `libsystemd` and drop the adapter.** §22.3's preferred road, and
ADR-0085 rejected it for reasons that have not changed: a C library dependency on every build, for
a format `journalctl --output=json` already exposes. §22.3 permits the adapter path explicitly, on
the condition that provenance names it, which it does.

**Leave the flags in `journal.rs` and declare only the five descriptive keys.** It satisfies the
letter of §23.2 and leaves the provider knowing journalctl's spelling — which is the special case
this ADR exists to remove, and the reason a second historical adapter would have needed a second
provider to know a second tool's flags.

**A free-text `coverage:` string.** More expressive, and unusable: a coverage record has to be
built from it, and "whatever the journal happens to keep" is not a value a reconstruction can
reason about. Two closed words are enough for what the tree has, and a third is a contract edit.
