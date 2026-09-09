# ADR-0811: Recovery planning attributes a change before it gates on it, and digest evidence outranks a timestamp

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.6 §24.2, §24.5, §40.1, §56.3, §62.8, Appendix C.3, Appendix C.4
- Decided by: agent (autonomous)

## Context

Appendix C.3 asks the RecoveryPlan to classify "changes since asset creation within the candidate
restore scope" as preserved, discarded, conflicting or unknown. Appendix C.4 gives the worked
example: plan a82f wrote `/etc/nginx/nginx.conf` at 14:03, the user edited it again at 15:12, and
recovering toward the 14:02 snapshot would discard the 15:12 edit.

Read literally, the object changed twice after the asset was captured, and the first of those
changes was the plan's own. An analysis that counts both gates every recovery that ever happens,
because every recovery is a recovery of something the plan changed. §40.1 — apply on a low-risk
plan is sufficient intent — would then be unreachable for recovery, and §24.5's gate would degrade
into the "are you sure?" §40.2 forbids.

A second question sits behind it. Appendix C.3 says "changes", and there are two kinds of evidence
that one happened: a modification time that moved, and content that differs. They disagree. A file
rewritten with identical bytes has a newer mtime and nothing to lose; a file restored from a backup
can have an older mtime and completely different content.

## Decision

**The original plan's own write is not newer state.** `ConflictRequest` carries the instant the
source plan was applied, and a change at or before it produces no item at all. Appendix C.4's
conflict is the *second* edit, which is what its own worked example shows.

Where the applied instant is unknown — recovering toward an asset directly (§5.8's second form),
or from a plan whose apply record is gone — any change to a restored object is `Conflicting`.
§56.3's direction is to block rather than guess, and the guess here would be in the losing
direction.

**Digest evidence outranks a timestamp.** Where both a captured and a current digest exist, the
comparison decides and the timestamp is ignored. Where no digest exists on either side, the change
time answers. Where neither exists, the answer is `Unknown` — and `Unknown` gates, because
`NewerStateClass::requires_acceptance` answers `true` for everything except
`PreservedByMethod`.

**The gate is stricter than the plan's own predicate.** `RecoveryPlan::needs_destructive_acceptance`
lets an acceptance flag clear an unanalysed recovery, which is the right shape for a value type
that cannot know why it was asked. `gate::check` refuses one outright with
`recovery.plan_incomplete`, and the reasoning is that §24.5's acceptance is acceptance of a
**known** loss. There is nothing to accept when the analysis did not run, and offering a flag that
says "proceed anyway" for an unanalysed recovery is §62.8 with an extra step.

## Consequences

The ordinary case stays ordinary. Recovering a config file that nobody has touched since needs no
gate, which is what makes the gate mean something when it does appear.

An operator who recovers toward an asset rather than a plan gets a gated recovery even where
nothing conflicts. That is a real cost, and it is the correct direction: without the plan, Ono does
not know which of the changes it can see were its own.

`recovery.plan_incomplete` now carries four situations — no usable asset, no method that meets the
goal, no action to run, and a host that holds no asset. A dedicated `recovery.method_unavailable`
would read better in `inspect recovery`, and is worth adding if the distinction turns out to matter
to a caller rather than only to a reader.

## Alternatives considered

**Treat every change after the asset as newer state, and rely on the operator to read the list.**
Rejected: the list would always contain the object being restored, so the gate would fire on every
recovery and be learned as noise.

**Use the modification time alone.** Rejected: it is the evidence that is wrong most often, in both
directions, and the two failures are asymmetric — a false conflict is an annoyance, a missed one is
data loss.
