# ADR-0449: A dependency arrives with a reason, or it does not arrive

- Status: accepted
- Date: 2026-09-02
- Spec refs: spec §45.1 (advisory scanning), §45.2 (licence and source policy), §45.3 (git
  dependencies), §45.4 (new cryptographic dependencies), §62.3 (dependency audit), §65.11
  (mutable release inputs)
- Issues: #101
- Decided by: agent (autonomous)

## Context

ADR-0433 pinned every input CI pulls from outside the repository, so the same commit now resolves
to the same eleven references twice. It said nothing at all about *which* crates those inputs are
allowed to build, and that is the larger surface: 27 direct third-party dependencies, several
hundred transitively, and no statement anywhere about what this project may ship, what it may be
built from, or what happens when one of them turns out to be vulnerable.

Concretely, before this change:

- nothing checked advisories. A `RUSTSEC` advisory against a crate compiled into `ono` would have
  been discovered by a user;
- nothing checked licences. The binary is distributed under MIT (`LICENSE`), and no rule said
  which dependency licences are compatible with distributing it that way;
- nothing checked sources. A dependency could have arrived from a git branch, or a registry
  nobody chose, and the diff would look like `foo = "1"`;
- six of those 27 dependencies are cryptographic — `ed25519-dalek`, `sha2`, `rustls`,
  `tokio-rustls`, `rustls-pemfile`, `rcgen`. Spec §45.4 requires an explicit review before such a
  crate is introduced. Each of them *was* reviewed, in ADR-0129, ADR-0311 and ADR-0353, and
  nothing connected the review to the dependency. A seventh would have arrived unreviewed and
  looked identical to the six.

Spec §45.2 names `cargo-deny`; §62.3 requires a fixture proving the policy command actually fails
on a denied condition, which is the part that separates a policy from a decoration.

## Decision

**`deny.toml` states the policy, `cargo deny` enforces it in the gate, and `xtask spec-check`
enforces the things `cargo deny` cannot say about itself.**

### The policy runs in the gate, not only in CI

`scripts/gate.sh` gained a `supply chain` step running `cargo deny --locked --all-features check`
— all four checks — and `.github/workflows/ci.yml` installs `cargo-deny@0.20.2` so CI runs the
same gate. If `cargo-deny` is missing the gate stops with the install command; it does not skip.

The gate rather than CI alone, because a dependency is chosen on a developer machine. A policy
that only speaks after a push tells you about a decision you already made, and `cargo deny` costs
seconds beside a gate that already fuzzes and builds the workspace.

`.github/workflows/audit.yml` runs the same command daily on a schedule. This is not redundancy:
the gate answers "did this change bring in something we refuse", and only a clock can answer "did
the world publish an advisory against what was already here". Nothing pushes on the day an
advisory lands.

### One policy tool, one waiver list

`cargo-audit` reads the same RustSec database as `cargo deny check advisories`, and it reads a
different configuration file to learn what has been waived. Running both would mean maintaining
the waiver in `deny.toml` and again in `audit.toml`, and two lists that must agree are a way to
end up with neither. §45.1 asks for *a* maintained advisory source and §45.2 for `cargo-deny`
*or an equivalent*; one tool satisfies both, and the second tool's only unique feature —
auditing a compiled binary's embedded dependency list — needs `cargo auditable`, which this
project does not use.

### A waiver names the day it dies

Spec §45.1 permits a known vulnerability past the gate only against a note saying why the
vulnerable path is unreachable *and* a removal deadline. cargo-deny's own waiver carries an `id`
and a `reason` and nothing else, so the deadline lives inside the reason in a form the scan
reads, and the scan — not a reviewer — enforces it:

```toml
ignore = [{ id = "RUSTSEC-0000-0000", reason = "why the path is unreachable, expires 2027-03-01" }]
```

A waiver with no `expires` fails the gate. A waiver whose date has passed fails the gate on the
day it passes. That makes the gate depend on the calendar, deliberately: a deadline nothing
enforces is a sentence, and this is the one place where a check that goes red without anybody
touching the repository is the correct behaviour.

The policy found one thing on its first run, and it is waived under exactly that rule:
`rustls-pemfile` was archived in August 2025 (RUSTSEC-2025-0134). It is unmaintained rather than
vulnerable, `ono-remote` uses it only to read the local certificate and key files that are the
host's own identity, and the migration to `rustls_pki_types::pem::PemObject` belongs to the owner
of `crates/ono-remote`. The waiver expires 2027-03-01 whether or not that has happened.

### A git dependency and a cryptographic dependency are decisions, and decisions get written down

Both rules of §45.3 and §45.4 are about something somebody decided once and that is invisible in
the manifest afterwards — `rustls = "0.23"` looks exactly like `regex = "1"`. So the decision
lives beside the dependency, in the one table cargo hands to other tools and ignores itself:

```toml
[[workspace.metadata.supply-chain.cryptographic]]
crate = "rustls"
role = "the TLS transport that carries the direct remote protocol, with the ring provider"
adr = "ADR-0353"
reviewed = "2026-08-29"
```

`check_dependency_justifications` reads it in both directions. A git dependency must pin a 40-hex
`rev` and carry an entry with a `reason` and an `adr`; a cryptographic dependency must carry an
entry with a `role`, an `adr` and a `reviewed` date. An entry naming a crate the workspace no
longer depends on fails too — a register that keeps its dead entries stops being a statement
about what is here now. A crate writing `rustls.workspace = true` inherits the decision recorded
at the root and needs nothing of its own; a crate naming its own version is making the decision
itself and is held to it.

What counts as cryptographic is a name list plus a fragment list (`tls`, `crypto`, `ed25519`,
`pkcs`, `x509`, `signature`, …), and it is deliberately over-inclusive. A false positive costs
one line in the register. A false negative is a crate that handles keys and that nobody decided
to trust.

### The policy is proven to fail

Spec §62.3 asks for a fixture, and the two that exist run the real `cargo deny` against a
condition arranged from outside it:

- `should_fail_the_dependency_policy_on_a_denied_license_fixture` builds a throwaway workspace
  whose path dependency is GPL-3.0 under an MIT-only allow list, and asserts the command fails
  and names the licence;
- `should_fail_the_dependency_policy_on_a_denied_advisory_fixture` seeds a private advisory
  database with one invented advisory against a crate this workspace really depends on — read
  out of `Cargo.lock`, so it stays true as the dependencies change — and asserts the advisory
  check fails and names it.

The advisory fixture uses this workspace rather than a throwaway one because RustSec advisories
match crates that came from a registry, and a fixture built out of path crates can never trigger
one; a synthetic registry would be a larger fiction than the seeded advisory is. It finds where
cargo-deny keeps a database by asking it — the first run fails because the database is absent and
says which directory it looked in — rather than reproducing cargo-deny's URL-to-directory naming.

## Consequences

Easy: the seventh cryptographic dependency cannot arrive quietly, an unlicensed or
strangely-licensed crate cannot arrive at all, and an advisory against something already here
turns the tree red the next morning rather than at the next release.

Hard, and accepted: **the gate now needs `cargo-deny` installed**, exactly as it already needs
`cargo-deb` and `cargo-generate-rpm` (ADR-0121). The failure mode is a message with the install
command, not a skip — a policy that quietly does not run when the tool is missing is worse than
no policy, because it reports green.

Also hard: `cargo deny check advisories` fetches the RustSec database, so a gate run on a machine
with no network fails at that step. That is the trade §45.1 asks for by putting the advisory
check in the gate. The alternative — degrade to the offline checks when the database cannot be
reached — is the skip-as-pass defect of §38.3 wearing a different hat.

Left to the neighbours: `deny.toml` says nothing about which *versions* of the tools that build a
release are used (ADR-0450), and nothing about whether two builds of one commit are identical
(H11). The register says nothing about transitive cryptographic dependencies — `ring` arrives
under `rustls`, and the decision recorded is the one about `rustls`.

Encoded by, in `xtask/tests/supply_chain.rs`:

- the policy — `should_reject_a_repository_with_no_dependency_policy_at_all`,
  `should_reject_a_dependency_policy_that_leaves_one_of_the_four_checks_unconfigured`,
  `should_reject_a_dependency_policy_that_allows_no_licence_at_all`,
  `should_reject_an_ignored_advisory_that_names_no_removal_deadline`,
  `should_accept_an_ignored_advisory_carrying_a_reason_and_a_removal_deadline`,
  `should_reject_an_ignored_advisory_whose_removal_deadline_has_passed`,
  `should_reject_a_dependency_policy_that_nothing_in_the_gate_runs`,
  `should_report_this_repository_as_running_its_dependency_policy_in_the_gate`;
- the justifications — `should_fail_the_dependency_policy_on_an_unjustified_git_dependency`,
  `should_reject_a_git_dependency_that_follows_a_branch_instead_of_a_revision`,
  `should_accept_a_git_dependency_pinned_to_a_revision_and_written_down`,
  `should_reject_a_cryptographic_dependency_nobody_recorded_a_review_for`,
  `should_reject_a_cryptographic_dependency_a_single_crate_pulls_in_on_its_own`,
  `should_accept_a_cryptographic_dependency_whose_review_is_recorded`,
  `should_ignore_a_crate_that_only_inherits_a_dependency_the_workspace_already_recorded`,
  `should_report_a_recorded_justification_for_a_dependency_the_workspace_no_longer_has`,
  `should_report_this_repository_as_justifying_every_git_and_cryptographic_dependency`;
- the policy failing for real — `should_fail_the_dependency_policy_on_a_denied_license_fixture`,
  `should_fail_the_dependency_policy_on_a_denied_advisory_fixture`.

## Alternatives considered

**Run the policy only in CI.** Cheaper for the gate, and it moves every finding to after the
push. The dependency decision is made where `Cargo.toml` is edited, and §45.1 says "the gate".

**Keep the justification register in a separate file.** A `supply-chain.toml` beside `deny.toml`
would be one more file to remember. `workspace.metadata` is the extension point cargo documents
for exactly this, it sits three lines below the dependency it justifies, and a dependency added
without its entry is a diff where the omission is visible.

**Detect cryptographic dependencies from what the code imports rather than from crate names.**
More precise and much larger: it would need the resolved graph and a list of the modules that
matter. The name list is crude and its errors point the safe way — the register grows a line it
did not strictly need.

**Let `cargo deny`'s `[bans]` express the crypto rule.** `bans` refuses crates; there is no
"allow only with a recorded reason". The rule is about a record existing, which is a question
about this repository rather than about the graph.

**Waive nothing and migrate `ono-remote` off `rustls-pemfile` now.** That crate belongs to
another agent's increment, and rewriting someone else's certificate loading inside a
supply-chain change is exactly the mixing AGENTS.md §4 forbids. The waiver has a date; the
migration has an owner.
