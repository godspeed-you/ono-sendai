# ADR-0442: The confinement control table is a registry the gate compares

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.3 (safety mechanisms fail closed), §15.1 (the three concepts), §16.1
  (mandatory controls), §16.4 (mandatory versus best-effort, in *one* central table), §16.5
  (the confinement report), §17.1–§17.3 (optional stronger isolation, no marketing ahead of it),
  §52.1 (`kuang_confinement_controls` as a machine-readable registry), §52.2 (single source of
  truth), §52.3 (an unknown control id fails the gate), §56.5 (the supervisor owns the table),
  Appendix D (the confinement matrix); base spec §31.10, §31.15, §31.16, §31.79;
  AGENTS.md §2 (`docs/spec/`), §6 (registries are the public contract); ADR-0283, ADR-0430
- Decided by: agent (autonomous)

## Context

v0.4.1 §16.4 requires that "each control MUST be declared either `mandatory` or `best_effort` in
one central table" and gives seven rows. Appendix D gives eleven rows of the same table in a
different shape, with a `Failure` column §16.4 does not have and four rows §16.4 does not mention
— filesystem isolation, network isolation, seccomp and Landlock, all `not provided by native
tier`. §16.1 gives a third list: the eight controls the default native tier must attempt. §52.1
names the table `kuang_confinement_controls` and asks for it as a machine-readable registry
rather than prose-only constants; §52.3 makes an unknown control id in a tier definition a gate
failure.

Before this increment there was no table at all. `crates/ono-kuang-supervisor/src/sandbox.rs`
installed seven controls inside one `pre_exec` closure, and which of them mattered was a property
of the code's control flow — specifically, of the fact that nothing checked any of them
(§0.5.3, ADR-0430). Nothing named a control, so nothing could report on one, refuse a spawn over
one, or be asked whether a spawn had had one.

Three questions had to be answered before #59, #60 and #61 could be written, because all three
consume the answer.

## Decision

**The table is `docs/spec/hardening/kuang_confinement_controls.yaml`, and the gate compares it
against `ono_kuang_protocol::confinement` in both directions on every run.**

**1. It is a registry under `docs/spec/hardening/`, not under `docs/spec/kuang/`.** The seven
`docs/spec/kuang/` contracts are the *plugin API*: what a package author writes and what the two
sides of the protocol agree on. A confinement control is not that — no package declares one, no
manifest names one, and a plugin never learns which controls it is running under. It is hardening
policy, and §52.1 enumerates it beside `security_boundaries`, `remote_limits` and five others as
one of seven registries of exactly that kind. `docs/ACCEPTANCE.md` §4.8.5 already names the path
`docs/spec/hardening/kuang_confinement_controls.yaml`, and #117 collects the other six into the
same directory. Putting it beside the plugin contracts would have made #117 move it.

**2. The vocabulary is Rust in `ono-kuang-protocol`; the behaviour is Rust in
`ono-kuang-supervisor`.** §56.5 gives the supervisor "mandatory/best-effort control table" as a
responsibility, and it keeps it: the supervisor decides what to do about a control and builds the
report. What it does not own is the *names*. `Control`, `Requirement`, `FailureBehaviour` and
`ExecutionTier` are vocabulary that travels — through the `plugin.*_failed` errors of §16.3,
through the confinement report of §16.5, through the audit trail, through `inspect plugin` — and
in this repository travelling vocabulary lives in `ono-kuang-protocol` beside `Capability`,
`PluginState` and `KuangErrorCode`, each of which is held against a registry the same way. It is
also what lets `xtask` compare the two without depending on the supervisor.

**3. The eleven `not_provided` and unconfigured rows are rows, not omissions.** Appendix D closes
with "The UI/documentation MUST never infer the last four rows from the first rows", and the way
to make that hard is to answer the question rather than to leave it silent. So `Requirement` has
three values, not two: `mandatory`, `best_effort` and `not_provided`. Filesystem isolation,
network isolation, seccomp and Landlock are declared `not_provided` in `native-confined`, and so
are the three rlimits §16.4 makes "mandatory when configured by tier" that this tier does not
configure — `rlimit_address_space`, `rlimit_cpu` and `rlimit_processes` — each with the reason
beside it in the registry. A reader of the table can see that they were considered.

Three consequences of the shape are worth stating as rules:

- **A tier that declares any control declares all of them.** `spec-check` reports a control a
  tier's table omits, so a new `Control` variant cannot be added without deciding what every
  populated tier does about it. The Rust side is an exhaustive `match` for the same reason: the
  compiler is the first referee, the gate the second.
- **`native-isolated` and `wasm` are names with `available: false` and no control rows.** §17.2
  asks the tier model to make stronger isolation expressible; §17.3 forbids describing isolation
  that does not exist. A name the code refuses to select is the first without being the second.
- **A row is a claim about this build.** `mandatory` in the table means the supervisor really does
  refuse the spawn, which is what makes the table checkable at all: #59 and #60 make the code
  match the rows, and the gate then holds them to each other.

## Consequences

Easy: #59, #60, #61 and #64 all consume one vocabulary. The confinement report of §16.5 is one
row per `claimed_controls()`, the fail-closed decision of §16.3 is `requirement().is_mandatory()`,
and the error a failure raises is `Control::failure_code()`. The documentation of §15.2 has one
place to be generated from, which is what §19.2 asks for. #117 finds the directory already there
with a working `check_hardening_contracts` to extend.

Hard: the table is now two artifacts that must agree, and the gate is the only thing making them.
That is the deliberate trade of §52.2 — one typed number, many readers — and the cost is that
adding a control is a two-file change. `spec-check` fails loudly in both directions, so the cost
is visible rather than latent.

Also hard: `Control` is `#[non_exhaustive]` and its variants are public API. A control that turns
out to be misnamed cannot be renamed without a contract change. The ids follow the specification's
own words (`no_new_privs`, `session_separation`, `fd_hygiene`) rather than the syscall names, so a
platform that installs the same control by another means keeps the id.

Encoded by: `crates/ono-kuang-supervisor/tests/confinement.rs::should_classify_every_control_the_confinement_table_declares`,
`::should_treat_a_control_the_table_calls_mandatory_as_mandatory`,
`xtask/tests/contracts.rs::should_reject_an_unknown_control_id_in_a_kuang_tier_definition`,
`::should_reject_a_tier_row_whose_requirement_disagrees_with_the_supervisor`,
`::should_match_the_confinement_control_table_against_the_runtime_that_serves_it`.

## Alternatives considered

**Constants in `sandbox.rs`, with a doc comment listing which are mandatory.** What §16.4 exists
to forbid, in as many words: "one central table". A comment is not consumed by the report, the
errors or the tests, so the three would drift from it and from each other.

**The table in `docs/spec/kuang/lifecycle.v1.yaml`, beside `isolation_tiers`.** Tempting, because
that file already declares T0–T3. Rejected: those tiers are spec §31.10's *manifest* vocabulary,
declared by the package and checked at install, and a confinement control is a host-side policy
the package never sees. Mixing them would put an operator's hardening policy inside the contract a
third-party author writes against.

**Two values only — `mandatory` and `best_effort` — with unprovided controls left out.** Exactly
half a control's requirement is the interesting half. Appendix D's last sentence is a warning
against inference, and an absent row is the strongest possible invitation to infer.

**Generate the Rust from the YAML with a build script.** The purest reading of §52.2, and how
`docs/reference/` is produced. Rejected here because the generated artifact would be the security
decision itself: a build script that mis-parses one row silently downgrades a mandatory control,
and nothing downstream would notice. A hand-written exhaustive `match` that the gate compares
fails closed instead — the compiler refuses an undecided variant, and `spec-check` refuses a
disagreement.
