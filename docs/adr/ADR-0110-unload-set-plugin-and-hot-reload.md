# ADR-0110: `unload plugin`, `set plugin` with persisted enablement, and hot reload

- Status: accepted
- Date: 2026-08-27
- Spec refs: §31.3, §31.8, §31.31, §31.38, §31.72; lifecycle.v1 `unload`, `enable`, `disable`, `hot_reload`; ADR-0065 §6, ADR-0068, ADR-0107
- Decided by: agent (autonomous)

## Context

Lifecycle.v1 names `ono.plugin.unload` and `ono.plugin.set` as the commands behind its
`unload`, `enable` and `disable` transitions, and spec §31.31 puts management state on disk.
Spec §31.72 wants a stateless package with no jobs to reload immediately. None of the three
existed; `load plugin` on an already loaded package replaced the handle and left the old
process running.

## Decision

### 1. `unload` and `set` are `act` on `ono.shell`

`ono.shell` advertises `plugin.unload` and `plugin.set`, so both bind through the mutation
road (ADR-0068 §3) and answer one `ono.action-result/1` row per package. `unload` takes the
instance out of the host and shuts it down with `ShutdownReason::Unload`; its contributions
are withdrawn with it, so `<package>:command` is `resolve.command_not_found` afterwards. A
package that is not loaded is `skipped`, not failed. `set --enabled false` unloads first
(lifecycle.v1 `disable`), then records; `set --background` records. Unchanged settings are
`skipped`; `--dry-run` reports what would be recorded.

### 2. Enablement is management state on disk

`enabled` and `background` live in `<state dir>/kuang/<id>/management.json` (ADR-0107 §3), so
a later session sees them. `load plugin` of a disabled package refuses with
`safety.policy_denied` naming `set plugin <id> --enabled true`; the package stays `installed`
with `enabled: false` in `get plugin`.

### 3. Re-loading replaces

`load plugin` of a package that is loaded shuts the running instance down
(`ShutdownReason::Upgrade`) before the new one is kept — spec §31.72's
`stateless-no-jobs → reload-immediately`, which is every package this build runs, since
invocations finish before the next statement. The manifest is re-read from disk, so
`get plugin` shows the reloaded version; the capability check is re-run against it
(lifecycle.v1 `hot_reload` rules) because the load is an ordinary load.

## Consequences

- `crates/ono-cli/tests/plugins_missing.rs`:
  `should_withdraw_contributions_when_a_package_is_unloaded`,
  `should_disable_a_package_and_refuse_to_load_it`,
  `should_persist_enablement_across_sessions`,
  `should_show_the_new_version_when_a_loaded_package_is_reloaded`.
- `--timeout` on `unload` is accepted; the supervisor's own drain deadline applies.

## Alternatives considered

- **Enablement in the session's settings (`set config`).** A package's eligibility is about
  the package, not the shell; spec §31.31 puts it beside the package's state. Rejected.
- **Refuse `load plugin` of a loaded package.** Spec §31.72 and ADR-0065 §6 both say
  re-loading replaces. Rejected.
