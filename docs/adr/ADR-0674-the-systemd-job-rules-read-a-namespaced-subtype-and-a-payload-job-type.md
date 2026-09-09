# ADR-0674: The systemd job rules read a namespaced subtype and a payload job type

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §6.1, §6.5, §15.2, §15.8, §22.2; `docs/contracts/temporal/events.yaml`;
  ADR-0627, ADR-0712
- Decided by: agent (autonomous)

## Context

`docs/contracts/temporal/causality.yaml` declares two systemd rules that must be told apart:
`ono.systemd-job-result` explains *"the one its result names"* and `ono.systemd-job-to-unit-state`
explains *"the transitions a job produces while it is running"*. Both read `provider.event` and
`object.changed`, and the registry says the two are kept separate *"because the two carry
different evidence and a reader of an explanation is entitled to know which fired"*.

Nothing in the contracts says how a `provider.event` announces which systemd signal it is, or
where a job's type — `start`, `stop`, `restart`, `reload` — lives. `events.yaml` gives
`provider.event` a `payload` that must be *"typed and namespaced"* and gives every event a
`subtype` that is *"§6.1's namespaced provider or plugin refinement"*, which is the right shape
for both facts and does not name them.

## Decision

The causal engine reads exactly four things off a systemd job event, and exports the names of the
first two so the provider and the rules cannot disagree quietly:

| Fact | Where | Constant |
|---|---|---|
| which signal this is | `subtype` | `SYSTEMD_JOB_REMOVED` = `linux.systemd-dbus.job-removed`, `SYSTEMD_JOB_NEW` = `linux.systemd-dbus.job-new` |
| the job type | `payload.job_type` | `JOB_TYPE_FIELD` |
| the job path | an `EvidenceClaim::Transaction` token from `linux.systemd-dbus` | — |
| the unit | the event's `subject` or `related` `SpatialRef` | — |

The job path is evidence rather than payload because it is the identity the rules join on, and
§15.8 requires the join to be inspectable: the link's evidence list then names the record that
carries the token, so a reader following an explanation reaches the fact rather than a field.
ADR-0712 already widened `SystemdBus::queue_job` to return the `OwnedObjectPath`, so the token
exists to be recorded.

The unit comes from a canonical `SpatialRef` rather than from a unit name in the payload, because
§5.1 keeps spatial identity authoritative and a rule that joined on a string would join two units
with one name across a rename.

A job's lifetime is the interval spanned by the presentation instants of every event in the
candidate window carrying that job's token. That is systemd's own report of when the job existed,
read back out of the ledger, rather than a duration Ono chose.

`ono.systemd-job-to-unit-state` additionally requires the job type to be consistent with the state
the unit reached: `start` with `activating|active|reloading`, `stop` with
`deactivating|inactive|failed`, `restart` with either, `reload` with `reloading|active`. An
inconsistent pair emits nothing, because the job identity alone does not establish that *this*
transition is the one the job produced.

## Consequences

- Agent J's systemd source must set `subtype` to one of the two constants and write `job_type`
  into the payload, or the two job rules never fire. The constants are exported from
  `ono_temporal_query::causal` so the provider can use the same values the rules read.
- A systemd event that carries the job path and nothing else still reaches
  `ono.provider-causal-token`, which needs only the token and the provider's own sequence. So a
  partially instrumented source degrades to a weaker rule rather than to silence.
- The unit state field is read under both `active_state` and `state`, because sources spell it
  both ways and refusing one of them would be a rule that fails for a reason nobody can see.

## Alternatives considered

**Discriminate the two signals by the presence of a `result` field in the payload.** Rejected: it
infers the signal from a field's absence, and §6.2's discipline about absence applies to a
provider's payload as much as to a field change.

**Put the job path in the payload beside the job type.** Rejected: the path is the join, and a
join that is not evidence produces a link whose evidence list does not contain the reason it
fired.
