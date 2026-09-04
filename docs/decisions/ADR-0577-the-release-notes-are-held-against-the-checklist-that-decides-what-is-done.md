# ADR-0577: The release notes are held against the checklist that decides what is done

- Status: accepted
- Date: 2026-09-04
- Spec refs: v0.4.1 §66.8, §66.9, §54.3; ADR-0402, ADR-0529, ADR-0542, ADR-0545, ADR-0575
- Issues: #116 in part (the box §4.8.12 left open for it)
- Decided by: agent (autonomous)

## Context

§66.8's fifth bullet asks that *"`docs/STATE.md`, acceptance documentation and release notes agree
on status"*. ADR-0402 built the first half of that: `cargo xtask state-check` refuses a
release-ready verdict while the work board holds a claim or an unexplained deferral. The release
notes were the surface nothing read, and §4.8.12's box said so in its own text — it named a test
`xtask/tests/scan.rs::should_report_release_notes_that_disagree_with_the_checklist` that did not
exist, beside a `docs/releases/v0.4.1.md` that did not exist either, and stayed open for both.

The notes are the one document a reader outside this repository actually sees. `docs/ACCEPTANCE.md`
is 2 250 lines of boxes and `docs/STATE.md` is four thousand lines of working history; nobody
installing a `.deb` reads either. So a release whose notes are silent about a box its own checklist
leaves open has taken that decision on the reader's behalf without telling them, and §66.9's whole
exclusion mechanism — a P2 or P3 item may remain open only through an ADR made before the freeze —
becomes a private arrangement between the tranche and itself.

## Decision

`xtask::scan::check_release_notes` runs on every gate run, beside the other scans, and reports
three disagreements:

1. **There is no `docs/releases/v<version>.md`** for the version `[workspace.package]` declares. A
   version bump without notes is caught by the bump.
2. **The notes do not open by naming that version.** A note whose first line says another version
   is a note about another release.
3. **The checklist leaves a box open that the notes do not name.** The box's own bolded title, less
   its `P2 ·` prefix, is what the notes have to contain — a reader has no use for the priority, and
   requiring the exact sentence would make the check about wording rather than about disclosure.

The third is the one worth having, and it is deliberately a *containment* check rather than a
semantic one. Nothing here can tell whether the notes describe an open box honestly; what it can
tell is whether they mention it at all, which is the difference between an omission somebody chose
and one nobody noticed.

## Consequences

Easy: §4.8.12's status-documents box closes, and `docs/releases/v0.4.1.md` cannot fall behind the
checklist without the gate saying so — including in the direction that matters at the end of a
tranche, when the last boxes are being ticked and the notes were written before them.

Hard: writing the release notes becomes part of the gate rather than part of publishing. A commit
that ticks the last box of a tranche and leaves the notes as they were is red, which is the
intended pressure and is also one more thing to do in the increment that closes a release.

This release is the first to run under it, and it is why v0.4.1's notes carry a *Known, measured,
not hidden* section naming both boxes that remain open — the signature over the checksum manifest
and the verification instructions, which ADR-0529, ADR-0542 and ADR-0545 record as waiting on a
signed release, and which the tag that produced this release is the run that closes.

Encoded by `xtask/tests/scan.rs::should_report_release_notes_that_disagree_with_the_checklist`,
`::should_accept_release_notes_that_name_every_box_the_checklist_leaves_open`,
`::should_report_release_notes_that_do_not_exist_for_the_version_the_workspace_declares`,
`::should_report_release_notes_that_open_by_naming_another_version` and
`::should_find_this_repositorys_release_notes_in_agreement_with_its_checklist`.

## Alternatives considered

**Generate the notes from the checklist.** A release note is prose for a person and a checklist is
evidence for a gate; generating one from the other would produce a document nobody would read, and
§66.8 asks that they *agree*, not that one be the other.

**Check the notes against `docs/STATE.md` as well.** `state-check` already holds the board, and a
third pairwise comparison would make the same claim a third time. The checklist is the document
that decides what is done (§3), so it is the one the notes are held against.
