# ADR-0538: The six remote trust concepts are a registry, and each states what it does not

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §51.3, §7.3, §4.2, §4.3, §2.1, §2.2, §9.1, §10.1, §10.2, §65.1, §65.2, §65.3
- Decided by: agent (autonomous)

## Context

§51.3 lists six things remote-link documentation must distinguish and does not say how. The
reason the list exists is §7.3: the cryptographic transport identity and the runtime
`Identity { user, uid, elevated }` are different fields, one is a credential and the other is a
self-report, and §65.1 and §65.2 are the two named failure modes that follow from confusing them.
Every entry on the list is there for being mistakable for another entry on the same list.

The Wiki's `Remote-Links.md` had a "Security, stated plainly" section written for v0.4.0. It
described the ssh and tcp transports correctly and said nothing at all about client authorization
— which did not exist when it was written — so a reader who pinned a host key had no way to learn
that pinning is about the server and grants them nothing. The repository itself had no remote
trust page for a gate to read.

`docs/ACCEPTANCE.md` §4.8.12 also named the six wrongly: "Transport encryption, transport
authentication, host pinning, client authorization, self-reported identity and runtime user" is
six items, and it is not §51.3's six — it splits the identity metadata in two and drops both
transports and the capability negotiation.

## Decision

**`docs/spec/hardening/remote_trust.yaml` holds the six.** Each concept carries what it
`establishes`, what it `does_not`, the §6.1 boundary it belongs to, the commands that operate it
and the sections that fix it. `docs/reference/remote-trust.md` is rendered from it by
`cargo xtask docs`, so the repository has a remote trust page the gate can read and nobody
maintains by hand.

**"Distinguish" is checked as a shape.** A page satisfies §51.3 when, for each of the six, it
names the concept and says what that concept does not establish. Both halves are required: a page
that lists six headings and describes only what each one gives you is exactly the page that lets a
reader believe a host pin authorized them.

**The checked phrase is a `distinguisher`, not the whole paragraph.** Each row carries the
shortest phrase that proves the distinction was made — `"peer key visible to ono: no"`, `still
refused until it is authorized`, `says nothing about whether this client may connect`, `It is not
authentication and does not replace it`, `It grants nothing`, `The offer is a view of the policy`.
`does_not` contains it verbatim, so the generated page passes by construction and a hand-written
page passes by saying the same thing in the same words. Requiring a whole paragraph word for word
would be a rule against editing prose; requiring only the heading would be no rule at all.

**The Wiki page was rewritten to keep the six apart** and is checked by
`cargo xtask terminology --wiki <path>`, for the reason ADR-0536 records: the Wiki is a separate
git repository and no gate run reaches it.

## Consequences

- The repository now states its remote trust model in a place a gate reads, and the Wiki says the
  same thing. A seventh concept, or a change to what one of the six means, is a registry edit that
  moves the page and the rule together.
- The Wiki's remote page gained client authorization, which it had never mentioned: the
  `add client-key` step, the `remote.unauthorized` refusal and the fingerprint the refusal carries
  so an operator can paste it back.
- `boundary` is recorded and not yet validated for membership.
  `docs/spec/hardening/security_boundaries.yaml` — §6.1's inventory — is owed by issue #118 and
  does not exist, so §4.8.12's claim that the page "is held against the §6.1 boundary inventory"
  was a claim about machinery nobody has built. The box now names the two tests that do exist and
  says the join is #118's; this ADR is where the debt is recorded.
- `docs/ACCEPTANCE.md` §4.8.12's list of the six was corrected to §51.3's.
- Encoded by `xtask/tests/terminology.rs::should_find_all_six_remote_trust_concepts_described_separately`
  and `::should_report_a_remote_page_that_leaves_one_of_the_six_to_be_inferred`.

## Alternatives considered

- **Write the six into the Wiki only.** Rejected: §51.3 is a documentation requirement and the
  Wiki is unreachable from the gate, so the rule would have had nothing to hold.
- **Check that each concept's full `does_not` paragraph appears.** Rejected: two repositories
  would then have to carry identical prose forever, and the first honest edit to either would turn
  the gate red for a change that improved the page.
- **Check only that the six headings appear.** Rejected: the heading is not the distinction. §51.3
  exists because the concepts are confusable, and a page that names all six and conflates two of
  them is the page it is written against.
