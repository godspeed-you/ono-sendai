# ONO-SENDAI CNCF Readiness

## Community, Governance and Ecosystem Readiness Plan

**Status:** Living project and ecosystem document  
**Scope:** Ono-Sendai core project and the repositories governed as part of the Ono-Sendai project  
**Primary target:** CNCF Sandbox readiness, with Incubation as the later maturity target  
**Relationship:** Complements the Cloud-Native Vision and External System Provider architecture. It does not replace release specifications or provider-specific specifications.  
**Last requirements review:** 2026-09-05

> CNCF is an ecosystem goal, not a feature.

> Ono-Sendai should enter CNCF because it has become useful to the cloud-native ecosystem, not become cloud-native because it wants to enter CNCF.

---

# 0. Purpose and Authority

## 0.1 Purpose

This document defines the readiness path by which Ono-Sendai can evolve from a founder-built open-source project into a credible, sustainable, vendor-neutral community project suitable for CNCF Sandbox application and, later, Incubation.

It exists to prevent three failure modes:

1. **CNCF-driven feature development.** CNCF participation must not distort the product into a superficial Kubernetes CLI or a checklist-driven cloud project.
2. **Premature institutionalization.** Governance, process and organizational machinery should be introduced when they solve real project problems, not because they look mature.
3. **Founder dependency disguised as community.** A project with users but only one person who can review, release, triage and make architecture decisions is not sustainable merely because it has a public issue tracker.

The Cloud-Native Vision defines *why* cloud-native infrastructure is strategically interesting. The External System Provider specification defines the generic technical boundary. Provider-specific specifications define implementation behavior. This document defines the project, community and governance conditions under which CNCF participation becomes appropriate.

## 0.2 This is not a release specification

Nothing in this document is assigned to a numbered Ono-Sendai release.

A release may improve CNCF readiness, but CNCF readiness is not itself a product feature and MUST NOT force arbitrary release scope.

## 0.3 This is not a CNCF application form

CNCF requirements change. This document tracks the project's readiness intent and records the current upstream expectations as of the review date above.

Immediately before any Sandbox or maturity-level application, the current CNCF application, lifecycle, onboarding, governance, security and IP requirements MUST be re-read and this document updated if necessary.

## 0.4 Readiness is evidence, not aspiration

A box is complete only when there is inspectable evidence.

Acceptable evidence includes repository files, merged pull requests, release artifacts, CI results, public governance records, maintainer history, adopter references, public issues and discussions, security documentation, review reports and CNCF/TAG meeting records.

Statements such as "we intend to", "we will add", or "the project is designed for" are not completion evidence.

---

# 1. Strategic Thesis

## 1.1 CNCF follows technical proof

The intended order is:

```text
Ono core architecture
        |
        v
generic external-system provider contract
        |
        v
Kubernetes provider PoC
        |
        v
Cloud-Native Validation Gate
        |
        +-----------------------+
        |                       |
      FAIL                    PASS
        |                       |
        v                       v
rethink abstraction      cloud-native strategy confirmed
                                |
                                v
                        Apache-2.0 transition
                                |
                                v
                    community/governance maturation
                                |
                                v
                       CNCF Sandbox readiness
```

The project MUST NOT invert this order.

The goal is not:

```text
"We want CNCF"
      |
      v
"therefore add Kubernetes"
```

The goal is:

```text
"Ono's systems model proves unusually useful for Kubernetes
and adjacent infrastructure"
      |
      v
"therefore Ono has a credible cloud-native role"
      |
      v
"CNCF becomes a natural community home"
```

## 1.2 Kubernetes is the reference proof, not the product boundary

Kubernetes is the first reference external-system provider because it is structured, relational, dynamic, extensible and operationally important.

The Kubernetes provider lives in its **own repository**, [ono-sendai-kubernetes](https://github.com/godspeed-you/ono-sendai-kubernetes), created 2026-09-05.

That repository separation is intentional:

- Kubernetes domain logic MUST NOT accumulate inside Ono core;
- Kubernetes contributors should be able to work without understanding the entire shell implementation;
- provider release cadence can differ from core cadence;
- provider ownership can grow independently;
- the generic KUANG/11 external-system contract remains testable as a real extension boundary.

The dedicated repository does **not** imply that the Kubernetes provider must become a separate CNCF project. Whether CNCF eventually treats provider repositories as subprojects or simply additional repositories of the Ono-Sendai project is a later governance decision.

The main Ono-Sendai repository remains authoritative for:

- shell language and pipeline semantics;
- generic systems model;
- KUANG/11 host/runtime contracts;
- generic external-system provider architecture;
- cross-provider policies;
- project-wide governance;
- project strategy;
- CNCF readiness.

The Kubernetes provider repository is authoritative for:

- Kubernetes API integration;
- Kubernetes resource mapping;
- Kubernetes-local relationships;
- CRD handling;
- Kubernetes watch/cache behavior;
- Kubernetes-specific compatibility policy;
- Kubernetes-specific tests and fixtures;
- the Kubernetes Provider Specification.

The full Kubernetes Provider Specification MUST NOT be duplicated as a second canonical copy in the core repository.

---

# 2. Cloud-Native Validation Gate

## 2.1 Purpose

The Kubernetes PoC is not merely a feature demonstration. It is the architectural experiment that determines whether the cloud-native strategy is earned.

The PoC passes only if it demonstrates that Ono's existing concepts become *more useful* when applied to Kubernetes without creating a Kubernetes-specific second shell.

## 2.2 Minimum validation evidence

The Cloud-Native Validation Gate SHOULD require, at minimum:

- direct Kubernetes API interaction; no dependency on `kubectl` for core conformance;
- discovery of real cluster resources;
- stable Kubernetes identity preservation, including UID-aware resource lifetime;
- typed or schema-aware representation of resources;
- useful behavior for resources unknown when Ono was compiled, including CRDs;
- resource relationships with inspectable evidence;
- navigation through the existing Ono spatial model;
- at least one useful relationship path across several Kubernetes resource kinds;
- read-only operation that is already valuable without mutation;
- honest handling of RBAC denial, incomplete scope, stale state and watch discontinuity;
- no Kubernetes-specific parser or shell grammar;
- no Kubernetes-specific privileged exception in Ono core;
- no requirement to flatten all Kubernetes resources into raw JSON or YAML;
- cancellation and resource limits compatible with the generic provider contract;
- deterministic tests sufficient to prove the architecture without requiring permanent access to a live production cluster.

The provider-specific specification may define stronger gates. Passing this strategic gate does not mean the full Kubernetes provider is complete.

## 2.3 Success criterion

The PoC succeeds when a technically informed user can reasonably conclude:

> Ono is not wrapping kubectl. Ono is preserving Kubernetes as a system of typed resources, identities, relationships, observations and scopes inside the same systems interface used for local and remote infrastructure.

## 2.4 Failure is allowed

A failed PoC is useful evidence.

If Kubernetes requires extensive core exceptions, a second grammar, pervasive provider-specific types in core, unreliable relationship inference, or architecture that cannot plausibly generalize to a second provider, the project SHOULD revise the provider abstraction before declaring the cloud-native strategy successful.

CNCF work MUST NOT proceed merely to preserve a previously announced direction.

---

# 3. License Decision Gate

## 3.1 Current intent

Ono-Sendai began under the MIT license because it is intended to be freely usable and is not designed as a proprietary commercialization vehicle.

The intended CNCF-aligned long-term license is Apache License 2.0.

This change is about project and contributor legal structure, including explicit patent terms and CNCF alignment. It does not represent a change toward monetization.

## 3.2 Primary trigger

The normal trigger for the MIT -> Apache-2.0 transition is:

> **The Kubernetes PoC passes the Cloud-Native Validation Gate and the project formally confirms the cloud-native strategy.**

The license transition SHOULD occur at that strategic boundary, before broad external provider contribution begins and well before a CNCF application.

## 3.3 Early trigger

The license transition SHOULD happen earlier if substantive external contributions begin before the Kubernetes PoC is complete.

"Substantive" means contributions that create meaningful shared authorship or long-term maintenance expectations, such as:

- significant production code;
- major architecture changes;
- non-trivial KUANG/11/provider implementation;
- large test suites tied to new functionality;
- ongoing contribution by an emerging maintainer.

Trivial typo fixes, documentation spelling corrections or similarly minor contributions need not trigger an immediate transition by themselves.

The purpose of the early trigger is to avoid unnecessary licensing complexity once a real contributor community begins to form.

## 3.4 The Kubernetes provider repository is already Apache-2.0

The `ono-sendai-kubernetes` repository was created on 2026-09-05 under Apache License 2.0. It
carries no code inherited from the MIT-licensed core, so this is an initial licence choice rather
than a relicensing, and it does not execute or pre-empt the core transition described above.

The consequence to keep visible: the project currently spans two licences. Core is MIT, the
Kubernetes provider repository is Apache-2.0, and any statement about "the project's licence"
must say which repository it means until the core transition is executed.

## 3.5 Application hard gate

Regardless of the historical trigger:

- the project MUST verify current CNCF license requirements immediately before application;
- the project MUST NOT apply with a promise to fix an incompatible license later;
- Ono-Sendai's target state before Sandbox application is Apache-2.0 for the project code entering CNCF;
- dependency licenses MUST be reviewed against current CNCF third-party license policy;
- required existing copyright/license notices MUST be preserved;
- the transition MUST NOT pretend that historical MIT grants have been revoked.

## 3.6 Repository work for the transition

When the transition is executed:

- [ ] replace the project license with the intended Apache-2.0 project licensing state;
- [ ] preserve notices required by previously incorporated code;
- [ ] add or update SPDX metadata where the project uses it;
- [ ] review `Cargo.toml`, package metadata, generated artifacts and documentation;
- [ ] review dependency license policy;
- [ ] update README license text;
- [ ] update contribution documentation;
- [ ] update provider repository templates;
- [ ] record the decision in an ADR;
- [ ] document the effective project version/commit boundary;
- [ ] verify that release packaging contains the correct licensing material.

A `NOTICE` file SHOULD be added only when required by the licensing/provenance state; it must not be created merely as Apache-2.0 decoration.

---

# 4. Repository and Project Topology

## 4.1 Core repository

The core repository SHOULD contain:

```text
ono-sendai/
  README.md
  PHILOSOPHY.md
  CONTRIBUTING.md
  SECURITY.md
  GOVERNANCE.md
  MAINTAINERS.md
  CODE_OF_CONDUCT.md
  LICENSE
  docs/
    strategy/
      cloud-native-vision.md
      cncf-readiness.md
    architecture/
      external-system-provider.md
    specs/
      ...
    adr/
      ...
    contracts/
      ...
```

The exact supporting directories may evolve, but strategy, architecture, immutable release specifications, ADRs and machine-readable contracts SHOULD remain distinguishable.

## 4.2 Kubernetes provider repository

The Kubernetes provider has its own repository.

Recommended conceptual structure:

```text
ono-sendai-kubernetes/
  README.md
  CONTRIBUTING.md
  SECURITY.md
  LICENSE
  docs/
    architecture/
      kubernetes-provider.md
  src/ or crates/
  tests/
  fixtures/
```

Project-wide governance MAY initially be inherited by reference from Ono-Sendai rather than duplicated. Repository-specific maintainer/domain ownership can be added as the provider community develops.

The provider repository MUST clearly identify:

- compatibility with specific Ono/KUANG/11 provider contract versions;
- its release/version policy;
- supported Kubernetes versions;
- ownership and maintainers;
- security reporting path;
- relationship to the Ono-Sendai project.

## 4.3 Future providers

AWS, Azure, GCP and other substantial providers SHOULD be evaluated for separate repositories using the same principles.

A new repository is justified when it improves independent ownership, release cadence, dependency isolation, test infrastructure or contributor accessibility.

Repository proliferation is not itself a goal.

## 4.4 Project metadata

Before CNCF onboarding, all repositories intended to enter the project scope MUST be inventoried.

The project MUST be able to answer:

- Which repositories are part of Ono-Sendai?
- Which are canonical?
- Which are generated?
- Which are experimental?
- Who maintains each?
- Which governance applies?
- Which security reporting path applies?
- Which license applies?
- Which repositories would be contributed to CNCF?

---

# 5. Community Readiness

## 5.1 Community before application

Sandbox is an early maturity level, but Ono SHOULD NOT use Sandbox as a substitute for proving that a community can form.

The internal target before application is stronger than the minimum eligibility checklist.

Recommended internal target:

- at least 3 active maintainers;
- maintainers from at least 2 independent employer/organizational contexts where realistically possible;
- external contributors with merged non-trivial work;
- evidence that at least one subsystem can progress without founder implementation;
- public issue and contribution workflow;
- at least one public community communication channel if community volume justifies it.

This is an **Ono-Sendai quality target**, not a claim that CNCF currently mandates this exact maintainer count for Sandbox.

## 5.2 Contributor ladder

Governance SHOULD eventually distinguish at least:

```text
user
  -> contributor
  -> reviewer/domain owner
  -> maintainer
  -> project leadership role, if needed
```

Promotion MUST be based on demonstrated contribution and trust, not sponsorship, employment, friendship or founder preference.

The governance model SHOULD remain lightweight while the community is small.

## 5.3 Bounded ownership domains

Ono should deliberately create areas that can be owned without full-core expertise.

Examples include the Kubernetes provider, provider conformance, CRD/schema handling, cloud-provider domains, cross-system relationship resolvers, release engineering, documentation, test fixtures, security and KUANG/11 SDK/tooling.

Maintainer growth is more credible when ownership is real rather than honorary.

## 5.4 Founder role

The founder has historical authorship and may remain a maintainer or technical/project leader.

Founder status MUST NOT become a permanent governance override.

As the project matures:

- decisions SHOULD increasingly follow documented governance;
- maintainership MUST be earned and removable by transparent rules;
- the founder SHOULD be able to lose a vote without the project becoming illegitimate;
- architecture principles SHOULD live in project documents rather than private founder knowledge.

## 5.5 No entitlement to maintainer time

Open-source rights to the software do not create an SLA against volunteer maintainers.

The project SHOULD document realistic expectations:

- no guaranteed issue response time unless a funded project policy explicitly creates one;
- no guaranteed feature implementation;
- no guarantee that every PR will be merged or rapidly reviewed;
- security reports receive a defined process, but not promises the team cannot sustain;
- user urgency does not automatically become maintainer urgency.

---

# 6. Maintainer Sustainability

## 6.1 Sustainability is a readiness property

A project that becomes more successful by consuming progressively more unpaid founder time has a negative scaling model.

The desired relationship is:

```text
more users
   |
   v
more relevance
   |
   +------------------+
   |                  |
   v                  v
contributors        funding
   |                  |
   +--------+---------+
            |
            v
     maintainer capacity
```

not:

```text
more users
   |
   v
more issues
   |
   v
more unpaid founder work
```

## 6.2 Funding is not product monetization

Ono-Sendai may remain fully open-source, vendor-neutral and non-commercial in product strategy while still funding the human work required to maintain it.

Possible future funding mechanisms include individual sponsorship, corporate sponsorship, grants, public open-source funding programs, funded maintainer fellowships, employer-funded upstream contribution and project-budget reimbursement for specific maintenance work.

Funding MUST NOT buy:

- votes;
- maintainer status;
- roadmap priority;
- architectural exceptions;
- private feature queues;
- privileged access to security information;
- project ownership.

## 6.3 Maintainer capacity should be explicit

As community demand grows, the project SHOULD track capacity rather than implicitly absorb all incoming work.

A healthy project can truthfully say:

```text
available maintainer capacity: 8 h/week
open issues: 170
```

and leave low-priority work waiting.

The number of open issues is not by itself a maintainer failure.

## 6.4 Sustainability evidence before higher maturity

Before Incubation is pursued, the project SHOULD demonstrate that:

- release responsibility is not held by only one individual;
- issue triage can continue during founder absence;
- at least one other maintainer can review meaningful changes;
- credentials and infrastructure are not founder-personal single points of failure;
- governance supports maintainer onboarding and offboarding;
- project workload is compatible with actual available human capacity.

---

# 7. Governance Readiness

## 7.1 Governance should solve current problems

Do not create a large steering committee for a three-person project.

Initial governance can be compact, but it MUST be explicit enough to answer:

- Who is a maintainer?
- How does someone become one?
- How can someone become inactive or emeritus?
- Who can merge?
- Who can release?
- How are architecture changes approved?
- How are governance changes approved?
- How are conflicts resolved?
- How are subprojects/provider repositories governed?
- How is vendor neutrality protected?
- How are security roles assigned?

## 7.2 Required project artifacts

Target before Sandbox application:

- [ ] `GOVERNANCE.md`
- [ ] `MAINTAINERS.md`
- [ ] `CODE_OF_CONDUCT.md`
- [x] `CONTRIBUTING.md` exists in the current core repository
- [x] `SECURITY.md` exists in the current core repository
- [ ] public roadmap/direction is clearly discoverable
- [ ] all project repositories and subprojects are enumerated
- [ ] maintainer affiliation is documented
- [ ] contributor/maintainer lifecycle is documented
- [ ] release authority/process is documented
- [ ] public communication channels are documented when they exist

Existing files still need to be reviewed against current CNCF expectations; existence alone is not conformance.

## 7.3 MAINTAINERS data

Before Sandbox application, `MAINTAINERS.md` MUST satisfy the then-current application format.

As of the 2026-09-05 review, the Sandbox application expects a direct link to a maintainer file containing at least:

- Name;
- GitHub ID;
- Company/Organization.

Ono SHOULD add domain/responsibility information where useful even if the application does not require it.

## 7.4 Vendor neutrality

Vendor neutrality means more than "no company owns the repo".

The project SHOULD ensure:

- project direction is not controlled by sponsorship;
- employer affiliation does not create extra votes;
- provider vendors do not receive privileged integration status;
- comparable providers can implement the same public contracts;
- governance cannot be captured trivially by one employer as the project grows;
- project marks and infrastructure can be transferred into neutral foundation custody if accepted.

## 7.5 Governance evolution

Governance SHOULD evolve in stages.

### Small project

- maintainers decide by documented consensus;
- architecture changes require ADRs;
- founder has no undocumented veto;
- maintainer addition/removal is explicit.

### Growing project

- defined voting/fallback process;
- domain ownership;
- emeritus status;
- conflict-of-interest rules;
- repository/subproject ownership;
- security response roles.

### Mature project

If project size justifies it:

- maintainer council or steering structure;
- mechanisms to preserve organizational diversity;
- explicit election/term rules where useful;
- documented succession.

The structure SHOULD be proportional to actual community size.

---

# 8. Security and Supply-Chain Readiness

## 8.1 Security is unusually important for Ono

Ono is a shell, executes external commands, links to remote systems and hosts an extension system.

Cloud-native providers may receive access to clusters, cloud APIs, credentials, production infrastructure, secrets and remote hosts.

Security readiness is therefore a core project property, not a late CNCF checkbox.

## 8.2 Security checklist

Before Sandbox application:

- [ ] `SECURITY.md` has been reviewed against current project behavior;
- [ ] private vulnerability reporting path is documented and tested;
- [ ] supported security versions are explicit;
- [ ] security response roles are assigned;
- [ ] repository access policy is documented;
- [ ] maintainer 2FA expectations are enforced where platform support permits;
- [ ] dependency auditing is automated;
- [ ] secret scanning is enabled where available;
- [ ] CI permissions follow least privilege;
- [ ] release provenance/signing behavior is demonstrably working;
- [ ] provider capability boundaries have conformance tests;
- [ ] dependency licensing can be scanned with CNCF-compatible tooling;
- [ ] OpenSSF Best Practices work has begun before onboarding and reaches the required target as maturity increases.

## 8.3 Security self-assessment

A CNCF-style security self-assessment SHOULD be drafted before Incubation preparation, not for the first time during due diligence.

It should cover at least threat model, trust boundaries, credentials, remote execution, plugin execution, update/release path, privilege behavior, data persistence, supply-chain dependencies, vulnerability response and security-sensitive defaults.

---

# 9. Release and Engineering Maturity

## 9.1 Evidence should build on Ono's existing discipline

Ono already follows a specification-first, acceptance-driven engineering model.

CNCF readiness SHOULD use that strength rather than layering a second process beside it.

Readiness evidence should come from deterministic tests, acceptance gates, architecture decision records, machine-readable contracts, release verification, supply-chain provenance, compatibility matrices and explicit unsupported/unknown semantics.

## 9.2 Sandbox readiness checks

Before application:

- [ ] repository has at least the minimum active-development age required by the current Sandbox application;
- [ ] development activity is continuous enough to demonstrate an active project;
- [ ] installation/build instructions work from a clean environment;
- [ ] a new contributor can execute the development/test workflow;
- [ ] supported platforms are documented;
- [ ] release process is documented;
- [ ] release artifacts are reproducibly identifiable;
- [ ] compatibility policy is explicit;
- [ ] breaking-change posture is clear for project maturity;
- [ ] Kubernetes provider PoC has passed the Cloud-Native Validation Gate;
- [ ] generic provider contracts have at least one real external-system implementation.

As of 2026-09-05, the CNCF Sandbox application lists **6+ months old with active development** as a critical application requirement. Re-check before applying.

## 9.3 Second-provider proof

Kubernetes proves that the provider architecture can handle a complex dynamic system.

A second materially different provider — likely AWS — SHOULD later prove that the generic contract is not accidentally Kubernetes-shaped.

This is not required to start community work, but it is a strong architecture milestone before claiming a mature multi-system model.

---

# 10. Documentation Readiness

## 10.1 Documentation classes

The repository SHOULD clearly distinguish:

- **User documentation:** how to install and use Ono.
- **Strategy:** why the project is going in a direction.
- **Architecture:** stable system boundaries and contracts.
- **Release specifications:** immutable specification inputs for numbered releases.
- **ADRs:** decisions and justified deviations.
- **Machine-readable contracts:** schemas, verbs, capabilities and generated/reference inputs.
- **Project/community documents:** contributing, governance, maintainers, security, code of conduct.

## 10.2 Canonical-source rule

Each document MUST have one canonical source.

For example:

- `cloud-native-vision.md` is canonical in core;
- `external-system-provider.md` is canonical in core;
- `kubernetes-provider.md` is canonical in the Kubernetes provider repository;
- PDFs, if produced, are generated representations and MUST NOT become independent editable sources.

## 10.3 Cross-repository links

Cross-repository documents MUST avoid invented or unstable links.

If the Kubernetes provider repository does not yet exist, core documentation should state that the provider is maintained in a dedicated repository once established rather than committing a fake URL.

When the repository exists, links SHOULD be updated in one atomic documentation change.

---

# 11. CNCF Sandbox Readiness Gate

## 11.1 Philosophy

The project SHOULD apply when CNCF can accelerate a community that already shows signs of existing.

Do not apply merely because the checklist can technically be satisfied.

## 11.2 Internal Sandbox gate

Before submitting an application, all **hard** items below should be complete and all **health** items should have credible evidence.

### A. Technical fit — hard

- [ ] Cloud-Native Validation Gate passed.
- [ ] Ono's cloud-native role is demonstrable without marketing-only claims.
- [ ] Kubernetes support is through the generic external-provider boundary.
- [ ] project is reusable software, not a reference architecture.
- [ ] overlap/differentiation with adjacent shells and cloud-native tools is documented.
- [ ] architecture can plausibly support another provider without becoming Kubernetes-specific.

### B. License/IP — hard

- [ ] current CNCF license rules reviewed.
- [ ] Apache-2.0 target state complete before application.
- [ ] dependency licenses reviewed.
- [ ] project copyright/provenance understood.
- [ ] trademark/logo/domain ownership inventoried.
- [ ] maintainers understand that CNCF onboarding can require IP/trademark transfer and neutral hosting.

### C. Repository age/activity — hard

- [ ] current CNCF minimum repository age met.
- [ ] active development demonstrated.
- [ ] public development history is coherent.

### D. Community — health

- [ ] meaningful external contributions exist.
- [ ] at least 3 active maintainers is the preferred Ono internal target.
- [ ] maintainer affiliations are visible.
- [ ] independent organizational diversity is emerging.
- [ ] at least one non-founder maintainer owns meaningful project work.
- [ ] contribution workflow has been exercised by real contributors.

### E. Governance — hard/health

- [ ] `GOVERNANCE.md`.
- [ ] `MAINTAINERS.md` in the current CNCF-required format.
- [ ] `CODE_OF_CONDUCT.md`.
- [ ] `CONTRIBUTING.md` reviewed.
- [ ] `SECURITY.md` reviewed.
- [ ] maintainer lifecycle.
- [ ] decision process.
- [ ] release authority.
- [ ] subproject/repository scope.
- [ ] vendor-neutrality statement.

### F. Security — hard/health

- [ ] vulnerability reporting path.
- [ ] security response ownership.
- [ ] repository access controls.
- [ ] dependency/security scanning.
- [ ] release integrity path proven.
- [ ] extension/provider security boundaries documented.
- [ ] OpenSSF Best Practices work started/appropriate target met.

### G. Adoption — health

- [ ] real users outside the founder's own environment.
- [ ] adopter evidence can be documented truthfully.
- [ ] at least some users exercise the cloud-native functionality that motivates CNCF participation.
- [ ] adopters are not merely GitHub stars/downloads.
- [ ] public adopter list can be created when users consent.

### H. Public project operation — health

- [ ] roadmap/direction is public.
- [ ] issue tracker is actively triaged within available capacity.
- [ ] project status is honest.
- [ ] no private roadmap is required to understand project direction.
- [ ] public communication channel exists if community volume justifies it.
- [ ] maintainer workload is sustainable.

### I. CNCF engagement — preparation

- [ ] relevant CNCF TAG(s) identified.
- [ ] project has reviewed current General Technical Review questions.
- [ ] presentation/Day-0 technical review material can be produced.
- [ ] current Sandbox application reviewed line by line.
- [ ] no application answer depends on a promise to fix a critical criterion later.

## 11.3 Go/no-go decision

A Sandbox application SHOULD require an explicit public maintainer decision.

The decision should record:

- why now;
- what CNCF is expected to improve;
- which project assets/repositories are in scope;
- known readiness gaps;
- maintainer consent to neutral governance and onboarding obligations.

---

# 12. CNCF Onboarding Readiness

Acceptance is not the end of readiness work.

Current CNCF onboarding expectations include legal, infrastructure and governance work.

Before applying, maintainers SHOULD already understand and accept the likely consequences.

## 12.1 Expected onboarding work

Based on the 2026-09-05 upstream review, be prepared for work including:

- Project Contribution Agreement;
- Apache-2.0/inbound licensing compliance;
- CNCF third-party license policy;
- trademark/logo transfer where applicable;
- neutral GitHub organization;
- CNCF/Linux Foundation organization ownership/hosting arrangements;
- maintainer metadata;
- DCO enablement;
- Code of Conduct linkage;
- written open governance;
- security policy;
- OpenSSF Best Practices;
- CNCF-supported license/security scanning;
- project metadata/landscape integration;
- migration of community channels if applicable.

The exact list MUST be revalidated at application/onboarding time.

## 12.2 Neutral organization

Moving to a neutral GitHub organization is a material project change.

Before that move:

- repository automation MUST be inventoried;
- secrets and environments MUST be inventoried;
- release identities and Sigstore/OIDC rules MUST be checked;
- package references MUST be checked;
- documentation links MUST be checked;
- provider repositories MUST be included in the migration plan where appropriate;
- third-party integrations MUST be tested after migration.

No migration should be treated as a simple repository rename.

---

# 13. Incubation Direction

## 13.1 Sandbox is not the destination

Sandbox is useful if it increases community participation, technical feedback, contributor discovery, neutral governance and ecosystem interoperability.

The meaningful long-term maturity signal is Incubation.

## 13.2 Incubation evidence

Current CNCF due-diligence material indicates that Incubation requires substantially stronger evidence than Sandbox, including:

- completed technical review;
- completed governance review;
- vendor-neutral project metadata/resources;
- due diligence;
- documented security process and security self-assessment;
- OpenSSF Best Practices passing badge;
- documented adopters;
- real independent adopters;
- adopter interviews;
- integration/compatibility documentation;
- mature governance and maintainer ownership.

Current TOC process asks projects moving levels to provide **5–7 adopters willing to be interviewed**. Current incubation due-diligence guidance also requires demonstrated independent adoption.

These numbers/processes MUST be rechecked when Ono is actually approaching Incubation.

## 13.3 Ono's internal Incubation bar

Before seeking Incubation, Ono SHOULD additionally demonstrate:

- at least one significant provider maintained substantially by non-founder contributors;
- founder absence does not stop normal releases/triage;
- Kubernetes provider has real-world usage;
- another external-system provider has validated the generic architecture;
- cross-system relationships work in real environments;
- project governance has been exercised rather than merely written;
- maintainer onboarding/offboarding has real evidence;
- security processes have been exercised;
- maintenance funding/capacity is sustainable enough for the adoption level.

---

# 14. Project Sustainability and Funding Guardrails

## 14.1 Ono is not a startup project

The project does not need a commercial product strategy in order to be sustainable.

The project may remain:

- Apache-2.0;
- free to use;
- community governed;
- vendor neutral;
- non-open-core;
- without paid enterprise features.

## 14.2 It is legitimate to fund work

The project MAY pay maintainers for real project work.

Examples include issue triage, code review, release engineering, security work, architecture, documentation, community management and contributor mentoring.

Payment is compensation for work, not ownership.

## 14.3 Funding policy before material funding

If recurring project funding becomes material, add a public funding policy defining:

- how funds are received;
- who controls funds;
- eligible expenses;
- compensation approval;
- conflict-of-interest handling;
- transparency;
- whether maintainers can approve their own compensation;
- sponsor recognition;
- explicit prohibition on purchased roadmap influence.

A fiscal host or foundation mechanism may later be preferable to founder-personal fund custody.

## 14.4 Success metric

A valid sustainability milestone is:

> The project can fund enough maintainer capacity that increasing adoption does not require the founder to donate an ever-growing share of personal time.

That is project sustainability, not commercialization.

---

# 15. Readiness Stages

## R0 — Founder-built / architecture maturation

Characteristics:

- founder drives most work;
- project vision and architecture are still consolidating;
- contributors may be sparse;
- MIT may remain until the license trigger;
- Kubernetes PoC not yet validated.

Exit:

- Kubernetes PoC reaches the Cloud-Native Validation Gate.

## R1 — Cloud-native validated

Characteristics:

- Kubernetes PoC demonstrates the strategy;
- cloud-native direction formally confirmed;
- Apache-2.0 transition executed;
- Cloud-Native Vision and provider architecture are public and canonical;
- Kubernetes provider is separated into its dedicated repository.

Exit:

- external contribution workflow is ready;
- basic community/governance artifacts are in place.

## R2 — Community-ready

Characteristics:

- governance is lightweight but explicit;
- maintainers are documented;
- Code of Conduct exists;
- contribution paths are bounded;
- provider ownership can be delegated;
- real external contributions exist;
- project direction is public.

Exit:

- Sandbox internal readiness gate passes.

## R3 — Sandbox candidate

Characteristics:

- current CNCF application criteria satisfied;
- repository age/activity criterion satisfied;
- license/IP clean;
- maintainer/community evidence credible;
- adopter evidence beginning;
- relevant TAG engagement started;
- maintainers explicitly consent to CNCF implications.

Exit:

- public maintainer go decision and application.

## R4 — CNCF Sandbox

Characteristics:

- onboarding complete;
- neutral hosting/governance operational;
- project uses CNCF community and review mechanisms;
- focus remains on adoption, community and architecture proof.

Exit:

- Incubation evidence is real, not aspirational.

## R5 — Incubation candidate

Characteristics:

- technical and governance reviews complete;
- independent adoption;
- adopter interview pool;
- security self-assessment;
- OpenSSF maturity;
- project survivability;
- multiple meaningful maintainers;
- sustainable project operation.

---

# 16. Current Baseline Snapshot

This section is intentionally conservative. The documentation restructuring described in Phase A has landed; every other row still reports the repository as it is, not as it is planned to be.

Known current state around 2026-09-05:

| Area | State | Notes |
|---|---|---|
| Core repository | active | Ono-Sendai public repository exists and is actively developed |
| License | MIT | Apache-2.0 transition intentionally deferred to decision gate / early-contribution trigger |
| Cloud-Native Vision | canonical | `docs/strategy/cloud-native-vision.md` |
| Generic External System Provider spec | canonical | `docs/architecture/external-system-provider.md` |
| Kubernetes Provider spec | canonical elsewhere | `docs/architecture/kubernetes-provider.md` in [ono-sendai-kubernetes](https://github.com/godspeed-you/ono-sendai-kubernetes); deliberately not copied into core |
| Kubernetes provider implementation | planned/PoC path | must prove cloud-native strategy |
| `CONTRIBUTING.md` | present | review for community/CNCF maturity later |
| `SECURITY.md` | present | review against current threat model and CNCF expectations |
| `GOVERNANCE.md` | not yet baseline | add when community-readiness work begins |
| `MAINTAINERS.md` | not yet baseline | required before Sandbox application |
| `CODE_OF_CONDUCT.md` | not yet baseline | add before community/CNCF readiness |
| ADR discipline | strong | existing ADR corpus should remain canonical |
| Immutable release specs | strong | moved to `docs/specs/` during the restructuring; all nine hashes byte-identical and `docs/specs/spec.sha256` verifies |
| Supply-chain/release integrity | substantial design already present | prove and continuously verify actual release path |
| External maintainers | not assumed | must be demonstrated, never invented |
| Adopters | not assumed | real use must be documented when it exists |
| CNCF application | premature | application follows technical proof and community evidence |

No row may be marked complete merely because a future implementation is planned.

---

# 17. Immediate Next Actions

## Phase A — Documentation normalization

- [x] add `docs/strategy/cloud-native-vision.md`;
- [x] add `docs/strategy/cncf-readiness.md`;
- [x] add `docs/architecture/external-system-provider.md`;
- [x] place Kubernetes Provider Specification only in the dedicated Kubernetes provider repository;
- [x] reorganize immutable release specifications without changing their bytes;
- [x] normalize ADR and machine-readable-contract directory naming — `docs/decisions/` to `docs/adr/`, `docs/spec/` to `docs/contracts/`;
- [x] update all links, scripts, checksums/manifests and generated-doc paths;
- [x] keep README concise while linking project direction and internals.

## Phase B — Kubernetes proof

- [x] establish dedicated Kubernetes provider repository — [ono-sendai-kubernetes](https://github.com/godspeed-you/ono-sendai-kubernetes), 2026-09-05: specification, README, CONTRIBUTING and SECURITY in place; no implementation yet;
- [ ] implement PoC;
- [ ] pass Cloud-Native Validation Gate;
- [ ] document results;
- [ ] formally accept/reject/revise cloud-native strategy.

## Phase C — Strategic transition

If the gate passes:

- [ ] execute MIT -> Apache-2.0 transition;
- [ ] add ADR for transition;
- [ ] publish cloud-native direction prominently but without overclaiming;
- [ ] begin lightweight governance/community readiness.

If substantive external contributions arrive first:

- [ ] perform the license transition earlier.

## Phase D — Community growth

- [ ] create bounded ownership domains;
- [ ] add Code of Conduct;
- [ ] add maintainers file;
- [ ] add lightweight governance;
- [ ] establish contributor ladder;
- [ ] document maintainer capacity expectations;
- [ ] recruit/recognize maintainers based on real work;
- [ ] allow provider repositories to develop independent ownership.

## Phase E — Sandbox preparation

Only after real community/adoption evidence:

- [ ] re-read current CNCF lifecycle and Sandbox application;
- [ ] complete gap analysis;
- [ ] engage relevant TAG(s);
- [ ] review Day-0 General Technical Review questions;
- [ ] inventory marks/domains/repos/infrastructure;
- [ ] confirm neutral-hosting willingness;
- [ ] make public maintainer go/no-go decision;
- [ ] apply.

---

# 18. Readiness Anti-Patterns

The following are explicit failures of this plan:

1. **Badge-driven engineering:** adding Kubernetes/cloud features primarily to make a CNCF application look stronger.
2. **Governance theater:** creating committees and titles that solve no current community problem.
3. **Honorary maintainers:** adding people to create apparent diversity when they do not actually maintain the project.
4. **Adopter inflation:** counting stars, downloads, experiments or the founder's own machines as independent adoption.
5. **Founder bottleneck denial:** calling the project community-maintained while only the founder can release, approve architecture, respond to security issues or review difficult changes.
6. **Paid influence:** allowing sponsorship to purchase project direction, maintainer status or integration privilege.
7. **Kubernetes leakage into core:** moving Kubernetes-specific concepts into Ono core merely because the reference provider needs them.
8. **Canonical-document duplication:** keeping different authoritative copies of strategy/provider specs in multiple repositories.
9. **Application by promise:** submitting a CNCF application with critical requirements deferred until after acceptance.
10. **Unsustainable success:** accepting unlimited user expectations while maintainer capacity remains fixed and unpaid.

---

# 19. Upstream CNCF References

These references were reviewed on 2026-09-05. They are informative inputs to this living document; current CNCF policy always wins.

- CNCF Project Lifecycle and Process  
  https://contribute.cncf.io/projects/lifecycle/

- CNCF Sandbox application template  
  https://github.com/cncf/sandbox/blob/main/.github/ISSUE_TEMPLATE/application.yml

- CNCF Sandbox onboarding checklist  
  https://github.com/cncf/sandbox/blob/main/.github/ISSUE_TEMPLATE/project-onboarding.md

- CNCF Sandbox overview  
  https://www.cncf.io/sandbox-projects/

- CNCF TOC process  
  https://github.com/cncf/toc/blob/main/process/README.md

- General Technical Review questions  
  https://github.com/cncf/toc/blob/main/toc_subprojects/project-reviews-subproject/general-technical-questions.md

- Governance review template  
  https://github.com/cncf/toc/blob/main/toc_subprojects/project-reviews-subproject/governance-review-template.md

- Incubation due-diligence template  
  https://github.com/cncf/toc/blob/main/operations/toc-templates/template-dd-pr-incubation.md

- TOC due-diligence guide  
  https://github.com/cncf/toc/blob/main/operations/dd-toc-guide.md

---

# 20. Final Readiness Thesis

Ono-Sendai is ready for CNCF when the following statement is true without qualification:

> Ono-Sendai has demonstrated a real cloud-native systems-interface use case, external people are meaningfully participating in its development, governance can outlive the founder, security and release processes are credible, maintainers can sustain the workload, and CNCF would strengthen an existing community rather than manufacture the appearance of one.

The objective is not to make Ono look like a CNCF project.

The objective is to make Ono a healthy open-source project for which CNCF is the natural next home.
