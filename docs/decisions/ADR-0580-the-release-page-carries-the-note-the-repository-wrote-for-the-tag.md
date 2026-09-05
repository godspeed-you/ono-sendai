# ADR-0580: The release page carries the note the repository wrote for the tag

- Status: accepted
- Date: 2026-09-05
- Spec refs: v0.4.1 §49.4, §66.8; ADR-0532, ADR-0577, ADR-0579
- Issues: none (found by the `v0.4.1` release)
- Decided by: agent (autonomous)

## Context

`v0.4.1` published nine verified assets and a page that said this and nothing else:

```
**Full Changelog**: https://github.com/godspeed-you/ono-sendai/compare/v0.4.0...v0.4.1
```

`publish-release.sh` drafted with `--generate-notes --title "$tag"`, so the page carried the
commit range and the bare tag. Meanwhile `docs/releases/v0.4.1.md` — eleven kilobytes describing
what the tranche did, the four §33.2 targets before and after, the selector miss that is still
outside its target, and the two boxes the tag itself closed — sat in the repository, held against
the checklist on every gate run by ADR-0577, and reached nobody.

That is §66.8 met on one side and dropped on the other. The bullet asks that *"`docs/STATE.md`,
acceptance documentation and release notes agree on status"*, and ADR-0577 built the check that
keeps the note honest. A note nothing publishes is a document that agrees with the checklist in
private.

`v0.4.0`'s page carries its note in full, with the note's own heading as the release title. So the
convention existed; only the automation did not know about it, and the first release after that
automation was written is the one that noticed.

## Decision

`publish-release.sh` drafts from `docs/releases/<tag>.md` when there is one: `--notes-file` for the
body and the note's first line, less its `# `, for the title. That is the same shape `v0.4.0`'s
page has, produced rather than pasted.

Where no note exists the script keeps `--generate-notes` and titles the release with its tag, and
says on standard error that it did. The commit range is worth more than an empty page, and a
release whose note nobody wrote should look different from one whose note nobody published.

## Consequences

Easy: a release page says what the release did, without a maintainer remembering to paste it, and
the document that says so is the one the gate already holds to the checklist. The two are the same
text by construction rather than by discipline.

Hard: the release title now depends on a Markdown heading. A note whose first line is not `# Title`
produces an empty title, which `gh` accepts. The fallback covers a missing file and not a
malformed one, and nothing checks the shape of the heading — the tests here assert that the title
is what the note's first line says, so a note with no heading would fail them rather than the
release, which is the earlier of the two places to find out.

Encoded by `xtask/tests/provenance.rs::should_draft_a_release_from_the_note_this_repository_wrote_for_the_tag`
and `::should_draft_the_commit_range_when_no_note_was_written_for_the_tag`, which drive the script
against a `gh` stand-in the way ADR-0579's tests do.

`v0.4.1`'s own page was corrected by hand — `gh release edit --title --notes-file` — because it was
already published when this was found. Every release after it gets the page from the script.

## Alternatives considered

**Leave it to the maintainer.** That is what `v0.4.0` did, and it worked because one person
remembered. The first release published by the automation forgot, which is the argument.

**Generate the note from the checklist at publish time.** ADR-0577 rejected the same idea from the
other end: a release note is prose for a person and a checklist is evidence for a gate. The note is
written, reviewed and committed like anything else; publishing it is a copy, not a generation.

**Fail the release when no note exists.** A `v*` tag that somebody cut in a hurry should still
publish verified bytes. The note being absent is worth a line on standard error, not a refusal to
attach packages that are already signed.
