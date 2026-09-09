# ADR-0653: Historical path structure needs one of four supports and a current reading is not one

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §7.5, §14.5, §21.1, §21.4, §21.5, §37.3; v0.4 §15.4, §15.5
- Decided by: agent (autonomous)

## Context

§14.5 is the only place in v0.5 where the specification names a capability and then refuses it.
"Filesystem paths are especially difficult because normal filesystems do not retain arbitrary
historical directory trees. Ono MUST therefore show only historical filesystem structure
supported by: recorder checkpoints; filesystem-specific snapshot providers; explicit
audit/inotify/FSEvents-like evidence sufficient for reconstruction; KUANG/11 providers. No
generic v0.5 implementation may pretend that current directory contents represent the past."

The failure mode is specific and easy to reach by accident: a directory listing is cheap, it
always answers, and it answers about now. Nothing in the reconstruction path distinguishes it from
an observation about `T` unless something is made to.

## Decision

**The four bullets are a function, and the default answer is `None`.**

`SourceMatrix::structure_support(source, from_checkpoint)` returns which of §14.5's four supports
applies, in the order the specification lists them:

| support | condition |
|---|---|
| `Checkpoint` | the state came from a checkpoint (§42.2) |
| `KuangProvider` | `EvidenceSource::is_plugin()` — a `kuang:` source (§37.3) |
| `SnapshotProvider` | the source declared `historical_query` (§21.4) |
| `AuditEvidence` | the source declared `exhaustive_events` (§21.5) |

A source that declared only `current_snapshot` gets `None`, which is the sentence "current
directory contents are never the past" written as a match arm. A source the matrix does not name
declared nothing, and nothing is the honest default: §21.5 forbids advertising `exhaustive_events`
"merely because events usually arrive", so an unstated capability is a capability the source does
not have.

The rule applies to `SpatialType::File` and `SpatialType::Directory` and to nothing else. Mounts,
filesystems and block devices are named in §42.2 as things a checkpoint holds, and they are
bounded sets a provider enumerates; the path tree under them is the thing that is not.

An object refused this way is **left out of the reconstruction and a gap is added** —
`<type>.existence`, reason `unsupported`, over the whole window (§7.5). A caller therefore sees
that historical path structure was asked for and refused, rather than an empty directory that
looks like an answer. §7.5's "gaps MUST be shown where they materially affect a query" is exactly
this case.

The reused vocabulary is deliberate. `TemporalCapabilities` already carries the three flags this
needs and providers already declare it (§21.1), so §14.5 costs no new declaration surface and a
provider cannot opt into historical paths without also claiming, in the source matrix the gate
checks, a capability it can be held to.

## Consequences

- `at 12:17` followed by `look` in a directory reports a gap rather than today's listing, on every
  machine with no snapshot provider — which is every machine v0.5 ships to.
- A ZFS or Btrfs snapshot provider, a KUANG/11 package, or an inotify recorder makes historical
  paths work by declaring what it is, with no change here.
- A checkpoint that captured a directory tree replays it, because §42.2 permits a policy that
  admits the class and ADR-0652 makes that a deliberate decision.
- Encoded in `crates/ono-temporal-reconstruct/tests/absence.rs`.

## Alternatives considered

- **Show current directory contents with an `as_of` label.** Named and forbidden by §14.5, and
  it is the single most convincing wrong answer this crate could give.
- **Refuse the whole reconstruction when a path is involved.** Punishes the processes and services
  in the same answer for a class nobody could have captured.
- **A dedicated `filesystem_history` capability flag.** A fourth flag beside three that already
  say it, and one more thing for `docs/contracts/temporal/sources.yaml` to drift on.
