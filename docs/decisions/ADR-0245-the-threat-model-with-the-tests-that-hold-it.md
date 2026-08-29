# ADR-0245: The threat model, with the tests that hold it

- Status: accepted
- Date: 2026-08-29
- Spec refs: §17, §31.16, §35.6, §43, §49; ADR-0015 (superseded by this one), ADR-0203
- Decided by: agent (autonomous)

## Context

ADR-0015 turned spec §49's fifteen threats into a table, and its own Consequences said what was
missing: "several rows belong to phases that do not exist yet, so the table is a standing debt
tracked in `docs/STATE.md` until each is closed." The debt was the *Proven by* column. It named
intentions — "completion tests with hostile candidates", "fuzz targets plus explicit deep/large
input tests", "test: a recycled pid is not signalled" — and three rows named a file. Not one row
named a function.

That matters because of what the column is for. `docs/ACCEPTANCE.md` §4.4 ticks a release box on
this table, and §3 of the same document forbids a box that judgement alone can tick. A row naming
an intention is ticked by judgement: nothing fails when the test it describes is renamed, deleted
or `#[ignore]`d, because nothing ever read the row. ADR-0203 did it correctly for the seven rows
it added for the v0.4 §35 boundaries — each names a test, and
`xtask/tests/spatial_evidence.rs::should_find_every_test_the_spatial_enumeration_review_names`
asserts every one of them exists and runs. This ADR does the same for T1–T15.

Every row's mitigation is already implemented and already tested; nothing here is new behaviour.
What is new is that the table now points at the tests, and the gate reads the table.

## Decision

The table below replaces ADR-0015's. The threats, the mitigations and the owners are ADR-0015's
unchanged — this is not a re-analysis — and the *Proven by* column names, for every row, at least
one test function that exists, runs in the gate and is not ignored.
`xtask/tests/spatial_evidence.rs::should_find_every_test_the_threat_model_names` reads the rows
and fails when one of them does not.

| # | Threat (spec §49) | Mitigation | Owner | Proven by |
|---|---|---|---|---|
| T1 | malicious filenames containing control sequences | every value is sanitised at the render boundary, unconditionally; the raw value is retained separately (spec §49) | `ono-render` | `crates/ono-render/tests/presentation.rs::should_sanitise_control_characters_even_when_no_colour_is_applied`, `crates/ono-render/tests/value_scalars.rs::should_make_an_escape_sequence_in_a_value_inert`, `docker/acceptance/cases/048-hostile-bytes-stay-data.case` |
| T2 | terminal escape injection from external stdout | external bytes reaching a **terminal** are passed through only while the external command owns the foreground terminal (ADR-0013); bytes Ono itself renders are sanitised | `ono-process`, `ono-render` | `crates/ono-cli/tests/external.rs::should_keep_bytes_verbatim_at_a_terminal_under_raw`, `crates/ono-render/tests/value_table.rs::should_make_an_escape_sequence_inert_all_the_way_through_the_table`, `docker/acceptance/cases/048-hostile-bytes-stay-data.case` |
| T3 | plugin code execution | KUANG/11 isolation and the capability broker; install, enable, load and run are separate states (spec §31.8, §31.10) | `ono-kuang-supervisor` | `crates/ono-kuang-sdk/tests/conformance.rs::should_refuse_to_load_when_a_required_capability_is_denied`, `crates/ono-kuang-sdk/tests/conformance.rs::should_refuse_and_audit_a_path_outside_the_granted_scope`, `crates/ono-kuang-testhost/tests/adapter_package.rs::should_refuse_a_package_whose_adapter_names_an_undeclared_executable` |
| T4 | poisoned completion sources | completion never executes a candidate source; candidates are sanitised before display and never auto-accepted | `ono-editor`, completion | `crates/ono-editor/tests/completion.rs::should_sanitise_a_hostile_candidate_before_showing_it`, `crates/ono-editor/tests/completion.rs::should_stop_listing_the_candidates_when_the_next_key_edits_the_line` |
| T5 | remote agent impersonation | explicit trust store, pinned host keys, mutual authentication before any provider call | `ono-protocol` | `crates/ono-remote/tests/trust.rs::should_refuse_an_unauthenticated_transport_when_trust_is_required`, `crates/ono-remote/tests/trust.rs::should_link_and_answer_when_the_presented_key_matches_the_pinned_one` |
| T6 | host key changes | `remote.host_key_changed` (E0603), classified `safety` rather than transport (ADR-0006), and never auto-accepted | `ono-protocol` | `crates/ono-remote/tests/trust.rs::should_refuse_a_changed_host_key_with_the_stable_safety_code` |
| T7 | schema/protocol bombs causing memory exhaustion | bounded frames, bounded depth, bounded total size on every decoder; bounded channels everywhere (ADR-0013) | `ono-protocol`, `ono-kuang-protocol`, `ono-value` | `crates/ono-protocol/tests/framing.rs::should_refuse_a_frame_claiming_more_than_the_limit_before_allocating`, `crates/ono-protocol/tests/framing.rs::should_accept_a_payload_of_exactly_the_limit_and_refuse_one_byte_more`, `crates/ono-value/tests/codec_fuzzing.rs::should_refuse_a_deeply_nested_document_rather_than_exhausting_the_stack`, `crates/ono-provider-netlink/tests/malformed_messages.rs::should_report_a_message_whose_header_claims_more_than_it_carries` |
| T8 | history leakage of secrets | a `Secret` semantic type with redacted default rendering; a secret-aware history policy that redacts before writing (spec §17.5) | `ono-history`, `ono-value` | `crates/ono-history/tests/history.rs::should_redact_the_obvious_secrets_before_anyone_configures_it_to`, `crates/ono-history/tests/history.rs::should_redact_a_value_matching_a_configured_secret_pattern_before_writing`, `crates/ono-history/tests/history.rs::should_keep_a_hidden_command_out_of_the_file_and_not_merely_out_of_the_listing` |
| T9 | unsafe rendering of OSC hyperlinks | OSC is never emitted from data; hyperlinks only from a theme-controlled construct that cannot take a value's text as its target | `ono-render` | `crates/ono-render/tests/presentation.rs::should_carry_no_escape_sequences_from_a_value_into_the_terminal_when_painting`, `crates/ono-render/tests/error_rendering.rs::should_make_an_escape_sequence_in_an_error_message_inert` |
| T10 | command confusion between native and external namespaces | the fixed resolution order and forced namespaces of ADR-0011, with `explain` reporting what the code actually did | `ono-cli` | `crates/ono-cli/tests/language_missing.rs::should_resolve_a_function_before_an_external_command_of_the_same_name`, `crates/ono-cli/tests/external.rs::should_force_an_external_program_when_the_exec_namespace_is_used`, `docker/acceptance/cases/032-resolution-is-inspectable.case` |
| T11 | PATH shadowing | `explain` prints the absolute path of an external hit; a destructive command shows its resolved target before acting (spec §17.1) | `ono-cli` | `crates/ono-cli/tests/meta_config_missing.rs::should_resolve_an_external_program_to_its_path_when_nothing_earlier_claims_the_name`, `docker/acceptance/cases/032-resolution-is-inspectable.case` |
| T12 | TOCTOU between preview and destructive action | identity is confirmed immediately before mutation, not at preview time; a target whose identity changed is reported `failed` in its `ActionResult` rather than acted on | `ono-command`, providers | `crates/ono-command/tests/mutations.rs::should_resolve_a_selector_into_a_full_identity_before_acting`, `crates/ono-command/tests/mutations.rs::should_refuse_to_act_on_a_projection_that_has_no_identity` |
| T13 | PID reuse between selection and signal | a process's identity is `(pid, started)` — the `identity` list of `ono.process/1` — and is re-read before signalling; a mismatch refuses | `ono-provider-linux` | `crates/ono-provider-linux/tests/process.rs::should_refuse_to_signal_when_the_start_time_changed`, `crates/ono-provider-linux/tests/process.rs::should_deliver_the_signal_when_the_identity_still_matches` |
| T14 | symlink races | directory traversal uses `openat`-relative operations with `O_NOFOLLOW` where the operation is not meant to follow links; no path is re-resolved between check and use | `ono-provider-linux` | `crates/ono-provider-linux/tests/file.rs::should_stay_inside_the_tree_when_a_directory_is_swapped_for_a_symlink_mid_walk`, `crates/ono-provider-linux/tests/file.rs::should_not_descend_into_a_symlinked_directory_while_walking` |
| T15 | privilege escalation boundaries | elevation is explicit and visible (spec §17.2); no native command silently elevates; the prompt makes an elevated context impossible to miss | `ono-cli`, `ono-render` | `crates/ono-cli/tests/signals.rs::should_make_an_elevated_prompt_impossible_to_miss`, `docker/acceptance/cases/029-prompt-shows-context.case` |

### Standing rules that follow from the table

Unchanged from ADR-0015, and restated so this ADR stands on its own:

1. **Sanitisation is at the render boundary, not at the provider.** A provider must report exactly
   what the system said, because that is what makes `inspect` trustworthy (spec §49: retain raw
   data separately from display). The renderer is where hostile bytes stop.
2. **No security-relevant behaviour is conditional on a configuration setting.** T1, T2 and T9 in
   particular are unconditional; a setting that can turn them off is a setting an attacker can
   arrange to have set.
3. **Every decoder is fuzzed** (spec §35.6): the parser, each serializer, the remote protocol, the
   plugin protocol, and the procfs and netlink decoders. A decoder without a fuzz target is not
   finished.
4. **A refusal is never a prompt.** T5 and T6 fail with a structured error; they do not offer a
   "continue anyway" that a script will eventually answer for the user.

## Consequences

`docs/ACCEPTANCE.md` §4.4's threat-model box is now closed the way ADR-0137 requires every box to
be closed: by named tests that the gate runs, not by a reviewer's reading. Renaming any of the
thirty-odd tests named above without updating this table turns the gate red, which is the point —
the table and the suite move together or not at all.

The cost is that the table is now a maintenance surface: a test rename is two edits instead of
one. That is the same bargain ADR-0203 struck for the spatial rows, and it is the only bargain
that makes the column mean anything.

Encoded by: `xtask/tests/spatial_evidence.rs::should_find_every_test_the_threat_model_names`.

## Alternatives considered

- **Amending ADR-0015 in place.** Rejected: AGENTS.md §8 says an accepted ADR's history is not
  edited; superseding is the mechanism, and the superseded text stays readable beside this one.
- **Leaving the mapping in `docs/ACCEPTANCE.md` §4.4's prose bullet**, which already lists the
  files. Rejected: it names files, not tests, so a renamed or deleted *test* inside a file that
  still exists changes nothing, and the checklist is exactly the document §3 forbids from being
  ticked by judgement.
