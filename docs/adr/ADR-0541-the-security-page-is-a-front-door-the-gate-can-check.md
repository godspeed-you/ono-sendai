# ADR-0541: The security page is a front door, and the gate checks it is one

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §51.4, §5.1, §5.3, §6.1, §6.2, §15.2, §20
- Decided by: agent (autonomous)

## Context

The repository had no `SECURITY.md`. The README's Security section carried three sentences: report
privately through GitHub, do not post an exploit, there is no published contact. §51.4 asks for
four things — supported versions, how to report privately, the high-level trust boundaries, and
that a public issue is the wrong place for an unpatched exploitable vulnerability — and says what
the file is *not*: "This file does not replace the architectural security specification."

Two other sections point at it. §5.3 ends with a sentence no other section in the specification
has: after saying that v0.4.1 does not claim to defend against a compromised kernel, a root
attacker on the same host, malicious hardware, or a native plugin under the same Unix account,
it says **"The product MUST say this plainly."** And §15.2 fixes the wording of the native trust
statement. Neither had a home on a page a security researcher would find.

The question was what a gate can hold a hand-written page to without making it generated.

## Decision

**`SECURITY.md` is written by hand and checked for the things §51.4 names.** Four rules, all of
them about whether a reader can act:

- **The reporting path.** The page names a channel, a timescale, which versions are supported, and
  says a public issue is the wrong place for an unpatched exploitable vulnerability. Checked by
  the words a page cannot answer those without — a page that says "report responsibly" gives a
  finder nothing.
- **The protected assets.** All nine of §5.1 appear by name. §51.4 asks for the boundaries, and a
  boundary is only meaningful beside what it protects; a page naming eight of nine has dropped one
  from the model the reader is shown, silently.
- **The out-of-scope statement.** The compromised kernel, the root attacker on the same host and
  malicious hardware are each named as out of scope. This is §5.3's "plainly", made checkable.
- **The native trust statement.** Already enforced: `SECURITY.md` joins `README.md`,
  `PHILOSOPHY.md` and `CONTRIBUTING.md` in the terminology gate's user-facing set, so §15.2's
  statement is required of it by the rule that already existed (ADR-0447, ADR-0536).

**The response times are commitments, not aspirations.** Seven days to acknowledge, fourteen to
assess, ninety to fix or state a plan. A front door with no answer behind it sends the next report
to the public tracker, so the page states times the project will meet and says it will tell a
finder before a deadline passes rather than after.

**The boundary table is written, not generated.** §6.1's machine-readable inventory —
`docs/contracts/hardening/security_boundaries.yaml` — is owed by issue #118 and does not exist. The
eleven rows on the page are §6.1's own list, transcribed, and the page says which component owns
each. When the inventory lands, this table is the obvious thing to generate from it, and
`docs/ACCEPTANCE.md` §4.8.12 now says so rather than claiming a check nobody wrote.

## Consequences

- A security researcher has a place to look, a channel that needs no third-party service, and a
  stated expectation. GitHub's private vulnerability reporting is used because §47.5's rule for
  release verification applies here too: no proprietary service, and no email address the project
  cannot actually monitor.
- The page says plainly what is not protected, including the two things a reader is most likely to
  assume: that a native KUANG/11 plugin is contained (it is confined, which is not isolation) and
  that configuration and commands the user wrote are attack surface (they are not — the shell
  obeys its user, and `explain` is the protection offered).
- §5.1's nine assets now exist as a list in `xtask::terminology::PROTECTED_ASSETS`. That is a
  second copy of immutable specification text, which §52.2 dislikes; the alternative is a gate
  that reads the narrative specification as a contract, which AGENTS.md §5.1 forbids. The copy is
  nine short nouns and it is where a reviewer sees it.
- `docs/ACCEPTANCE.md` §4.8.12's box claimed the file "is held against the boundary inventory so a
  new boundary cannot be added without appearing there". No such inventory exists; the box now
  names the two tests that do.
- Encoded by `xtask/tests/terminology.rs::should_find_every_protected_asset_of_the_threat_model_in_the_security_document`
  and `::should_find_a_reporting_channel_and_a_response_expectation_in_the_security_document`.

## Alternatives considered

- **Generate `SECURITY.md`.** Rejected: §51.4 wants a front door, and a front door is prose. The
  parts that are data — the terminology, the refusal census, the six remote trust concepts — are
  generated already and linked from it.
- **Leave the README's Security section as the only page.** Rejected: GitHub surfaces
  `SECURITY.md` in the repository's security tab and in the "Report a vulnerability" flow, and a
  researcher who does not find it there will use the issue tracker.
- **Publish a security email address.** Rejected: the project has none it can commit to
  monitoring, and an address nobody reads is worse than no address. Private vulnerability
  reporting is on the repository already.
- **Check only that the file exists.** Rejected under §20's spirit: a box that closes on a file
  being present is a box that closes on a file being empty.
