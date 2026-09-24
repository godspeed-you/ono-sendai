# ADR-0880: A test reads the host once, owns what it reads, or brackets the second reading

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4 §2.16, §2.20, §7.4, §9.3, §9.4, §15.3, §29.4, §43.2; v0.4.1 §2.7, §2.17, §34.4,
  §38.1, §38.2, §38.4; AGENTS.md §11 ("never rely on the developer machine's real processes
  unless the fixture creates them")
- Issues: #155, #164, #165, #215
- Decided by: agent (autonomous)

## Context

Six tests compared two observations of a live host and failed when the host changed between
them. All of them were seen red on a busy machine and green on a quiet one, and every one of them
was first read as a product defect:

- `spatial_topology.rs::should_stream_neighbors_as_pipeline_objects_when_near_runs_at_the_root`
  counted the rows of one `ono` run of `near` against the count of another (#165). The root's
  neighbours include the containers the container provider reports; a container started between
  the two runs turned 34 rows into a count of 35.
- `spatial_orientation_bound.rs`'s two service tests compared a bounded orientation, and a bounded
  `get service`, against a second, unbounded `get service` (#155). Starting one container
  registers three systemd units: 594 before, 597 in the orientation.
- `spatial_topology.rs::should_show_the_mounts_the_mount_provider_answers_for_when_entering_storage_mounts`
  compared `get mount` in one run with `near` at `storage/mounts` in another (#215). One
  container adds an overlay mount and a network-namespace mount; the old test failed on
  `/run/docker/netns/…` with a container started between its two runs.
- `spatial_map.rs::should_only_remove_nodes_and_leave_no_dangling_edge_when_a_type_filter_narrows_the_map`
  required every node of a type-filtered map to be in a complete map taken after it (#215). A
  process that ended in between is in the first and, wherever the second observation does not
  share the first one's session, not in the second.
- `spatial_topology.rs::should_complete_the_relations_available_from_the_current_place_when_tab_follows_follow`
  waited for the entered process's pid, which the typed walk's own echo already carries (#215).
  Its completion budget then started while the walk was still running.
- `jobs_native.rs::should_finish_a_bounded_background_pipeline_and_say_so` waited a fixed 0.4 s
  for a job to finish (#164).

ADR-0552 settled one instance of this — the terminal-width comparison — and said nothing about
the rest. This ADR states the rule they share.

## Decision

**A test that compares what the shell says with what the host is takes the host's side from one
of three places, in this order of preference.**

1. **One reading.** Where the shell can hold an observation, both assertions are derived from it:
   `let neighbors = (near); $neighbors | select relation | count; $neighbors | to json` asks the
   host once, and the count and the rows are two readings of the same answer. The claim under
   test — `near` composes with the v0.2 stages — is unchanged.
2. **A population the fixture owns.** Where the population is the subject, the fixture supplies
   it. The service tests start a private `dbus-daemon` and serve `org.freedesktop.systemd1` on it
   with seventeen units, and the shell reaches it through `DBUS_SYSTEM_BUS_ADDRESS`. The
   production provider runs unchanged — `Manager.Version`, `Manager.ListUnits`, `LoadUnit`, the
   `Unit` and `Service` properties — and the whole population is a number the test chose, so the
   bound under test, `limits.orientation_objects` against the whole population, is asserted
   against an exact figure rather than a second reading. A host without `dbus-daemon` skips with
   `external_tool_unavailable`, the category the two rows already declared.
3. **A bracketed second reading.** Where the comparison is between two sources that cannot be one
   reading and whose population no fixture can own — the mount table against the spatial layer's
   places — the provider is read before and after the observation under test, in the same run.
   Agreement settles it. A place the first reading does not know is a defect only when the two
   readings around it agree with each other, because then the table held still and nothing but
   the spatial layer can have produced it. When they differ the host moved: the attempt is
   repeated, three times as in ADR-0552, and then the test announces
   `SKIP(fixture_not_applicable)` naming what moved. The skip is declared, and it is listed under
   `canonical_ci.permitted_skips`, because whether a host holds still is a property of the host.

Two rules go with them:

- **An object that has ended is evidence about the machine, and the process table says whether it
  ended.** The node-filter test skips a filtered node that is absent from the complete map only
  when its `object_ref` names a process that no longer exists; one still running was running when
  the complete map was drawn, and must be in it.
- **A wait is for the state the next step needs, never for a symptom that can precede it.** The
  completion test waits for the place the walk arrives at — the prompt's `process/sleep` — and
  the job test polls `get job`, the structured form of the table `jobs` prints (spec §18.4), until
  the job is no longer `running`, under the shell's watchdog.

No assertion was loosened. Every comparison asserts what it asserted before, against an
observation that can only differ for the reason the test is about.

## Consequences

Easy: all six fail for one reason. With a container started between the old tests' two
observations each was red; each new one passed 20 of 20 at load 12–35 and 20 of 20 with 24 extra
busy loops on eight cores, and the ones about live populations passed while containers were
started and removed beside them in a loop.

Hard: the mount test can skip, on a host whose mount table changes across all three attempts.
That is the same trade ADR-0552 made, for the same reason, and the acceptance container, whose
mount table nothing changes, measures the same contract.

Also hard: the service tests no longer read the host's systemd. Reading it is covered elsewhere —
`ono-provider-systemd`'s own suites and every `get service` test — and what these two tests are
about, a count that is the provider's rather than the sample's, needs a provider whose count is
known. The fixture implements only the D-Bus surface the shell reads today; a provider that
starts asking systemd for more will find the fixture's manager answering `UnknownMethod`, and the
fixture grows with it.

A transient mount that appears after the first reading and is gone before the second, inside one
run, would read as a place the provider does not know. The window is the length of one `near`,
and no case of it has been seen; the bracket is as good as ADR-0552's, and no better.

Encoded by the six tests named above, and by the `declared:` and `permitted_skips:` rows of
`docs/contracts/hardening/expected_test_skips.yaml` for the mount test.

## Alternatives considered

**Scale or lengthen the waits.** Neither race is about time. A longer wait makes the echo-matched
completion pass more often and still waits for the wrong thing; the two-reading comparisons fail
at any speed when the host changes between them.

**A private mount namespace for the mount test.** Deterministic, and refused on this host:
`unshare -rm` fails with "Operation not permitted" under Ubuntu's
`apparmor_restrict_unprivileged_userns`, and a fixture that needs a user namespace is a skip on
every host that restricts them.

**Bracket the service tests too.** It works and it was the first version written. The issue asks
for a single reading or an owned population, and an owned population is strictly better where one
can be had: it cannot skip, and the figure it compares against is exact.

**Compare only what no container can change.** The containers and mounts are exactly the live
objects §2.16 and §29.4 are about; comparing only the static geography would remove the host and
the assertion together.
