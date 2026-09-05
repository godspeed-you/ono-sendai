# ADR-0465: A plugin runs as you, and that was the whole argument

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §15.1 (required terminology), §15.2 (native trust statement), §17.3 (no
  security marketing ahead of implementation), §51.1, §65.5 ("sandbox" as marketing shorthand);
  AGENTS.md §8 (an accepted ADR is not edited); ADR-0422 (partly superseded here), ADR-0447 (the
  terminology gate), ADR-0448
- Decided by: agent (autonomous)

## Context

`docs/adr/ADR-0422-the-rpm-database-is-one-provider-with-three-front-ends.md` decides that
the rpm database is one provider with three front ends. In its *Alternatives considered* section
it rejects building the same thing as a KUANG/11 plugin, and gives this reason:

> **A KUANG/11 plugin.** Rejected, and not possible today: `provider.query` is dispatched only
> for targets the package itself contributes (`ono-kuang-supervisor/src/supervisor.rs`), there is
> no host→plugin call for an action at all, and package mutations need root while a plugin **runs
> sandboxed under the shell's uid**.

The emphasis is mine. That clause is the claim v0.4.1 §65.5 names as a failure mode and §17.3
forbids: a `native-process` plugin executes as an ordinary process of the Ono user, with process
confinement and no kernel isolation. It can open any file and reach any network that account can,
without asking Ono at all. Calling that sandboxed describes a boundary that is not there.

It was found by the H4 agent while correcting the same wording in `README.md`, `PHILOSOPHY.md`,
`help` and the Wiki (issue #63, ADR-0447), and it was deliberately left alone: AGENTS.md §8 makes
an accepted ADR a historical record that no agent edits, so `xtask::terminology` excluded
`docs/adr/` entirely. That exclusion is what this ADR is really about. It meant a false
security claim could sit in the repository with the gate green, and that the *next* one would sit
there too.

## Decision

**Two things, and the second is the one that matters.**

**1. ADR-0422's rejection stands, and its reason is corrected here.** The corrected clause reads:

> package mutations need root, and a KUANG/11 plugin executes as a process of the Ono user.

The conclusion does not move an inch, because the argument never depended on the false half.
What rejects the plugin alternative is that a plugin runs as *you* and `rpm -e` needs root — and
that is true, and is exactly what "runs as a process of the Ono user" says. The word "sandboxed"
added a claim the argument did not need and the runtime does not support. ADR-0422's `Status`
becomes `superseded by ADR-0465 (in part: the sentence about a plugin running sandboxed)`,
following the form ADR-0331 already established for a partial supersession. Its body is untouched.

**2. `xtask::terminology` now reads the decision records, and a superseded record is out of
scope.** AGENTS.md §8 forbids editing an accepted ADR, and that is precisely why holding one to
the terminology looked impossible — the gate would have demanded a correction the rules forbid
making. The way out is that §8 permits one correction: superseding. So an **accepted** record is
held to the vocabulary, and a record whose `Status` names a superseding ADR is not, because the
correction has been written and lives in the newer record. The rule points at the one repair that
is allowed, rather than at one that is not.

Two narrowings keep it honest:

- **Only the assertion rule applies, never the disclaimer obligation.** A decision record is not a
  description of the runtime, so demanding §15.2's statement in every ADR that mentions the native
  tier would be asking a decision to carry documentation prose. Asserting an isolation that does
  not exist is false wherever it is written; omitting a disclaimer is a gap only in a document a
  user reads.
- **A phrase inside backticks or quotation marks is a mention, not a claim.** ADR-0447 defines
  this vocabulary and must name every term in it, and a rule that caught the naming would delete
  the record that carries the rule. The same carve-out is what lets this ADR quote the sentence it
  is correcting.

## Consequences

- The next ADR that writes "runs sandboxed" fails the gate on the commit that writes it, which is
  where the mistake is and where it is still cheap to fix.
- ADR-0422 is now discoverable as partly superseded, so a reader who arrives at its plugin
  argument finds the correction rather than believing the sentence.
- An ADR can be taken out of the rule's scope by superseding it, and by nothing else. Marking a
  record superseded is a real act with a successor attached, so the escape is not free.
- Scanning 268 records costs one read each, in a check that already walks the tree.
- The Wiki still cannot be reached from the gate (ADR-0447), and is held by hand. Issue #112 owns
  extending this to the generated reference and to §19.1's remaining six terms.

## Alternatives considered

- **Leave ADR-0422 as a historical record and change nothing.** Rejected. It is not a record of a
  belief that later turned out wrong — the runtime never had that boundary, on the day the
  sentence was written. Leaving it would also have left the exclusion, and the exclusion is the
  actual defect.
- **Edit the sentence in place.** Rejected: AGENTS.md §8, without exception. It would also destroy
  the evidence that the claim was ever made, which is the thing worth keeping.
- **Supersede ADR-0422 as a whole.** Rejected: its decision about the rpm provider is correct and
  in force. `superseded by … (in part: …)` says what actually happened.
- **Exempt ADR-0447 by name.** Rejected in favour of the quoting rule, which is a reason rather
  than a list. A named exemption would have to be extended by hand for every future ADR that
  discusses the vocabulary, and each extension is a chance to exempt one too many.
