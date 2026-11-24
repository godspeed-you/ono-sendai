# ADR-1084: v0.6 §3.1's `update package` is `set package <name> --latest`

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.6 §3.1, §4.3, §6.1, §7.2, §17.2, §30.1–§30.5, Appendix I.1; v0.2 §7, §9.1, §40.1;
  ADR-0115, ADR-0813, ADR-0935
- Issues: #291
- Decided by: agent (autonomous)

## Context

v0.6 §3.1 lists `update package openssl` among its example intents, Appendix I.1 plans it
("openssl 3.x -> newer candidate"), and `docs/contracts/commands/change.yaml` documented
`plan update package openssl --protection require`. No verb `update` exists (`verbs.yaml`), so the
plan refused the example as not plannable, and the conformance register exempted it (ADR-0935).

v0.2 §7 keeps the verb registry small — "third-party modules reuse an existing verb whenever the
semantics match" — and §40.1 asks for a verb review before one is added. ADR-0813 applied that
rule to `replace file` (spelled `copy file`). The package family already has a verb whose
semantics match: `ono.package.set` is "Request a version transition where the provider supports
it", with `--version` naming the version to move to and `--hold` pinning it. An update is a version
transition whose destination is "the newest version the package manager's repositories offer" —
which `--version` cannot name without the operator first looking it up, and which a version string
cannot spell (`latest` is a legal version string for some managers).

## Decision

**`update package openssl` is spelled `set package openssl --latest`.** No verb is added.

- `ono.package.set` gains the flag `--latest`: move an *installed* package to the newest version
  its repositories offer. It never installs a package that is not installed, and it is refused
  together with `--version` (one destination per transition).
- The providers run the manager's own upgrade-only form: `apt-get install --only-upgrade -y
  <name>` (dpkg), `dnf upgrade -y <name>` (dnf/yum), `zypper --non-interactive update <name>`
  (zypper). As every package mutation, it runs only as root and answers `io.permission_denied`
  otherwise, before any manager is started (ADR-0115 §5); `changed` is the installed version before
  against after.
- Planning is the established path, unchanged: `plan set package openssl --latest` resolves the
  package through the provider and freezes its installed version into the identity and the
  §7.2 `version` precondition, takes `ono.package.set`'s declared effects, risk and verification
  from `docs/contracts/change/actions.yaml`, and evaluates protection over them. The advisory
  `version == {option:version}` check is dropped where no version was named, as for `--hold`
  (§23.1: a check that cannot be stated is not invented). Planning runs no package manager
  command that changes anything; `apply` goes through the plan/apply executor and its protection
  gate like every other plan.
- The newest candidate is **not** resolved at planning time. Doing so would mean reading
  `apt-cache policy`'s human output, which §50 forbids a provider to parse; the plan says what it
  does ("move `openssl` to the newest version its repositories offer"), §7.2's precondition still
  refuses an `apply` whose installed version moved since the seal, and the verification reads the
  installed version afterwards.
- `change.yaml`'s example becomes `plan set package openssl --latest`, and its conformance
  exemption is removed: the example runs in the conformance suite's scratch shell on any host
  whose package manager has `openssl` installed (this run's host and the CI image both do).
  `--protection require` is not part of the example: no first-party provider maps a package's
  files to a persistence domain yet, so `require` refuses such a plan with
  `recovery.coverage_insufficient` (E1807) — which is the honest answer, and which
  `--protection require` still gives.

## Spec deviation

- Section: v0.6 §3.1, Appendix I.1
- Text: "`update package openssl`" / "`local:// > plan update package openssl`"
- Instead: `set package openssl --latest`, and `plan set package openssl --latest`.
- Why: v0.2 §7 reuses an existing verb where the semantics match, and `set package` already is
  the package's version transition; a verb `update` would be a synonym the verb review of §40.1
  exists to prevent.

## Consequences

- Tests (no host mutation — a scratch `PATH` of fake package managers, ADR-0115's pattern):
  `crates/ono-cli/tests/containers_packages.rs::should_plan_an_update_of_an_installed_package_with_its_protection_evaluated`,
  `…::should_refuse_to_plan_an_update_of_a_package_that_is_not_installed`,
  `…::should_refuse_an_update_and_a_named_version_together`,
  `…::should_fail_with_permission_denied_when_updating_a_package_unprivileged`; the provider's argv
  is held by unit tests in `crates/ono-provider-linux/src/packages.rs` and `packages_rpm.rs`.
- Acceptance case `docker/acceptance/cases/441-plan-a-package-update.case` plans an update of an
  installed package in the container and shows that `apply` as the unprivileged user changes
  nothing.

## Alternatives considered

- A verb `update` in `verbs.yaml` — a synonym for a transition `set` already names; §40.1.
- `set package openssl --version latest` — a version string, so a manager whose versions may be
  words could not tell the two apart.
- `set package openssl` with no option meaning "newest" — an empty `set` is refused today as naming
  nothing, and a default destination for a mutation is a surprise.
