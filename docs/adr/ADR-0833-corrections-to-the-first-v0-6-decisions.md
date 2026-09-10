# ADR-0833: Corrections to the first v0.6 decisions

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §4.1, §4.4, §12.3, §16.2, §16.3, §17.3, §22.1, §30.4, §36.4, §39.1, §39.2, §45, Appendix F
- Decided by: agent (autonomous)

## Context

A read-only audit of ADR-0800 to ADR-0821 held each factual sentence against the tree. Some
describe code that does something else, some count wrongly, and one describes a function that
does not exist. AGENTS.md §8 keeps an accepted record's text as it was written, so the corrections
live here, and each corrected record's `Status:` line points at this one. ADR-0818's `Status:`
points at ADR-0822, which replaced the part of it that no longer holds; its remaining errors are
recorded below.

Line numbers are those of the tree on the date above; several of these files are still moving.

## Decision

Each item quotes the claim, states what is true, and says whether the code or this record changed.

### 1. ADR-0800:42 — "every mutator is on the draft"

Nine builder methods on `ChangePlan` take `self` and return it changed without looking at the
state: `as_recovery`, `with_impact`, `with_protection`, `with_protection_mode`, `with_risk`,
`with_strategy`, `with_verification`, `binding` and `expiring_at`
(`crates/ono-change-core/src/plan.rs:222-313`), and `settled` (`plan.rs:441`) also overwrites
action statuses. Only `resolve`, `with_action` and `seal` refuse a sealed plan with
`change.plan_sealed` (`plan.rs:234`, `:251`, `:325`). An edit made through the other nine leaves
the recorded seal digest stale, and `ChangePlan::digest_holds` (`plan.rs:570`) is the predicate
that exposes it. Today only tests call `digest_holds`: neither `apply` nor the plan store checks
it. §4.4 therefore holds by convention at those nine methods.

### 2. ADR-0801:45-47 — "Every `true` row's help text in the global registry says so in words, and `check_errors` refuses a code that does not answer the question"

`docs/contracts/recovery/errors.yaml` has 43 rows with `nothing_changed: true`. The help text in
`docs/contracts/errors.yaml` states it in words for 7 of them (`change.plan_not_sealed`,
`change.plan_already_applying`, `change.plan_store_unavailable`,
`recovery.destructive_history_not_accepted`, `recovery.asset_create_failed`,
`recovery.coverage_insufficient`, `recovery.quiesce_failed`). The other 36 don't mention it.
`xtask/src/change.rs::check_errors` (`change.rs:1125-1236`) checks that each row has a code and a
name, that `nothing_changed` is a boolean, and that the rows and the global registry agree in both
directions. It never reads help text. The same claim is repeated in the header of
`docs/contracts/recovery/errors.yaml:13-14`.

### 3. ADR-0803:69-71 — "refuses with `change.plan_reference_ambiguous` listing the candidates and the width that tells them apart"

The refusal listed the candidates in its metadata and gave no width. **Implemented with this
record:** `ono_change_core::error::plan_reference_ambiguous` computes the shortest prefix width
that tells the candidates apart (`shortest_unique_prefixes`, never below four). It carries that
width as `width` in the metadata, and the help reads "write at least N characters of the identity
to tell them apart: …" with each candidate at that width. The test is
`error::tests::should_state_the_prefix_width_that_tells_ambiguous_candidates_apart`. The asset
resolver shares the function (`crates/ono-change-plan/src/store.rs:605`), so an ambiguous
recovery reference gets the same answer.

### 4. ADR-0804:26-29 — "`PlanState::after` … is the only way a plan's state changes"

`after` is the only source of *edges*, and three paths set a state without asking it:

- `ChangePlan::resolve` falls back to `Resolved` where `after` answers `None`
  (`plan.rs:237-240`);
- `seal` assigns `Sealed` and `revise` assigns `Draft` directly (`plan.rs:334`, `:349`);
- the executor assigns outcome states (`crates/ono-change-executor/src/execute.rs:1410`, `:1561`,
  and the verdict mappings around `:2097-2148`). Its durable writes go through
  `PlanStore::record_state` (`store.rs:809-844`), which checks only that the row exists and
  accepts any state. `ono-cli` writes `PREPARE_FAILED` over `SEALED` without passing through
  `PREPARING` (`crates/ono-cli/src/change/lifecycle.rs:254`).

Transitions are guarded in three places. `ChangePlan::advance` refuses an edge `after` does not draw
(`plan.rs:365`). `apply` refuses a plan that is not `SEALED` (`change.plan_not_sealed`).
`xtask/src/change.rs::check_transitions` (`change.rs:374-470`) holds the machine against
`docs/contracts/change/plans.yaml` and asserts that `(PrepareFailed, BeginApply)` is absent. The
store's state column has no transition check.

### 5. ADR-0805:39-40 and :45 — "`ProcessRunner` … with a bounded wait", "Fifty-eight files"

The ZFS runner had a bounded wait (`crates/ono-recovery-zfs/src/process.rs`); the Btrfs runner
called `Command::output()` and waited indefinitely. **Implemented with this record:**
`ono_recovery_btrfs::ProcessRunner` spawns the child, drains both pipes on their own threads,
polls against a deadline (`DEFAULT_TIMEOUT`, thirty seconds; `ProcessRunner::within` sets
another), and on expiry kills the child and refuses with `recovery.provider_unavailable`. This
mirrors the ZFS runner. `crates/ono-recovery-btrfs/tests/runner.rs::should_stop_waiting_for_a_program_that_does_not_finish_within_its_bound`
runs a real `sleep 30` under a 300 ms bound.

The fixture directories hold 57 recordings (26 ZFS, 31 Btrfs) plus one `README.md` each.

### 6. ADR-0809:38-39, :58-60, :62-67 — consistency

- "A ZFS or Btrfs snapshot of a running database is `crash-consistent`": the providers label a
  single snapshot `filesystem-consistent` (`crates/ono-recovery-zfs/src/provider.rs:1633`, `:1688`,
  `:1771`; `crates/ono-recovery-btrfs/src/provider.rs:1136`, `:2688`), and nothing in either
  provider detects a database or any other application in the snapshotted domain. The true
  statement: such a snapshot is filesystem-consistent at the filesystem level, which for an
  application inside it is at best crash-consistent unless the application was quiesced, and
  nothing in the tree detects the application to say so. A Btrfs set of several subvolumes
  composes down to `crash-consistent` (`crates/ono-recovery-btrfs/src/assets.rs:143-154`), which
  is Appendix D.7's case.
- ":58-60, a KUANG/11 application provider raises the claim by participating in the quiesce
  protocol": what is checked is the declared capability string. A recovery-provider contribution
  claiming `application-consistent` without `recovery.quiesce` in its capabilities is refused at
  load (`crates/ono-kuang-supervisor/src/change.rs:230-250`). Nothing observes the protocol being
  run.
- ":66-67, the owner column is checked at the registry": `check_consistency`
  (`xtask/src/change.rs:834-899`) checks the registry's ranks and owners. The ownership check
  (`check_consistency_ownership`, `change.rs:2320-2366`) is a text scan: `consistency_claims`
  (`change.rs:2369-2412`) looks for `at_consistency(ConsistencyClass::…` in each first-party
  provider's `src/*.rs`. A claim spelled any other way is invisible to it.

### 7. ADR-0811:59-60 — "`recovery.plan_incomplete` now carries four situations"

`ono-change-recovery` raises it at six call sites, for six situations. The analysis did not finish
(`gate.rs:71`). Objects in the restore scope could not be established (`gate.rs:79`). A host holds
no asset (`remote.rs:218`). The request named no asset (`builder.rs:242`). The chosen method
contributed no action (`builder.rs:572`). No method meets the goal (`method.rs:567`). Seven more
call sites outside that crate raise it too:

- `ono-change-plan/src/store.rs:420`, no stored analysis;
- `ono-cli/src/change/recovery.rs:57`, no retained asset;
- `ono-recovery-zfs/src/checklist.rs:291` and `provider.rs:2106`, `:2807`;
- `ono-recovery-btrfs/src/error.rs:42`;
- `ono-recovery-files/src/provider.rs:727`.

### 8. ADR-0812:42-47, :53-56, :60-61 — the §16 surface

- "§16.2's four VM snapshot kinds are distinguished by `ConsistencyClass` plus the
  memory-inclusion fact a provider states": no field anywhere carries memory inclusion.
  `RecoveryAssetType::VmSnapshot`'s documentation says it must be stated
  (`crates/ono-change-core/src/asset.rs:31`), and `RecoveryProviderContribution`
  (`crates/ono-kuang-protocol/src/message.rs:653`) has no field for it. Its `asset_type` is checked
  only for being non-empty (`crates/ono-kuang-supervisor/src/change.rs:159`), so `vm-snapshot` is
  accepted without the fact. §16.2 says a VM provider "MUST distinguish" memory-inclusive
  snapshots from disk-only ones, so this is an open requirement. It is not yet reachable, because
  no VM provider exists.
- "§16.3's prohibition … is `ConsistencyClass::CrashConsistent` and the ownership rule of
  ADR-0809": the ownership rule is the text scan of item 6, over first-party crates only. A KUANG
  contribution is held by item 6's load-time capability check. Nothing refers to container
  checkpoints specifically.
- ":53-56, the `ono.recovery-provider/1` conformance version … checked by `xtask/src/change.rs`":
  the version is a constant (`crates/ono-change-core/src/provider.rs:97`) that first-party
  providers stamp and their own tests assert. Nothing in `xtask` reads it, and a KUANG
  recovery-provider contribution does not carry one.
- ":60-61, §30.4's verification — installed version, package-manager consistency, affected
  service state — is declared in `plannable_operations`": the package rows declare `installed`,
  `version == {option:version}` and `consistent` (`docs/contracts/change/actions.yaml:460-462`,
  `:482-483`, `:506-508`). Affected service state is not declared there; the comment at
  `actions.yaml:436-438` leaves it to the impact layer.

### 9. ADR-0814:65-67 — "fails to resolve, producing `resolve.command_not_found`"

A word after `plan` that is not a plannable operation is refused by
`crates/ono-cli/src/change/actions.rs::unresolvable` (`actions.rs:515-540`). The code is
`change.action_not_plannable` when the word is not a program on the tool path, and
`change.opaque_action_forbidden` when it is one. `crates/ono-cli/tests/change_planning.rs:205-212`
asserts the first.

### 10. ADR-0815:42-46, :54-56 — "`narrowed` returns the mode the plan asked for whenever it differs … `narrowing_note` is the sentence an operator sees … it says both things"

When ADR-0815 was accepted, `ProtectionPolicy::narrowed` answered for any difference between the
requested and the effective mode, and `narrowing_note` had no caller outside tests. `narrowed` now
answers only where the effective mode does not deliver what was asked
(`crates/ono-change-protection/src/policy.rs:691-693`, `delivers` at `:867-874`). Among the
modes `effective_mode` can produce, that is the one pair ADR-0815 names: `maximize` requested
under `require`. `narrowing_note` (`policy.rs:723`) still has no caller outside tests, so an
operator does not see the note yet. ADR-0815's "it says both things" holds only once a view prints
it.

### 11. ADR-0816:84-87 — "`xtask/src/change.rs::check_quiesce` is what keeps that true"

When ADR-0816 was accepted, `check_quiesce` checked only §39.3's registry entry: five steps, three
flags, and the two error codes. Nothing checked what first-party providers declare. It now also
runs `first_party_optional_capabilities` (`xtask/src/change.rs:2520`, the function at about
`:2987`). That function reads every non-comment line under `crates/ono-recovery-*/src` and reports
any spelling of `recovery.quiesce` or `recovery.transaction` (`RecoveryCapability::Quiesce`, the
quoted name). No first-party provider declares either today. The check is a text scan, with item
6's limit: a declaration spelled another way would pass.

### 12. ADR-0817:26-28, :61 — "the outcome's own state on every exit from `apply`, including every early return", "Every write is `let _ = …`"

The executor writes `PREPARING` only when the plan has protection actions
(`execute.rs:1399`). It writes `PREPARE_FAILED` at `:1419`, `APPLYING` at `:1442`, and the outcome
at `:1446` only while it holds the plan. The pre-flight refusals before `:1399` write nothing,
which leaves the plan in the state it was already in. The executor's writes are not discarded:
`write_state` records a failure in the request's `store_failures` (`execute.rs:1577-1585`). The two
`let _ =` writes are `ono-cli`'s (`crates/ono-cli/src/change/lifecycle.rs:254`, `:384`).

### 13. ADR-0818 — amended by ADR-0822

ADR-0818's `Status:` names ADR-0822. Three of its statements no longer hold:

- "`world::execute_with` routes a `RecoveryOperation`": there is no `execute_with`.
  `world::execute` refuses a `RecoveryOperation` (`crates/ono-cli/src/world.rs:471`, `:542-548`).
  A recovery plan's actions are routed by the plan's kind: every action of a recovery plan goes to
  `recovery::restore`, which hands it to the provider's `restore_with` in whatever form it was
  planned (`crates/ono-cli/src/change/lifecycle.rs:283-288`,
  `crates/ono-cli/src/change/recovery.rs:704-710`).
- "Only `recovery.restore` is carried out this way": the provider receives every action of the
  recovery plan, including ZFS `Program` actions and Btrfs operations whose capability is
  `recovery.prepare`.
- "the first-party providers build them as `Execution::RecoveryOperation`": Btrfs and files do
  (`crates/ono-recovery-btrfs/src/provider.rs:2919-2933`,
  `crates/ono-recovery-files/src/provider.rs:778-785`). ZFS plans `Execution::Program`
  (`crates/ono-recovery-zfs/src/provider.rs:2510`), as ADR-0822's context already says.

### 14. ADR-0819:40-41, :50 and ADR-0820:57-59

- ADR-0819:40-41, settling a PREPARE action by target against the created asset's scope domain,
  holds (`execute.rs:1871-1897`).
- ADR-0819:50, "`PREPARE 1/1 APPLY 1/1 VERIFY 1/1`", is unverified. `ono-change-render/src/progress.rs`
  renders one count per phase, and acceptance case `docker/acceptance/cases/295-apply-and-verify.case:42-43`
  matches only `PREPARE +[0-9]+/[0-9]+`. No test asserts that line for a protected plan.
- ADR-0820:57-59 on `plan kill process`: three irreversible effects is true
  (`docs/contracts/change/actions.yaml:375-394`). `HIGH` on irreversibility is asserted
  (`docker/acceptance/cases/297-risk-gates.case:53-57`, `crates/ono-cli/tests/change_settings.rs:125-140`).
  "three mutation domains, all `UNPROTECTED`" and "`not recoverable` naming the subject" are
  asserted by no test, and this record does not verify them.

### 15. ADR-0821:42-43, :55 — "`apply` and `protect` use it; `plan` keeps `created`. One creation, one event."

When ADR-0821 was accepted, `recover` and `remove recovery` constructed `PlanLifecycle::created`
for a plan that already existed, and each recorded a second `PlanCreated`. Both now use
`PlanLifecycle::continuing` (`crates/ono-cli/src/change/recovery.rs:541`, `:560`), as `apply` and
`protect` do (`crates/ono-cli/src/change/lifecycle.rs:689`, `:698`). `plan` is the one caller of
`created` (`crates/ono-cli/src/change/plan.rs:1057`), which is what ADR-0821 says.

## Consequences

- ADR-0800, 0801, 0803, 0804, 0805, 0809, 0811, 0812, 0814, 0815, 0816, 0817, 0819, 0820 and 0821
  now read `accepted — corrected by ADR-0833`; ADR-0818 reads `accepted — amended by ADR-0822`.
  Their bodies are unchanged. A record whose `Status` is no longer plain `accepted` leaves the
  scope of `xtask/src/terminology.rs::check_decisions`, which is that check's own rule for a
  record whose correction lives elsewhere.
- Items 3 and 5 changed code. The other items change only this record. Items 1, 2, 4, 6 and 8
  describe checks that are weaker than their records said. Each one is a candidate for its own
  work item, and this record fixes none of them.

## Alternatives considered

- **One superseding ADR per corrected record.** Fifteen records for fifteen short corrections,
  most of them a sentence each. A single record, with an item per claim, can be read in one sitting.
- **Edit the records in place.** Forbidden by AGENTS.md §8.
