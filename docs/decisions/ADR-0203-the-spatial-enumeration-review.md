# ADR-0203: The spatial enumeration review

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §35 (security and permission boundaries), §35.1–§35.5, §36.2, §43.7, §51
  (SEC-S01 "spatial enumeration review"), §52.2 ("security review completed"); v0.2 §49;
  ADR-0015 (the threat model, made testable); `docs/ACCEPTANCE.md` §4.7.2; ADR-0137
- Decided by: agent (autonomous)

## Context

v0.4 §52.2 makes a completed security review a release criterion and §51 names it SEC-S01, the
*spatial enumeration review*. §35's own sentence says what is at stake: "Spatial discoverability
must not become unauthorized enumeration." A navigable projection of a host is, by construction,
an enumeration tool; the question the review has to answer is whether it enumerates anything the
user could not have asked the providers for directly.

`docs/ACCEPTANCE.md` §3 forbids a box that judgement alone can tick, and ADR-0137 fixed in
advance what closes this one: **an ADR that extends ADR-0015's T1–T15 threat table with one row
per §35 boundary, each row naming a passing test**, with `xtask/tests/spatial_evidence.rs`
asserting that every test the table names exists and is not ignored. The reviewer's judgement
picks the rows; the suite closes the box.

This is that ADR. ADR-0015's fifteen rows are unchanged and still hold — the spatial layer adds
no renderer, no decoder and no protocol of its own — and the rows below are numbered on from
them.

## Decision

The review is the table. Each row is a release-blocking requirement in exactly the sense
ADR-0015's are: the mitigation is named, the component that owns it is named, and a test that
fails when the mitigation is gone is named.

| # | Threat (v0.4 §35) | Mitigation | Owner | Proven by |
|---|---|---|---|---|
| T16 | **enumeration without authorisation** (§35.1): the spatial layer becomes a way to read what the provider would have refused — a neighbourhood that lists another user's open files, a place record that carries a field the provider denied | the spatial layer never reads the system: it composes provider records (§2.16, ADR-0131), and a field the provider returned as an error stays an error through projection, ranking and rendering. There is no privileged path into the index | `ono-spatial-index`, `ono-spatial-core` | `crates/ono-cli/tests/spatial_identity.rs::should_report_permission_denied_rather_than_zero_files_for_another_users_process`, `crates/ono-cli/tests/spatial_contracts.rs::should_report_denied_information_as_denied_rather_than_as_an_empty_collection` |
| T17 | **a refusal disguised as an absence** (§35.2, §2.17): "0 files" where the truth is "14 file descriptors this user may not read", which teaches the operator the wrong thing about the host | the six states of §35.2 are one closed enumeration carried on every neighbourhood group, and every refusal a provider can state maps onto one of them; a withheld group has no count at all, so no renderer can print it as zero | `ono-spatial-index`, `ono-spatial-query` | `crates/ono-cli/tests/spatial_identity.rs::should_name_one_of_the_defined_permission_states_for_every_neighborhood_group`, `crates/ono-spatial-index/tests/conformance.rs::should_map_every_refusal_a_provider_can_state_to_one_of_the_six_states`, `crates/ono-spatial-index/tests/conformance.rs::should_report_a_denied_group_as_denied_rather_than_as_an_empty_one`, `crates/ono-cli/tests/spatial_relationships.rs::should_not_report_the_owner_of_a_socket_nobody_looked_up_as_no_owner` (added by ADR-0209: the row's other tests all exercise a provider that *stated* a refusal, and the defect `docs/dogfood/v0.4-2026-08-28.md` found was a provider that answered `null`) |
| T18 | **escalation by navigation** (§35.3): moving through the space asks for privilege the operator did not ask for, so that walking around a host silently becomes a privileged act | navigation is composed of ordinary provider reads and asks for nothing; `docker/acceptance/cases/097-spatial-permission-honesty.case` walks a restricted process as an unprivileged user in the container and asserts that the answer is a refusal and that nothing was elevated | `ono-cli`, providers | `docker/acceptance/cases/097-spatial-permission-honesty.case` (§35.3 assertions), `crates/ono-cli/tests/spatial_identity.rs::should_report_a_real_file_list_for_a_process_this_user_owns` |
| T19 | **silent dialling** (§35.4): `jump` opens a network connection because a selector *looks* like a hostname, turning a typo into an outbound connection and a name resolution into a side effect | a host is reachable only as a declared link; a name that is not one is refused with a structured error rather than resolved, and resolution never widens to a linked host unless the caller asked for it | `ono-cli`, `ono-spatial-query`, `ono-protocol` | `crates/ono-cli/tests/spatial_remote.rs::should_refuse_to_jump_to_a_hostname_that_is_not_a_known_link`, `crates/ono-spatial-query/tests/resolution.rs::should_not_reach_a_linked_host_unless_the_caller_asked_for_it` |
| T20 | **scope violation by identity merge** (§43.7, §10.2, §16.2): two objects in different scopes — two hosts, a container and its host — collapse into one place, so a fact observed inside one boundary is read as a fact about the other | the scope chain is part of the identity, not a label on it: a pid in a container and the same pid on the host are two identities, and so are the same process on two hosts | `ono-spatial-core`, `ono-spatial-index` | `crates/ono-spatial-index/tests/conformance.rs::should_keep_a_containers_pid_one_apart_from_the_hosts`, `crates/ono-cli/tests/spatial_remote.rs::should_keep_a_remote_process_place_distinct_from_the_local_one_with_the_same_pid`, `crates/ono-spatial-core/tests/identity.rs::should_give_the_same_pid_in_two_namespaces_two_identities` |
| T21 | **the remote boundary made invisible** (§2.18, §19.2): an operator acts on a remote object believing it is local, because nothing on the screen or in the trail said the boundary was crossed | crossing is announced in plain text at the moment it happens, the prompt carries the host afterwards, and every trail step records the host and the scope it crossed | `ono-cli`, `ono-render` | `crates/ono-cli/tests/spatial_remote.rs::should_announce_the_boundary_in_plain_text_when_jumping_to_a_linked_host`, `::should_mark_the_remote_host_in_the_prompt_after_a_jump`, `::should_record_the_host_and_the_scope_crossing_of_every_step_in_the_trail` |
| T22 | **the map as a plugin side channel** (§35.5, §36.2): a KUANG/11 package publishes nodes or edges describing what its capabilities do not let it see, and the host draws them as if they were its own facts | plugin contributions are filtered by capability scope *before* the merge, so an ungranted package contributes nothing at all, and every contributed edge carries the package as its origin so a reader can tell a plugin's claim from the host's | `ono-kuang-supervisor`, `ono-spatial-index` | `crates/ono-cli/tests/spatial_contracts.rs::should_keep_a_package_relation_out_of_the_map_until_its_capability_is_granted`, `::should_carry_the_contributing_package_as_the_origin_of_every_plugin_edge`, `crates/ono-kuang-testhost/tests/spatial_package.rs` |

### What the review looked at and found no row for

Stated so the absence of a row is a finding rather than an oversight:

- **§35.1 and the index's own memory.** The index caches what providers answered. A cache is a
  disclosure risk when it outlives the authorisation that produced it; here it does not, because
  the index is per session and per scope (ADR-0190 made the target cache carry the scope after a
  session that had jumped recalled a local answer for a remote host), and a stale entry is
  refused for a mutation rather than served (`crates/ono-spatial-index/tests/index.rs::should_refuse_to_hand_a_stale_entry_to_a_mutation`).
- **Rendering hostile names.** A place's label is provider data and can hold control sequences.
  This is ADR-0015's T1 and T9 unchanged: sanitisation is at the render boundary and applies to
  every value, including a `MapNode` label. The spatial layer adds no second render path — the
  full-screen view draws through `ono-render` — so no new row is needed.
- **§35.3's second sentence** ("an action or explicit privileged inspection may request
  escalation using the existing Ono security model"). Nothing in v0.4 adds a mutation; the
  spatial verbs are all reads. The escalation surface is v0.2's and is ADR-0015's T15.

### Standing rules this review adds to ADR-0015's four

5. **A place is never more privileged than the record it was projected from.** Any future
   provider that reads with elevated privilege must state so in its `docs/spec/providers/*.yaml`
   claims; a spatial place inherits the permission state of its source and never upgrades it.
6. **A boundary is a part of identity, not a decoration.** Host, namespace, container and mount
   boundaries are in the scope chain that identity is computed over. Adding a new kind of
   boundary means extending the scope chain, never adding a field beside the identity.

## Consequences

- `docs/ACCEPTANCE.md` §4.7.2's *security review completed* box is closed by
  `xtask/tests/spatial_evidence.rs`, which reads this file, extracts every `` `path::test` ``
  the table names, and fails if one does not exist or is `#[ignore]`d. The review cannot rot
  into a paragraph nobody runs.
- Seven rows became release-blocking requirements, and all seven are already green: this review
  found no unmitigated §35 boundary. That is a claim about today's tree, not a permanent one —
  a new spatial surface adds a row here in its own increment, exactly as a new phase adds one to
  ADR-0015.
- The two standing rules constrain future providers and future boundaries, and they are the part
  of this ADR no test can hold; the rows are the part that can.

## Alternatives considered

- **Fold the rows into ADR-0015.** Rejected: AGENTS.md §8 forbids editing an accepted ADR's
  history, and ADR-0015's table is cited by `docs/ACCEPTANCE.md` §4.4 for the v0.2 release. An
  extension that can be read on its own is also the greppable answer to "what did the v0.4
  review look at".
- **A checklist inside this ADR that a reviewer ticks.** Rejected by ADR-0137 in advance: a
  checklist a human ticks is judgement with extra steps.
- **A single row for the whole of §35.** Rejected: five sections with five different mitigations
  and five different owners would share one test, and the first one to be deleted would take the
  others' evidence with it.
