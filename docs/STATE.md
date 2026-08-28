# STATE

The shared work board. **Read it first, update it last, every session** (AGENTS.md section 9).
The stopping rule lives in `docs/ACCEPTANCE.md`: the run ends when `scripts/release-check.sh`
passes, not when this file looks tidy.

Working branch: **`implementation`** — never commit to `main` (AGENTS.md section 12.1)

**Commit every increment, and tag every completed phase.** A phase is done when its box in
`docs/ACCEPTANCE.md` section 4.1 is ticked; the commit that ticks it gets an annotated tag
`phase-<letter>` whose message names the exit criterion and the case that proves it. The tags are
how the state after each phase stays findable in a run of hundreds of commits:

```bash
git tag -n99 phase-a          # what Phase A delivered, and what proves it
git switch --detach phase-a   # the tree exactly as that phase left it
```

Tags so far: `phase-a` … `phase-j` (one per completed phase; H, I and J tagged at the release commit).

**Push after every commit.** AGENTS.md §12.1 keeps `main` untouched and §12.2 asks that
`implementation` be pushed freely so work is not lost; the branch and its phase tags live on
`origin`. Never push `main`, never open a pull request unless asked.

```bash
git push origin implementation && git push origin --tags
```

**`release-check: the shell is release-ready` — printed 2026-08-26 by `scripts/release-check.sh`
at commit 21b37d9.** All ten phases of spec §37 are complete, proven and tagged; every box in
docs/ACCEPTANCE.md §4 is ticked by a named automated proof; the containerised suite stands at
35 cases green and the workspace at ~1 400 outcome tests across 21 crates. What remains under
Next up is post-release deepening — every item deliberate, none blocking the deliverable.
Promoting `implementation` to `main` is the user's decision and the user's action
(AGENTS.md §12.1).

Phases A–D are complete and tagged. B/C/D landed as: native commands wired into the evaluator
(ADR-0028), partial failure semantics (ADR-0029), the §33.5 interop serialisation (ADR-0030),
path/string comparability (ADR-0031), the pre-flight field check (spec §11.3), shell stdin into
a parsing head (§12.4), unquoted `explain` over a pipeline, the provider registry
(docs/spec/providers/), and acceptance cases 040–044.

---

## The specification set

`docs/ono_sendai_shell_spec_v0.2.md` is the **base**. `docs/ono_sendai_shell_spec_v0.3_external_command_adapters.md`
is an **enhancement layered on it** — the External Command Adaptation Layer — and both are
immutable (AGENTS.md §5.2, ADR-0026). `spec-check` fails if either is missing a checksum line in
`docs/spec.sha256` or if `AGENTS.md` does not enumerate an enhancement by name.

**The v0.3 tranche is in progress (started 2026-08-27).** v0.2 was released as `v0.2.0` from
`main` at `273d3cd`; the External Command Adaptation Layer is implemented on `implementation`
in the same loop, against the same referee. Its definition of done is `docs/ACCEPTANCE.md`
§4.6 — 39 boxes derived from v0.3 §2.1–§2.6, §1.67 and §1.68 — and `scripts/release-check.sh`
is red until every one of them is ticked by a named automated proof. **ADR-0027 carries the
analysis**: what v0.3 requires, which five existing decisions grow (ADR-0006, -0011, -0013,
-0016, -0022), where v0.2 and v0.3 read differently, and the decomposition of the tranche. Read
ADR-0027 and §4.6 before picking a task; do not re-derive them from the 2182-line document.

Build order (v0.3 §1.69, one increment per line, each one RED-first):

1. `ADAPT-001` OutputDemand computed backwards from the consumer, reported by `explain`
2. `adapter.*` error family (E09xx) in `docs/spec/errors.yaml`
3. `ADAPT-003` guaranteed raw spelling
4. `ADAPT-009` manifest schema + `docs/spec/adapters/`, `spec-check` drift rules
5. `ADAPT-002` registry, identity pinning, negotiation states, conflict resolution
6. `ADAPT-004`/`005` plan execution through `ono-process`, streaming decoders, fuzz corpus
7. `ADAPT-007` provenance on adapted values, `inspect --provenance`
8. `ADAPT-006` version probe cache
9. `ADAPT-010` fixture harness generated from the contracts
10. Tier A tools: util-linux (`lsblk`/`findmnt`/`lsns`), `ip`, `journalctl`, `systemctl`
11. Tier B tools: `ps`, `stat`/`df`/`find`, `git`, `lsof`
12. Tier C tools: `ss`, `curl`
13. `ADAPT-008` KUANG/11 capability mapping, SDK, test host, packs and trust
14. `ADAPT-011` remote negotiation
15. integration surfaces: completion, history, script determinism, muscle-memory diff
16. release evidence: reference pages + compat matrix, overhead measurement, README

The container image gains the tools of step 10–12 when step 10 starts; a tool adapter is not
delivered until its live case runs there (`docs/ACCEPTANCE.md` §4.6.3).

---

## Product direction from the user (2026-08-26)

**"Es muss immer cool sein und Spaß machen, es zu benutzen. Es soll aufregend sein."** The shell
is the Ono-Sendai deck: correctness is the floor, not the ceiling. Where a decision is
otherwise free, prefer the option that feels alive — the prompt as a HUD, tables that update in
place, colour that means something, latency you never notice (spec §34's budgets are product
quality), and answers that invite the next question (`@2 | inspect`). Phase F's `watch` is the
showcase: a live view of the machine should feel like instrumentation, not like polling.

## In progress

**S11c — the four defects the v0.4 dogfooding session left open — is complete (2026-08-29,
agent `S11c`).** Seven commits, gate green on each; the container ran on image
`ono-sendai:acceptance-s11c`. Final verdicts: `gate: green`, `acceptance: 88 passed, 0 failed`,
`release-check: the shell is release-ready`.

| Commit | What it delivers |
|---|---|
| `fix(spatial)` | a null a provider left is not an empty exit (ADR-0209), and `ono.socket/1`'s `process` carries a refusal where the owner scan was refused |
| `fix(spatial)` | `find place` refuses a question it cannot ask (ADR-0210): E0202 for a field nothing declares, and evaluation errors surface |
| `test(spatial)` | the PTY budgets are liveness bounds, not a race with the machine |
| `fix(spatial)` | a refusal lists its candidates as values, not as newlines in its message (ADR-0211) |
| `fix(spatial)` | the hidden count says what it counts (ADR-0212) |
| `fix(spatial)` | two corrections the container found in the first two of those — an exit is "stated" by its group, and a record rather than a target decides whether a predicate can be asked |
| `test(spatial)` | the jump-refusal budget is a hang guard, not a race with the machine |

**Two things the container caught that the workspace suite did not**, both in this session's own
first drafts, both now covered by a workspace test as well: an exit is keyed by its *group*
(`process`) and not by its `follow` label (`owner`), and a provider *target* may serve several
schemas, so the *record* decides whether a predicate can be asked of it. Cases `091`/`094`
(`44.2m`, `44.5g`) and `092` (`44.3c`) are the assertions that found them.

**One flake seen twice and not fixed**, because it is a premise about the host rather than a
claim about the shell and its fix is not this session's work:
`spatial_topology_missing.rs::should_bound_the_root_horizon_instead_of_listing_every_known_object`
runs `get process | count`, and on a busy machine a process listed by the enumerator exits before
its `/proc/<pid>/stat` can be read, so v0.2 §9's partial-failure semantics give the run exit 1 —
correct behaviour, and a test premise that only holds on a quiet host. Seen in one gate run and
one `release-check` run; green on the next of each. Filed under *Next up*.

The four findings, and the one that was offered as a bonus:

- [x] finding 2 — a `null` a provider answered is rendered as `empty` (ADR-0209)
- [x] finding 3 — `find place --where` swallows unknown fields and evaluation errors (ADR-0210)
- [x] finding 4 — a multi-line diagnostic prints `\u{a}` instead of its line breaks (ADR-0211)
- [x] finding 1 — `look`'s hidden count does not describe the list above it (ADR-0212)
- [ ] `help here` (§38.2, a SHOULD) stays filed under *Next up*: it is a new user-visible
  capability needing help metadata, completion and an acceptance case, and none of the four fixes
  above went near the help code, so it was not the cheap addition it would have been for S11b.

**S6 + S7 + S8 + the map correction are integrated on one branch (2026-08-28, agent `integrate-1`).**
Three merges, in that order, on top of `implementation` at `cbbcd2c`; gate green, acceptance 75/75.
The resolutions worth knowing later:

- **`home` extends the navigation history (ADR-0184).** S8's ADR-0170 had excluded it; §20.1 lists
  `home` in the `movement` enum and §2.4 makes every movement reversible, so `back` returns
  through it. ADR-0170 is superseded on that one point, and
  `docker/acceptance/cases/106-spatial-remote.case` s8u spends three `back`s where it spent two —
  its assertion is unchanged.
- **`map --live` has two surfaces, and `Invocation::displays()` (ADR-0173) decides which.** Where
  the values are *shown* — an interactive terminal — it is S6's full-screen polled view
  (ADR-0176); where they are *consumed* it is S7's event-driven stream (ADR-0180), which is what
  `map --live --json | take 3 | to json` reads. Shown with no terminal to draw into it is still
  refused with `spatial.unsupported`, which §25.2 requires rather than a faked view. ADR-0180
  itself assigns the alternate screen to S6 and the stream to S7; this is that split.
- **The expansion memory of ADR-0183 lives in `crate::spatial::map::project_at`**, the one
  re-projection path the still map, the full-screen view and the live stream all take (§45.4).
- **`look --changes` answers `unknown`, not `unsupported`** (ADR-0181), so case 102's s4q reads
  the new word. It is delivered now; `unsupported` was the honest answer while it was not.
- **A case body that ends with a background job still running makes the acceptance runner report
  exit 129**, however green its assertions are: the orphan holds the outer `script`'s
  pseudo-terminal open. Reproducible with `( sleep 5 ) & exit 0` and nothing else. Case 107 now
  reaps its typist inside `drive`; any future PTY case must do the same.

- **What a target answered belongs to a moment and to a host (ADR-0190).** ADR-0186's target
  cache collided with two other decisions when the branches met. With ADR-0180, a live map
  re-projected by reading the answer from *before* the change, so `live::reproject` now calls
  `SpatialSessionState::forget_targets` first — an event is precisely the statement that §33.3's
  lifetime assumption no longer holds (§33.2). With ADR-0169's remote scopes, the cache key was
  the target name alone, so a session that jumped into a link recalled the *local* answer for the
  remote host; the key now carries the scope (§43.7). Case 106's s8l catches the second, and
  case 108 the first.

Two tests are environment-dependent on a developer machine and green in the container. Neither is
a merge regression:

- `spatial_relationships_missing::should_show_the_connection_edge_appear_and_vanish…` — **the
  TIME_WAIT identity collapse S7 recorded**, now diagnosed exactly. `ono.socket/1` declares
  `identity: [inode]` and a socket in TIME_WAIT has no inode, so *every* TIME_WAIT socket on the
  host projects to the same `SpatialId`. The test's own closing connection is then merged with
  whatever else on the machine happens to be in TIME_WAIT, and the third live value describes a
  foreign peer instead of the closure. The acceptance container has no other TIME_WAIT sockets,
  which is why case 108 is green and this is not. **Exit test:** two TIME_WAIT sockets are two
  places. The fix belongs to the v0.2 identity contract — a record whose identity components are
  all null has no identity and must not merge (§2.17, §35.3) — and is its own increment, not an
  integration's.
- `spatial_topology_missing::should_complete_the_relations_available…` — a PTY completion test
  with an 8 s budget; it fails under parallel load and passes with `--test-threads=1`.

**The v0.4 tranche is running (started 2026-08-28).** The specification is
`docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md`; its executable requirements are
the nine `crates/ono-cli/tests/spatial_*_missing.rs` suites (175 tests) and the ten
`docker/acceptance/cases/09x-spatial-*.case.v04` scenarios (139 assertions). The build order is
§50's own dependency-driven sequence, and a phase is done when its suites are un-ignored and
green — never by judgement:

**A phase is done when the tests the map assigns to it are green — not when a suite is.** The
test-to-phase analysis of 2026-08-28 (175 tests read body by body) gives the counts below and
found that §50 leaves eleven normative areas unassigned; they are slotted here, and the S4 block
is split, because 102 of the 175 tests first become attemptable when the command surface exists:

| Phase | Tests | Also owns (areas §50 never assigns) |
|---|---:|---|
| S1 | 5 | §47 configuration declarations |
| S2 | 1 | §18 device spaces — §50's identity list omits Device although §7.7 makes DEVICES a domain |
| S3 | 3 | — (`find place` + the ADR-0124 rewrite in one commit) |
| S4a `look`/`near` + domains | ~30 | §31 `trace` interop (a trace never moves the place) — **done**, 32 tests |
| S4b `enter`/`follow` | ~30 | §30 `cd`/place integration, §35 permission honesty |
| S4c `back`/`up`/`home`/`trail`/`jump`/`pin` | ~25 | §46 session state, §29.2 script isolation — **done**, 19 tests |
| S4d storage and the cwd distinction | ~12 | §15 mount boundaries |
| S4e the `spatial.enabled` refusal path and the §34 budgets | ~5 | §47 behaviour half |
| S5 | 27 | §26 landmark engine, §39 ASCII fallback — **done**, 26 tests; 2 deferred (ADR-0165) |
| S6 | 13 | §5 startup horizon, §21 prompt/HUD, §27.2 picker, §9.4 completion — **done**, 14 tests |
| S7 | 7 | — |
| S8 | 12 | — |
| S9 | 2 | — |
| S10 | 3 | — |
| S11 | 0 | the ten `.case.v04` scenarios renamed and green, the §34 budgets as gates, **`docs/ACCEPTANCE.md` §4.7 written from v0.4 §52** — without it `release-check.sh` cannot see this tranche — **the ten scenarios done**, 87/87 cases green |

| Phase | Delivers (§50) | Suites it turns green |
|---|---|---|
| S1 | spatial core contracts: `SpatialId`, projection, canonical places, relation registry, hierarchy, trail, structured errors, machine-readable registries | `spatial_contracts_missing` (registry, errors) |
| S2 | provider identity and relation bridge, canonical parents, permission-state propagation, conformance | `spatial_identity_missing`, `spatial_contracts_missing` (conformance) |
| S3 | index, aliases, selector resolution, `find`, neighborhood, pins, freshness | `spatial_topology_missing` (discovery) |
| S4 | the navigation commands | `spatial_navigation_missing`, `spatial_topology_missing`, `spatial_storage_missing` |
| S5 | `SpatialMap`, ranking, clustering, zoom, text renderer, relation inspection | `spatial_map_missing` |
| S6 | the interactive full-screen map | `spatial_interactive_missing` |
| S7 | live topology, tombstones, landmark updates, freshness | `spatial_relationships_missing` (live), `spatial_identity_missing` (tombstones) |
| S8 | remote federation | `spatial_remote_missing` |
| S9 | KUANG/11 spatial SDK | `spatial_contracts_missing` (§36) |
| S10 | v0.3 adapter reconciliation | `spatial_contracts_missing` (§37) |
| S11 | release hardening: the ten §44 scenarios renamed to `.case` and green | the acceptance suite |

Architecture is normative in responsibility, not in name (§45): `ono-spatial-core` (identity,
places, relations, trail, tombstones — no rendering), `ono-spatial-index` (registration,
reconciliation, aliases, freshness, canonical parent, pins), `ono-spatial-query` (look/near/find
plans, ranking, zoom, clustering), `ono-spatial-render` (text and full-screen), `ono-spatial-events`
(event merge, diff, live). `ono-cli` parses, dispatches and owns the session place — nothing more
(§45.6).

- (empty — the v0.4 tranche is complete through S11b; no agent holds a claim)

**S1 — spatial core contracts — is complete (2026-08-28, agent `S1`).** Five commits, gate green
on each:

1. `feat(spatial)` the fourteen §40 errors as the `spatial` family `Ono-Sendai-E1001`–`E1014`
   (ADR-0125, ADR-0127) in `docs/spec/errors.yaml` and `ono_core::ErrorCode`.
2. `feat(spatial)` the §41 registry: `docs/spec/spatial/{spatial,spaces,relations,landmarks}.yaml`
   (ADR-0126, ADR-0128), wired into `xtask spec-check`.
3. `feat(spatial)` `crates/ono-spatial-core` — `SpatialId` and the §10 tiers (ADR-0129), the
   `SpatialObject` projection, `SpatialScope` with boundary detection, `Place`, `HierarchicalEdge`
   and `RelationshipEdge`, the canonical geography, the canonical-parent resolver (ADR-0130), the
   navigation trail and tombstones.
4. `feat(spatial)` `crates/ono-spatial-index` — registration and §42.1 reconciliation, the alias
   and search index, freshness, canonical-parent lookup, bounded relation summaries and pins
   (ADR-0131).
5. `feat(config)` the eleven `spatial.*` settings of §47, plus the five landmark thresholds §26.3
   requires to be configurable.

Green from `crates/ono-cli/tests/spatial_contracts_missing.rs`:
`should_register_the_whole_spatial_error_family_in_the_error_taxonomy`,
`should_ship_the_machine_readable_spatial_registry`,
`should_declare_every_canonical_space_with_the_fields_the_registry_requires`,
`should_declare_every_relation_with_its_direction_labels_and_confidence`,
`should_expose_every_spatial_setting_as_a_typed_setting_with_its_default`.

Still ignored in that suite and correctly so: `should_serve_exactly_the_canonical_spaces_the_registry_declares`
and `should_serve_every_relation_it_declares_and_declare_every_relation_it_serves` run `map` and
`near`, which S3–S5 deliver. The registry-versus-implementation drift they describe is already
enforced against `ono-spatial-core` by `cargo run -p xtask -- spec-check`
(`xtask::contracts::check_spatial_implementation`); those two tests add the third party — what the
*commands* serve — and go green with S5.

**What S2 needs from S1** — the three things:

- `ono_spatial_core::Projection::project_as(record, object_type)` is the provider seam. The type
  is the caller's, because `ono.socket/1` is a `Listener` or a `Connection` and `ono.file/1` is a
  `Directory` or a `File`; `spatial_types_of(schema)` lists the candidates. `project` (without the
  type) works only where exactly one candidate exists.
- Identity is scoped and opaque (ADR-0129). Everything but a process takes its schema's identity
  fields plus the scope chain; a process takes boot identity, pid, start time and pid namespace
  (§10.2), reading `pid`, `started` and, where a provider supplies it, `pid_namespace`. Registering
  the same `(scope, ObjectRef)` under two ids is `spatial.identity_conflict`.
- The §42 provider claims block (`spatial:` under each entry in `docs/spec/providers/*.yaml`) is
  **not** written yet; `spatial_contracts_missing::should_declare_the_spatial_claims_on_every_provider_that_feeds_the_spatial_index`
  is S2's. Its `identity_strategy` must be one of `stable`/`lifetime`/`observation`, matching
  `ono_spatial_core::IdentityTier`, and its `cost_class` one of `ono_spatial_core::CostClass`.

**S2 — provider identity and relation bridge — is complete (2026-08-28, agent `S2`).** Six
commits, gate green on each:

1. `feat(spatial)` the §42 provider claims in `docs/spec/providers/*.yaml`, enforced by
   `xtask::contracts::check_provider_claims` (ADR-0132).
2. `feat(providers)` `pid_namespace` on `ono.process/1` and `ono.process-detail/1`, read from
   `/proc/<pid>/ns/pid` (ADR-0134).
3. `feat(spatial)` `ono_spatial_index::bridge` — which place a record is, and reconciliation
   (ADR-0133).
4. `feat(spatial)` the core exact relations, composed from provider facts (ADR-0135).
5. `feat(spatial)` permission-state propagation from the provider to the group (ADR-0136).
6. `test(spatial)` the bridge's type table held against the canonical geography.

**§50's gate for S2 — "provider objects can be reconciled into one graph without duplicate
identity for known-equal objects" — is met**, proven twice in
`crates/ono-spatial-index/tests/bridge.rs`: one process seen through `ono.process/1` and
`ono.process-detail/1` is one place, and one disk seen through `linux.sysfs` (`ono.device/1`) and
the util-linux `lsblk` adapter (`ono.block-device/1`) is one place — which is also §37.1's
adapter identity merge, four phases early.

Green from `crates/ono-cli/tests/spatial_contracts_missing.rs`:
`should_declare_the_spatial_claims_on_every_provider_that_feeds_the_spatial_index`. The other 46
new outcome tests live in the crates: `crates/ono-spatial-index/tests/{bridge,relations,conformance}.rs`
(37) and `crates/ono-provider-linux/tests/process.rs` (3), plus the existing suites unchanged.

**What S3 needs from S2** — the three things:

- **`ProviderBridge` is the entry point, not `Projection::project`.** `bridge::spatial_type_of`
  decides a record's place from the record (`ono.socket/1` → Listener or Connection,
  `ono.file/1` → Directory or File, `ono.device/1` → BlockDevice or Device), and
  `ProviderBridge::absorb(index, records, at)` registers a batch and settles its relations.
  `Absorbed` keeps four outcomes apart: `added`, `reconciled`, `unplaced` (a schema §7 gives no
  domain — a value, not an error) and `refused` (a place that could not be built). A schema that
  no canonical domain holds — `ono.image/1`, `ono.link/1`, `ono.plugin/1`, `ono.package/1` — is
  deliberately not a place (ADR-0133).
- **Selector resolution has two different keys, and they are not the identity.**
  `SpatialIndex::by_alias`/`search` answer what a *user* types (S1's alias index);
  `ProviderBridge::resolve(type, key)` answers what another *record* names — a pid, an interface
  name or index, a unit name, a uid, a container's full or short id, a path, a socket inode. It
  walks `SpatialType::is_a`, so a reference to a `Socket` reaches the `Listener` it is. Neither is
  the `SpatialId`, which stays opaque.
- **A neighborhood group may already be refused before ranking sees it.**
  `SpatialIndex::relation_summary` returns a `withheld` group with one of §35.2's six states and
  the provider's own message wherever a field carried an error, and `total()` is `None` there —
  so `near`/`look` must render the state, never fall back to a count. `SpatialIndex::withheld(id)`
  lists them. Three places are composed rather than served — `Endpoint`, `Cgroup`, `Namespace` —
  and are ordinary index entries with real identities (ADR-0135); `up` from a file follows the
  Unix path tree through `ono_spatial_core::PATH_PARENT`, not a relation.

**S3 — index queries, selector resolution, `find place`, neighborhood, pins — is complete
(2026-08-28, agent `S3`).** Five commits, gate green on each; `scripts/acceptance.sh` 68 passed,
0 failed:

1. `feat(parser)` a words-mode option whose value is a predicate expression (ADR-0138), declared
   in `docs/spec/language.yaml` and held against the parser by `spec-check`.
2. `feat(spatial)` `crates/ono-spatial-query` — §27.1 selector resolution, §27.2 ambiguity, §27.3
   fuzzy that never acts, §3.6 neighborhood ranking, §6.8 search, §34 cost-aware planning
   (ADR-0139).
3. `feat(spatial)` `find place` — the contract (`place` target, `ono.spatial-place/1`,
   `docs/spec/commands/spatial.yaml`) and the implementation in `ono-cli` (ADR-0140, ADR-0141),
   **with the ADR-0124 rewrite of every Table 3 site in the same commit**.
4. `feat(spatial)` pins that outlive the session — `$XDG_STATE_HOME/ono/pins.json` (§46.1).
5. `test(acceptance)` `docker/acceptance/cases/101-spatial-find-place.case`, the S3 gate in the
   container.

Green from `crates/ono-cli/tests/spatial_navigation_missing.rs`:
`should_stream_places_with_scope_and_provenance_when_find_searches_with_a_predicate`,
`should_compose_with_the_v02_pipeline_when_a_find_result_is_filtered_and_counted`,
`should_run_the_native_spatial_find_and_keep_the_external_find_reachable_when_both_exist`. The
other 40 new outcome tests live in the crates:
`crates/ono-spatial-query/tests/{resolution,neighborhood,search}.rs` (34),
`crates/ono-cli/tests/spatial_pins.rs` (6) and `crates/ono-parser/tests/parse_commands.rs` (3).

**What S4a needs from S3** — the four things:

- **`near`'s ranking is `ono_spatial_query::neighborhood_of(index, center, request, pins, now)`.**
  It returns S1's `Neighborhood` — `groups`, `landmarks`, `hidden_count`, `generated_at`,
  `completeness` — already bounded and ranked. `NeighborhoodRequest` carries §6.2's five options
  (`along`, `of_type`, `changed_within`, `limit`, `all`) plus `in_terminal_rows` for §3.6's
  terminal-size input. S4a builds the *command* and the record shape; it must not re-rank, and it
  must render a withheld group's §35.2 state rather than a count (`total() == None`).
- **`look`'s view is that neighborhood plus a place.** The place record is
  `ono.spatial-place/1` (ADR-0140) — `spatial_id`, `name`, `display_name`, `object_type` (the
  v0.2 schema), `spatial_type`, `place_path`, `scope`, `parent`, `freshness`, `observed_at`,
  `identity_tier`, `capabilities`, `pinned`, `provenance` — built by
  `crate::spatial::find::place_record`, which S4a should lift out of `find.rs` when `look` needs
  it too. A place carries no `pid`, `cpu` or `state`: those are the object's, one `inspect` away.
- **A place a user typed is `ono_spatial_query::resolve(index, selector, context, now)`.**
  `Resolution::require(selector)` turns it into the place or into §40's structured refusal —
  `spatial.ambiguous_selector` with §27.2's three columns, or `spatial.not_found` whose help
  lists the near misses. `SelectorContext::at(current_place)` is what makes steps 1 and 2 of
  §27.1 (visible child, visible neighbour) mean anything, so S4a must pass the session's current
  place, not `anywhere()`.
- **The index is built per command and thrown away (ADR-0141).** `find place` asks only the
  provider targets its query needs (`ono_spatial_query::discovery::targets_for`). S4a owns the
  step that changes this: §46's `SpatialSessionState` holds the current place *and* the index, so
  `look` twice reads the index rather than the providers, which is what §34's budget and S4e's
  `should_answer_repeated_looks_far_inside_the_look_budget` need. `spatial::local_scope()` gives
  the host and boot every observation belongs to (§10.2).

**Not S3's, and still open:**

- Canonical spaces are not answered by `find place`: it searches the index, and a space is
  declared geography rather than an observed object. If a later phase wants `find place compute`
  to answer the domain, that is a decision for it to record.
- ~~The `argument_mode`-versus-ADR-0009 check in `xtask::contracts::check_commands` is dead
  against this repository's own `docs/spec/language.yaml`.~~ **Repaired** by `harness` on
  2026-08-28 (ADR-0159): `expression_heads()` reads the sequence of named modes the registry
  actually writes, an empty declaration is now reported instead of short-circuiting the check,
  and the fixture is written in the registry's shape so it can no longer certify a blind reader.
  Exit test:
  `xtask/tests/contracts.rs::should_reject_an_argument_mode_that_disagrees_with_the_grammar_this_repository_declares`.

**Open, and deliberately not S1's:** `docs/ACCEPTANCE.md` has no v0.4 section yet, so
`scripts/release-check.sh` cannot see this tranche. §4.7 needs writing from v0.4 §52 before S11,
the way §4.6 was written from v0.3.

**S4a — `look`, `near`, and the six domains as real places — is complete (2026-08-28, agent
`S4a`).** Four commits, gate green on the last; `scripts/acceptance.sh` 69 passed, 0 failed:

1. `fix(command)` a bare flag followed by another option was dropped — `get dir --all
   --recursive` set only `--recursive`. A pre-existing defect, found because `look --all --json`
   hit it; fixed red-first in its own commit.
2. `feat(command)` an option whose value is optional (ADR-0144), so `look --changes [duration]`
   and `near --changed [duration]` are spelled as §6.1 and §6.2 write them.
3. `feat(spatial)` the commands themselves: contracts in `docs/spec/verbs.yaml` and
   `docs/spec/commands/spatial.yaml`, seven new schemas, `crates/ono-spatial-render`,
   `SpatialSessionState` in `ono-cli`, and `look`/`near`/`enter`/`home` (ADR-0142, ADR-0143,
   ADR-0145).
4. `test(acceptance)` `docker/acceptance/cases/102-spatial-look-near.case`, the S4a gate in the
   container: 48 assertions, none of which types a name the shell has not printed first.

Green now, all previously `#[ignore]`d — 32 tests:

- `spatial_topology_missing` (20): `should_report_the_system_root_as_the_current_place_when_home_runs`,
  `should_list_exactly_the_six_canonical_domains_when_looking_at_the_system_root`,
  `should_carry_a_permission_state_on_every_domain_so_an_unavailable_one_stays_visible`,
  `should_bound_the_root_horizon_instead_of_listing_every_known_object`,
  `should_describe_the_current_place_with_an_id_kind_name_scope_and_permission_when_looking`,
  `should_keep_the_same_spatial_id_for_the_root_across_separate_sessions`,
  `should_enter_every_canonical_domain_when_named_at_the_root`,
  `should_offer_the_{compute,network,storage,identity}_groups_the_spec_names_when_entering_*`,
  `should_keep_containers_and_devices_enterable_with_a_state_when_no_provider_contributes`,
  `should_show_the_users_the_user_provider_answers_for_when_entering_identity_users`,
  `should_show_the_mounts_the_mount_provider_answers_for_when_entering_storage_mounts`,
  `should_show_a_block_device_the_device_provider_answers_for_when_entering_devices`,
  `should_bound_the_neighborhood_and_count_what_it_hides_when_a_place_has_many_neighbors`,
  `should_expose_a_reason_on_every_landmark_when_a_place_reports_landmarks`,
  `should_stream_neighbors_as_pipeline_objects_when_near_runs_at_the_root`,
  `should_distinguish_an_unavailable_group_from_an_empty_one_when_a_domain_has_no_provider`,
  `should_resolve_find_as_a_spatial_verb_while_the_external_tool_stays_reachable_by_path`.
- `spatial_navigation_missing` (5): the three `look` tests, `should_bound_the_neighborhood_to_the_requested_size_when_near_is_limited`,
  `should_run_the_native_spatial_look_and_keep_the_external_look_reachable_when_both_exist`.
- `spatial_map_missing` (3): the three §24 tests —
  `should_describe_identity_state_exits_and_landmarks_when_look_json_reports_a_place`,
  `should_mark_a_group_as_an_exit_only_when_it_can_be_entered_when_look_lists_groups`,
  `should_not_invent_a_change_section_when_no_snapshot_or_event_source_exists`.
- `spatial_relationships_missing` (1): `should_keep_the_current_place_when_trace_projects_the_relationship_graph`.
- `ono-command::binding` (1, new): `should_bind_both_flags_when_one_bare_flag_follows_another`.

**What S4b needs from S4a** — the four things:

- **`enter` is already dispatched in two places, and both move the place.**
  `crate::context::claims` sends `enter` to the v0.2 context stack only when its first word names
  a target `docs/spec/commands/` declares for `enter` (`dir`, `process`, `service`, `user`, …);
  anything else — a domain, a collection, a pid, a quoted spatial id, or no argument at all —
  reaches `crate::spatial::commands::Enter`, the target-less `ono.place.enter`. §30.2 applies to
  both spellings, so `context::enter_record` now also calls `crate::spatial::enter_observed`
  (ADR-0142). S4b owns `enter @<result-ref>`, `enter .` and the piped form of §28.2.
- **`enter <object>` needs the object in the index first.** `Enter` resolves against what is
  known, and observes the current place's surroundings only when the declared answer misses. That
  is why `enter <pid>` at the root still refuses: the root observes the container and device
  targets and nothing else (ADR-0143's source table). The step S4b owns is planning the targets a
  *selector* implies — `ono_spatial_query::discovery::targets_for` already does exactly that for
  a predicate — so `enter 1842` from anywhere reaches the process.
  `spatial_navigation_missing::should_stream_neighbors_that_compose_with_the_pipeline_when_near_runs_in_a_script`
  is the near test that waits on it.
- **The trail is recorded, and nothing reads it yet.** Every move — `enter`, `home`, and the v0.2
  `enter <target>` — records a `NavigationStep` with its `Movement` in
  `SpatialSessionState::trail_mut()`. `follow` records `Movement::Follow` with the relation;
  `back`, `up` and `trail` (S4c) read what is already being written.
- **A place view is one function.** `crate::spatial::view::place_record` builds every
  `ono.spatial-place/1` the shell emits — `look`, `near` and `find place` — so a field `follow`
  needs is added once. `view::neighborhood_here` decides which of the two projections applies: a
  canonical space gets `ono_spatial_query::space_neighborhood` over observed exits, an object
  gets `neighborhood_of` over its edges. `follow <relation>` reads the second.

**Left ignored, and why** (S4a's assignment ends here):

- `spatial_topology_missing`: `should_answer_look_near_and_map_without_an_object_name_when_at_the_root`
  (all-or-nothing, and two of its six scripts are `map` — S5);
  `should_reach_a_process_it_never_names_…`, `should_offer_the_process_exits_…`,
  `should_follow_the_parent_relation_…`, `should_discover_a_listening_socket_…`,
  `should_reach_a_running_service_…` (all need `enter @-1` or `follow` — S4b); the two completion
  tests (§9.4, PTY — S6).
- `spatial_navigation_missing`: everything that needs `enter <object>`, `follow`, `jump`, `back`,
  `up`, `trail` or `map`.
- `spatial_map_missing`: everything about `map` (S5).

### S9 + S10 + ADR-0191, the last six tests of the tranche (2026-08-28, agent `S9S10`)

**Nothing in the nine spatial suites carries `#[ignore]` any more.** The six tests the table above
listed are delivered and green; `grep -rn '#\[ignore' crates/*/tests/*.rs` finds nothing.

- **S9 — KUANG/11 spatial extensions (§36, ADR-0194).** A package's `contributions.relations`
  shapes now register real relations in its own namespace, gated by `relation.write`: without the
  grant nothing is contributed, so §35.5's "filter before merging" holds by construction. A
  package asserts its edges as data — the new canonical schema `ono.spatial-relation/1`, answered
  by a contributed command whose target is `spatial-relation` — and the host resolves both ends
  through the canonical provider, so a package can say two objects are related and can never say
  an object exists. Every contributed edge carries the package as its provider and its `origin`
  and a §11.5 confidence the host never raises to `exact`.
  Files: `crates/ono-spatial-core/src/relation.rs` (the contributed registry),
  `crates/ono-cli/src/spatial/contributions.rs` (new), `crates/ono-cli/src/spatial/map.rs`
  (the merge into the horizon), `crates/ono-cli/src/plugins.rs` (adopt at load),
  `crates/ono-spatial-query/src/map.rs` (`--relations` accepts a contributing package),
  `crates/ono-kuang-sdk/src/bin/kuang-example-plugin.rs`,
  `crates/ono-kuang-testhost/{src/lib.rs,tests/spatial_package.rs}`.
- **S10 — v0.3 adapter reconciliation (§37, ADR-0193).** An adapted record never mints an identity
  the canonical provider would not: `enter` resolves it through the provider first, so
  `ps … | enter process` stands where `enter process <pid>` stands. A place keeps every source
  that observed it, exposed as `sources` on `ono.spatial-place/1` and `ono.spatial-neighbor/1`, so
  `inspect` on `lo` names `linux.netlink` **and** `adapter:org.ono.compat.iproute2.ip-link`. A
  whole-document adapter's batch is offered to the index; a stream is not buffered to index it.
  `… | enter` on bytes is `spatial.not_enterable`.
- **ADR-0191 — one `enter`, one refusal.** A failed `enter` is `spatial.not_found`
  (`Ono-Sendai-E1001`) in both grammars; `resolve.target_not_found` keeps every other job. Four
  assertions were adjusted to the new spelling and named in the commit body:
  `identity_missing::should_refuse_to_enter_a_user_that_does_not_exist`,
  `network_missing::should_refuse_to_enter_an_interface_that_does_not_exist`,
  `processes_missing::should_refuse_to_enter_a_process_that_does_not_exist`, and
  `docker/acceptance/cases/044-remote-links-as-objects.case` (`enter link` after `remove link`).
- **The TIME_WAIT flake above is fixed (ADR-0192)**, and it was a product defect rather than a
  fixture one: a socket in `time-wait` or `close` has no inode — `ono.socket/1`'s identity — so
  the index registered the kernel's 2MSL remnant as a *second* connection beside the one that had
  just ended, and `map` carried two nodes for one connection (the duplicate §37.1 and §42.1
  forbid). A released socket now has no place at all; `get socket` still lists it with its state.
  `spatial_relationships_missing::should_show_the_connection_edge_appear_and_vanish…` failed two
  runs in three before, and is green in four consecutive runs of its file after, unchanged.
- **Acceptance:** `docker/acceptance/cases/110-spatial-contributions.case`, 13 assertions
  (`s9-a`–`s9-g`, `s10-a`–`s10-f`). The two §4.7.1 boxes for §36 and §37 are ticked.

**Next up from this increment:**

- A contributed relation is an edge on the map and in the index, and is not yet a navigable exit:
  `look` does not print it and `follow`/completion do not offer it (ADR-0194 §Consequences). Exit
  test: `follow <contributed relation>` from a place the package's edge starts at moves there.
- `spatial_topology_missing::should_complete_the_relations_available…` still fails under parallel
  load and passes alone; the PTY budget in it is the fixture problem S6 recorded, not this.

**Found, not fixed, and deliberately outside this increment:**

- `network/addresses`, `compute/cgroups` and `network/namespaces` report `unsupported`: no v0.2
  provider target serves an address, a cgroup or a namespace as an object, although the bridge
  composes cgroups and namespaces from process records (S2, ADR-0135). Composing the collections
  from the same facts is a real increment, and §7.3 only requires the place to exist and to say
  what it could not tell. `storage/directories` reports `unknown — available on request`, because
  §33.3 makes the filesystem query-driven; S4d owns storage and the cwd distinction.
- `ono.system/1` is declared from §7.1 field for field, and `look --all` at the root carries it
  with `os`, `kernel` and `uptime` null: no provider answers for them, and §2.16 forbids the
  spatial layer from reading them itself. A `get system` producer fills them.
- `spatial_topology_missing::should_show_the_mounts_the_mount_provider_answers_for_when_entering_storage_mounts`
  compares two separate `ono` runs against a live mount table. On a workstation where Docker is
  creating and removing netns mounts it can lose the race and see a mount the first run did not.
  Seen once, passing on re-run and green in the container; the test is right and the environment
  is what moved.

**S4b — `enter` on any place, and `follow` along a real edge — is complete (2026-08-28, agent
`S4b`).** ADR-0146 to ADR-0149; gate green; acceptance case
`docker/acceptance/cases/103-spatial-enter-follow.case` added (31 assertions).

The increment turned on the thing S4a left dark: **an object place had no exits**. `near` at a
process answered nothing, because the only source of relationship edges was the record-field
bridge, which reads a `ppid` and a `cgroup` and cannot know which files a process holds open.
ADR-0146 makes the edges of an object place the ones the **v0.2 relationship providers** of
`ono-graph` assert about that object — the same providers `trace` walks — translated into the
declared relations of `docs/spec/spatial/relations.yaml`. A neighbour therefore reports the
relation word and the provider id `trace` reports for the same edge (§2.16, §31.3), and the
record-field bridge keeps only the relations no relationship provider serves (cgroup, namespace,
container, and the listener a connection was accepted by).

**What S4c needs to know** — the five things:

- **The trail is written and still unread.** Every movement records a `NavigationStep`:
  `enter`/`home` from S4a, and now `follow` with `.along(relation)` — §6.4's "the relation
  traversed MUST be recorded". `back`, `up`, `trail` and `jump` read what is already there.
  `crates/ono-cli/tests/spatial_relationships_missing.rs::should_record_the_relation_it_traversed_when_a_follow_enters_the_trail`
  is the test waiting on `trail --json`, and
  `spatial_topology_missing::should_follow_the_parent_relation_from_a_discovered_process_to_its_spawner`
  is green up to its last statement, which is `trail --json`.
- **`up` is `place.canonical_parent`, and it is already on every place view.** The place record
  carries `canonical_parent` (§11.3, §33.1) and no longer carries a second `parent` field
  answering the same question (ADR-0148). `ono_spatial_query::resolve::parent_of` computes it.
  `spatial_identity_missing::should_move_to_the_declared_canonical_parent_deterministically_when_going_up`
  asserts `up` lands on exactly that id and that `follow parent` lands somewhere else.
- **Resolution and observation are one function.** `crate::spatial::commands::resolved_place`
  resolves a selector the way §27.1 orders it and, when nothing visible answers, plans the
  provider targets the *selector* implies and asks those. `jump` is the same resolution with
  §27.1's step 6 allowed (`SelectorContext::across_links`) and a `Movement::Jump` step; it needs
  no new observation machinery.
- **A place view is still one function**, and it now carries what a movement needs to be checked
  from outside: `canonical_ref`, `lifetime`, `state`, `summary`, the `exits` map keyed by the
  word `look` prints, and the object's own identity fields at the top level, so
  `look --json | from json | where pid == 1842` is an ordinary pipeline (ADR-0148).
- **A refusal prints its dotted name.** `ono: Ono-Sendai-E1006 spatial.history_empty …` — the
  renderer shows both halves of §43's identity now, so `back` at an empty trail and `up` at the
  root are distinguishable in a terminal as well as in `catch e { $e.name }` (ADR-0148).

Green now, all previously `#[ignore]`d — 39 tests:

- `spatial_relationships_missing` (9): `should_enter_the_open_file_when_following_it_from_the_holding_process`,
  `should_name_the_holding_process_among_the_file_neighbors_when_the_file_is_the_place`,
  `should_name_the_same_relation_and_provider_as_trace_when_the_neighbor_is_the_open_file`,
  `should_enter_the_listening_socket_when_following_it_from_its_owner_process`,
  `should_reach_the_accepted_connection_when_following_it_from_the_listening_socket`,
  `should_refuse_the_traversal_with_no_relation_when_the_process_owns_no_socket`,
  `should_refuse_to_follow_a_canonical_child_that_is_not_a_relationship_edge`,
  `should_bound_the_neighborhood_by_default_and_widen_it_with_all`,
  `should_report_the_unreadable_namespace_group_as_unknown_rather_than_absent`.
- `spatial_navigation_missing` (8): the two `enter` tests, `should_traverse_the_relationship_edge_when_following_the_parent_relation`,
  `should_answer_no_relation_when_following_an_edge_the_current_place_does_not_have`,
  the three ambiguity tests, `should_leave_the_callers_place_untouched_when_a_called_script_navigates`.
- `spatial_identity_missing` (11): the four identity tests (287, 313, 334, 362), the three
  permission-honesty tests, `should_keep_every_relationship_parent_while_naming_one_canonical_parent`,
  `should_carry_source_provenance_and_confidence_on_every_relationship_edge`,
  `should_use_the_defined_confidence_vocabulary_and_never_call_an_inferred_edge_exact`,
  `should_expose_how_fresh_the_data_behind_a_place_is`.
- `spatial_storage_missing` (9): the six §30 tests (cwd, place, `cd`, `PWD`), the two §44.3
  walking tests, `should_refuse_a_path_that_does_not_exist_with_a_structured_error`.
- `spatial_topology_missing` (1): `should_discover_a_listening_socket_by_its_port_and_follow_it_to_its_owning_process`.
- `spatial_contracts_missing` (3): `should_refuse_an_unknown_place_with_a_structured_spatial_error`,
  `should_serve_every_relation_it_declares_and_declare_every_relation_it_serves`,
  `should_report_denied_information_as_denied_rather_than_as_an_empty_collection`.

**Left ignored, with the reason on the test** (each carries it in its `#[ignore]` line):

- `spatial_topology_missing::should_reach_a_process_it_never_names_…` and
  `…should_offer_the_process_exits_…` — **delivered and green with `--test-threads=1`.** The
  fixture selects its process with `ppid == std::process::id()`, and under cargo's default
  parallelism that also matches the children every other test in the same binary spawned, so the
  discovery walk reaches one of theirs. The fixture needs a predicate unique to itself.
  *(Fixed 2026-08-28 by agent `fixtures` — see below.)*
- `spatial_topology_missing::should_follow_the_parent_relation_…` — the `follow parent` half is
  green; the test's last statement is `trail --json` (S4c).
- `spatial_topology_missing::should_reach_a_running_service_…` — **the test and the inherited
  v0.2 contract disagree.** It selects with `--where state == "running"`, and `ono.service/1`
  declares `state` as `active | reloading | inactive | failed | activating | deactivating |
  unknown` and reports `running` as the *substate*. No service on a systemd host answers to it.
  In the acceptance container there is no service manager and the test takes its skip branch.
  *(Resolved 2026-08-28 by ADR-0167 — see below.)*
- `spatial_contracts_missing::should_refuse_an_ambiguous_selector_in_a_script_…` — the ambiguity
  path is delivered, but the fixture copies `/bin/sleep` to a new name and runs it twice; on a
  host whose coreutils is a multi-call binary (Ubuntu 25.10) the copy refuses to start
  (`coreutils: unknown program 'ono-spatial-twin'`), so nothing answers to the name and the
  refusal is `spatial.not_found`. The fixture needs a program it can rename.
  *(Fixed 2026-08-28 by agent `fixtures` — see below.)*
- everything that needs `back`, `up`, `trail`, `jump`, `pin` (S4c), `map` (S5), the mount
  boundary and the directory summary (S4d), or tombstones (S7).

**The three fixture-blocked v0.4 tests are delivered (2026-08-28, agent `fixtures`).** No
assertion was weakened; only the fixtures were corrected, per AGENTS.md §11. One commit,
ADR-0167.

- `spatial_topology_missing::should_reach_a_process_it_never_names_…` and
  `…should_offer_the_process_exits_…` — `SleepChild::selector()` now spells
  `ppid == <test pid> and pid == <child pid>`. Parentage alone matched every other test's `ono`
  shells in the same binary; the child's own pid is known to the fixture, and §9's "discovery
  without prior names" forbids naming the *object* (its command name), not pointing at one's own
  fixture. The walk is still `find place` → `enter @-1` → `look`. The same selector now serves
  `should_follow_the_parent_relation_…` and the `follow` completion test, which carried the same
  latent race.
- `spatial_contracts_missing::should_refuse_an_ambiguous_selector_…` — the twins are now a
  **symlink to `/bin/sh`**, not a copy of `/bin/sleep`. Two facts fix the fixture: the kernel
  takes `comm` (the `name` of `ono.process/1`) from the basename of the path handed to `execve`,
  symlink included, and it truncates it to 15 characters — hence `ono-twin-place`, not
  `ono-spatial-twin`. A *copy* additionally loses to `ETXTBSY` under parallelism, because a
  concurrent test's `spawn` inherits the copy's write descriptor across `fork`; a symlink leaves
  no descriptor. Each twin is `sh -c 'read line'` on a pipe the test holds, and the test waits
  for `/proc/<pid>/comm` before asking the shell to resolve the name.
- `spatial_topology_missing::should_reach_a_running_service_…` — ADR-0167: a running service is
  `state == "active" and substate == "running"`, held in the suite's `RUNNING_SERVICE` constant.
  `running` is a *substate* in `ono.service/1`, never a `state`; requiring both also keeps
  `active`/`exited` oneshots and `active`/`plugged` `.device` units — which have no process for
  §44.2's "follow one of its processes" — out of the selection.
- Proof: each file run ten times in a row under cargo's **default** parallelism, 10/10 green,
  in a clean worktree at `da26bba` carrying only these fixture changes.

**Found, not fixed, and deliberately outside this increment:**

- `process.connects_to` is declared and nothing serves it: the v0.2 graph reports a process's
  sockets, not its endpoints, and the endpoint at the far end is the *socket's* `peer`. The exit
  answers `unsupported`, which §35.2 makes a real answer; removing the relation or serving it is
  a decision for whoever writes the endpoint provider.
- `interface.has_address` has the same shape: `network/addresses` has no provider target, so an
  interface's `addresses` exit is `unsupported` rather than a list.
- A file place's `owner` is `unknown — available on request`: `user.owns_file` is
  `CostClass::Expensive` and no user record is observed at a file place. Loading it on
  `near --type user` is one line in `relations::adjacent_targets` and one test.
- **`cargo test` retains no results between statements of one `-c` script until now.**
  `stage_scope` did not populate `Scope::previous`, so `@-1` in a command argument resolved to
  null while `@-1` at the head of a pipeline worked. Fixed here because `enter @-1` is §28.2;
  every other command that takes a value argument gains the same reference.

**S4c — movement through history and hierarchy (`back`, `up`, `home`, `trail`, `jump`,
`pin`/`unpin`) — is complete (2026-08-28, agent `S4c`).** ADR-0150 to ADR-0153; acceptance case
`docker/acceptance/cases/104-spatial-back-up-home-trail.case` added (25 assertions).

The increment turned the trail from something written into something read, and fixed the one rule
that made §44.6 undemonstrable. Six commands are new — `back`, `up`, `jump`, `trail`, `pin`,
`unpin` — with their contracts in `docs/spec/commands/spatial.yaml`, their verbs in
`docs/spec/verbs.yaml` and one new schema, `ono.navigation-step/1`.

**What the next phases need to know:**

- **`trail` answers `ono.navigation-step/1`** (ADR-0150). §20.1's six fields, plus `from_ref`/
  `to_ref` (the `<type>/<key>` spelling a user can type back), `from_name`/`to_name`, `relation_id`
  beside the `relation` *word*, and `host`. `trail` streams the records, `trail --json` writes them
  as one array, `trail --compact` writes §20.2's breadcrumb. **S8** will need `host` to become
  per-step rather than the session's — it is set in one place, `movement::step_record`.
- **`scope_crossing` is already recorded and already rendered.** Every `jump` and `up` compares the
  scope of both ends and records the boundary where they differ, as a record with `kind`, `from`,
  `to`, `entering` and `remote`. **S4d**'s mount boundary (§44.3) and **S8**'s host boundary both
  need only the two ends to carry different scopes; nothing in the trail has to change.
- **A socket's canonical parent is `network.listeners`, not the process that owns it**
  (ADR-0151, a fix). The S1 rule chain made `up` from a socket land on the same place as `back`,
  which is precisely the distinction §44.6 exists to demonstrate. `parent_rules(Listener)` and
  `parent_rules(Connection)` are now empty and fall through to the collection space;
  `docs/spec/providers/linux-netlink.yaml` declares the same chain, because `spec-check` compares
  them. A socket's `place_path` is therefore `local/network/listeners`.
- **`still_a_place` in `crates/ono-cli/src/spatial/movement.rs` is the seam S7 needs** (ADR-0152).
  §20.3's four outcomes are all implemented — return, skip-with-a-notice, `spatial.destination_gone`,
  `spatial.history_empty` — behind one predicate that today answers "the session still knows this
  place". A tombstone makes that predicate answer differently and makes `back` return the tombstone.
- **A pin stores the place's *name* as its selector, plus its type** (ADR-0153). `jump @<pin>` reads
  what `with_pins` already resolved; a pin whose place is gone is `spatial.destination_gone` and
  stays in the store. **S5**'s landmark engine gets `user_pinned` from the same registry the query
  layer already ranks by; nothing new is needed there.

Green now, all previously `#[ignore]`d — 19 tests:

- `spatial_navigation_missing` (9): `should_move_across_scopes_and_record_both_ends_when_jumping_to_a_resolved_place`,
  `should_return_to_the_process_when_back_follows_the_navigation_history`,
  `should_move_to_the_network_hierarchy_parent_when_up_follows_the_canonical_hierarchy`,
  `should_return_to_the_system_root_when_home_runs_after_deep_navigation`,
  `should_answer_history_empty_when_back_runs_with_no_previous_place`,
  `should_answer_no_parent_when_up_runs_at_the_system_root`,
  `should_record_every_movement_with_its_kind_and_relation_when_the_trail_is_read_as_json`,
  `should_answer_not_found_when_a_navigation_argument_names_nothing`,
  `should_start_at_the_system_root_with_an_empty_trail_when_a_new_session_begins`.
- `spatial_contracts_missing` (4): `should_refuse_to_go_back_or_up_from_the_root_with_a_named_spatial_error`,
  `should_start_every_session_at_the_local_system_root`,
  `should_keep_a_scripts_navigation_out_of_the_callers_place`,
  `should_keep_the_trail_session_local_while_a_pin_survives_the_session`.
- `spatial_relationships_missing` (3): `should_return_to_the_process_with_back_after_following_a_socket_edge`,
  `should_leave_the_relationship_chain_with_up_after_following_a_socket_edge`,
  `should_record_the_relation_it_traversed_when_a_follow_enters_the_trail`.
- `spatial_identity_missing` (2): `should_move_to_the_declared_canonical_parent_deterministically_when_going_up`,
  `should_not_confuse_the_old_and_the_new_process_when_a_place_is_replaced`.
- `spatial_topology_missing` (1): `should_follow_the_parent_relation_from_a_discovered_process_to_its_spawner`
  — its last statement was `trail --json`.

**One assertion changed, with ADR-0151 in the same commit.**
`spatial_navigation_missing::should_move_to_the_network_hierarchy_parent_when_up_follows_the_canonical_hierarchy`
built its haystack from `display_name` and `scope`; under ADR-0140 the field that names the
canonical location is `place_path`, and `scope` is the §3.2 boundary (`host:web01`). `place_path`
is now in the haystack. What the test demands is unchanged: `up` lands under NETWORK and is not
where `back` lands.

**Left ignored, with the reason on the test:**

- `spatial_identity_missing::should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`
  — `back` returns to the recorded place and the trail keeps the record, but the test also demands
  that the place say it is dead, which is S7's tombstone (§10.3). Attempted and left.

**Found, not fixed, and outside this increment:**

- **`up` from a file place answers `spatial.no_parent`.** `parent_rules(File)` is `[path.parent]`,
  and `path.parent` is only supplied by `canonical_parent_with`, which `resolve::parent_of` does not
  call because only the caller knows which directories have been observed. §15.1 makes the enclosing
  directory a file's parent, so this is a real gap and it is **S4d's**: it needs the directory
  observed, which is the same query §15.4 and §44.3 need anyway.
- `docs/spec/schemas/file.v1.yaml` gives a file the identity `[device, inode]`, so a trail step's
  `from_ref`/`to_ref` for a file reads `file/0:46`. It is honest — that *is* the provider's
  reference — but it is not a spelling anyone types. Whoever gives `ono.file/1` a path-shaped alias
  fixes the trail's readability for free.

**S5 — semantic maps, the landmark engine and the ASCII fallback — is complete (2026-08-28, agent
`S5`).** ADR-0162 to ADR-0166; gate green; acceptance case
`docker/acceptance/cases/105-spatial-map.case` added (55 assertions).

Delivered:

1. `crates/ono-spatial-query/src/map.rs` — the `SpatialMap` projection: §23.1's ranking, §8.1's
   five zoom levels, §8.2's clustering, §8.3's expansion, the §34.2 budgets and the §6.9 filters.
   It is handed a *horizon* by the shell and asks no provider anything (§45.3, §2.16).
2. `crates/ono-spatial-query/src/landmark.rs` — **the landmark engine §50 assigns to no phase**
   (ADR-0163). Eight of §3.7's fourteen reasons are produced from real provider fields; the other
   six are documented absences, not silent branches.
3. `crates/ono-spatial-render/src/map.rs` — the default textual map of §23.2 as a ranked tree,
   width-aware, with the ASCII fallback §39.2 requires (ADR-0166).
4. `crates/ono-cli/src/spatial/map.rs` — the `map` command, its contract in `docs/spec/verbs.yaml`
   and `docs/spec/commands/spatial.yaml`, and five new schemas: `ono.spatial-map/1`,
   `ono.map-node/1`, `ono.map-edge/1`, `ono.map-cluster/1`, `ono.hidden-summary/1` (ADR-0162).
5. `spatial.map.node_budget`, `spatial.landmarks.*` and `spatial.look.change_window` are now
   *read* — `crate::spatial::configure_from` hands the session what the user configured, which is
   what makes §26.3's "inspectable and configurable" true rather than advertised.

Green now, all previously `#[ignore]`d — 26 tests:

- `spatial_map_missing` (21 of the 24; the three §24 tests were already green): the six §22
  contract tests, the two §43.2 filter tests, the four §8 zoom and cluster tests, `--focus`, the
  three landmark tests, and the three §23.2/§39 rendering tests.
- `spatial_contracts_missing` (2): `should_serve_exactly_the_canonical_spaces_the_registry_declares`,
  `should_bound_the_default_map_to_its_node_budget`.
- `spatial_navigation_missing` (1): `should_answer_a_bounded_graph_when_map_json_runs_without_a_tty`.
- `spatial_topology_missing` (1): `should_answer_look_near_and_map_without_an_object_name_when_at_the_root`.
- `spatial_identity_missing` (1): `should_resolve_every_edge_endpoint_to_a_node_or_an_explicit_off_map_endpoint`.
- `spatial_relationships_missing` (1): `should_explain_every_edge_with_relation_provider_and_confidence_when_mapping_a_process`.
- 17 new crate-level outcome tests: `crates/ono-spatial-query/tests/{map,landmarks}.rs`.

**Left ignored, with the reason on the test** (both in `spatial_map_missing`):

- `should_show_more_than_the_default_when_the_map_is_asked_for_all` — **its two halves contradict
  each other and the contracts suite.** The first (`--all` is strictly larger than the default) is
  delivered and green. The second asks that `--all` at a 300-process collection contain one
  particular freshly spawned process; `spatial_contracts_missing::should_bound_the_default_map_to_its_node_budget`
  requires `--all` to stay inside `spatial.map.node_budget` (100) and §34.2 prohibits unbounded
  rendering, so only a clock-relative ranking could reach it — and that makes the two §43.2 filter
  tests compare two maps of two different moments and fail. ADR-0165 carries the analysis under a
  `Spec deviation` heading. Reconciling it needs either a second, larger explicit bound or a
  fixture the map is guaranteed to rank in.
- `should_yield_exactly_the_members_and_keep_the_place_when_a_cluster_is_expanded` — **delivered
  and green with `--test-threads=1`.** It compares a cluster's member count from one `ono` run
  against the nodes a second run draws, and every sibling test in the binary spawns and reaps
  twelve processes between the two, so the collection it counts is a different size each time.
  Same family as the two topology fixtures S4b left.

**What S6 needs from this renderer** — the four things:

- **The seam is `spatial_map`'s input, not its output.** `ono_spatial_render::spatial_map(record,
  width, charset)` is the whole text projection; the full-screen view of §23.3 takes the same
  `ono.spatial-map/1` record — already ranked, bounded and clustered by `ono-spatial-query` — and
  adds a viewport, a cursor and the key bindings. It must not re-select or re-rank, or the two
  views will disagree about what the system looks like (§45.4, §49.5).
- **Focus is already a request, not a mode.** `MapRequest::focus(node)` goes in and
  `SpatialMap::focus` comes out beside `center`; moving the cursor is a new projection with a new
  focus and no movement of the place (§23.4). `Enter` on the focused node is `enter <id>`, which
  `crate::spatial::commands::resolved_place` already resolves.
- **The interactive budget is the same number.** §34.2's 100 nodes is `spatial.map.node_budget`,
  which `--all` already uses and `crate::spatial::configure_from` already reads.
- **Colour is S6's to add and no semantics may depend on it.** §39.1 lists six things colour must
  never be needed for; all six are carried by a word or a glyph today (`◆` for a landmark, `~~▸`
  for an inferred edge, the confidence word, the state word), and the ASCII/Unicode choice is
  `Charset`, decided from the locale and `TERM` in `crate::sink`.

**What S7 needs from this map projection** — the three things:

- **`live_capable` is `false` and says so honestly.** Nothing in this build subscribes to a
  provider event, so §25.1's live map has no source; S7 flips the field when it has one, and
  `map --live`/`map --changes` of §6.9 are declared in no contract until then.
- **`MapEdge.changed` and `MapNode` are ready for a change state.** The edge already carries a
  `changed` field (null today, §24.3 forbids inventing one), and the three change reasons of §3.7
  — `new_object`, `removed_object`, `connection_spike` — are exactly the ones ADR-0163 leaves
  undelivered because they are differences between two observations (§25.4), not facts about one.
- **Landmarks are recomputed on every `absorb`,** so a live update recomputes them for free; what
  S7 adds is the diff that makes a *change* visible, and the rule that a landmark asking for
  attention reorders the map while one that merely informs does not (ADR-0165).

**Found, not fixed, and deliberately outside this increment:**

- §26.2's high-memory rule cannot fire: no provider serves a host or cgroup memory budget, so a
  share cannot be computed and §2.16 forbids the spatial layer reading `/proc/meminfo` itself. The
  threshold setting stays inspectable. Same for the restart-loop rule: `ono.service/1` declares no
  restart count.
- §26.2 names four network rules — interface down, route change, unusually high traffic, new
  remote peer — that §3.7's closed reason vocabulary has no word for. A core landmark may not
  invent a reason (§3.7), so they are absent rather than approximated.
- Clustering has one dimension, the canonical collection (§8.2's first). A cluster by user, by
  cgroup or by container is a real increment with its own test; the dimension is already a field
  on `ono.map-cluster/1`, so adding one changes no contract.
- `map` honours `COLUMNS` even when stdout is redirected, which no other view does (ADR-0166).
  Whoever decides that the whole renderer should do the same has one function to change,
  `crate::sink::terminal_width`, and the table snapshots to re-check.

**S6 — the interactive spatial surface — is complete (2026-08-28, agent `S6`).** ADR-0173 to
ADR-0177; acceptance case `docker/acceptance/cases/107-spatial-interactive.case` added — 39
assertions driven through a real pseudo-terminal — and **the containerised suite stands at 73
cases green, 0 failed** (`scripts/acceptance.sh`, 2026-08-28).

Delivered — the phase, plus the four areas §50 assigns to nobody:

1. **§5 the startup horizon.** An interactive session runs `look` once before the first prompt
   and never in a pipe. It is `look`, not a second renderer of the root, so §49.5 cannot be
   broken by the two drifting apart (ADR-0175). `spatial.startup_horizon` and `spatial.enabled`
   each switch it off.
2. **§21 the prompt and the HUD.** `ono_spatial_query::resolve::concise_path` is §21.2's rule as
   a function — `local`, `local/compute`, `local/process/nginx` — and the prompt, the place
   view's heading and the map's header all read it. The working directory stays in the prompt
   beside the place, because §30 keeps them different state (ADR-0175).
3. **§23.3 the full-screen map.** `ono_spatial_render::view` holds the whole view model with no
   terminal in it: `MapView` (viewport, cursor, search, help, detail overlay), `Action` (§23.3's
   twenty-one semantic actions), `Keymap` (§23.3's table, overridable through the new
   `spatial.map.keys`), `Effect` (what is left for the shell to do). `crate::spatial::interactive`
   is the terminal side: alternate screen and raw mode as guards, resize, and the same
   `go_back`/`go_up`/`go_home` the commands call (ADR-0174).
4. **§27.2 the ambiguity picker,** interactive only; a script still gets
   `spatial.ambiguous_selector`. `Candidate` now carries the identity key, so §27.2's rows read
   `nginx/1842` and disambiguate rather than repeating one name three times — which also improved
   the non-interactive refusal (ADR-0177).
5. **§9.4 completion as spatial discovery.** `enter`/`jump`/`map <TAB>` offer the neighbourhood
   and `follow <TAB>` the relations this place actually has, with §9.4's compact count or §35.2's
   state word. Shown on the first Tab through the new `Completion::listing`; ordinary word
   completion is unchanged (ADR-0177).
6. **§25.1 `map --live`** as the explicit polling source §25.1 permits, saying `live polled` in
   §25.3's vocabulary, refused with `spatial.unsupported` where there is no terminal (ADR-0176).
7. **The seam that made all of it safe:** `ono_command::Invocation::displays` — the evaluator
   tells the last stage of a foreground statement that its values will be *seen*. `map | to json`,
   `map > file`, `$(map)` and `ono -c 'map'` therefore never open a screen (ADR-0173).

Green now, all previously `#[ignore]`d — 14 tests:

- `spatial_interactive_missing` (all 12): the horizon at a TTY and never in a pipe, `look` at 80
  and at 40 columns, the prompt following the place, the picker, the map opening and closing, focus
  that does not move the place, `back` at the prompt and Backspace in the view, Ctrl-C leaving the
  live map, resize preserving the place, `stty`/`pwd` in order afterwards, `enter <TAB>`.
- `spatial_topology_missing` (2): `should_complete_the_places_of_the_current_neighborhood_when_tab_follows_enter`,
  `should_complete_the_relations_available_from_the_current_place_when_tab_follows_follow`.
- 10 new crate-level outcome tests: `crates/ono-spatial-render/tests/view.rs`.

**Nothing was left ignored by this phase.**

**What S7 needs from this view loop** — the four things:

- **The loop is where a live update lands.** `crate::spatial::interactive::run_map_view` reads a
  terminal event with a timeout and, when the timeout expires and the view is live, rebuilds the
  projection and calls `MapView::redraw`. S7 replaces the *source* of that rebuild — an event
  subscription instead of a one-second poll — and changes nothing else: `redraw` already keeps the
  cursor on the node it was on, and `MapView::set_live(live, freshness)` already takes §25.3's
  word, so flipping `polled` to `event_driven` is one argument.
- **Nothing repaints unless the drawing changed.** The loop compares the new frame with the one on
  the screen and writes nothing when they are equal. That is what makes §39.4's `reduced_motion`
  true by construction today; when S7 adds a change highlight, `spatial.reduced_motion` is the
  switch that turns *that* off, and it is already read into the session (`configured_flag`).
- **`Effect` is the whole vocabulary between the view and the shell.** A new key means a new
  `Action` variant, a default binding and an `Effect`; the config syntax, the `?` help table and
  the key-name parser follow for free.
- **The map projection is one function.** `crate::spatial::map::projection(ctx, session, center,
  request, now)` builds every `ono.spatial-map/1` the shell emits — `map`, `map --json`, every
  frame of the full-screen view. A live diff belongs in what feeds it, never beside it.

**Found, not fixed, and deliberately outside this increment:**

- §5's "providers SHOULD populate expensive counts asynchronously and update the horizon when
  available" is a SHOULD this build does not do: the horizon is one synchronous `look`. It costs
  what `look` costs. Making it asynchronous needs the same update channel S7 builds for the live
  map, and belongs there — not in a second mechanism.
- `spatial.reduced_motion` is read and inspectable but has nothing to disable, because this
  renderer draws no animation at all (§25.2 forbids decorative motion). ADR-0176 says so; S7 gives
  it something to switch off.
- **§21.3's third marker has nothing to mark yet.** The section requires privilege, remote *and
  namespace* changes to be recognisable in a colourless terminal. Privilege is the ` root`
  segment and the `#` marker (v0.2 §17.2); remote is the link segment, which takes the host's
  name instead of `local` (§14.4). A container or namespace boundary cannot be shown because
  nothing produces a place in one: `ProviderBridge` projects every observation into the session's
  own host scope, so `ScopeKind::Container` and `ScopeKind::Namespace` exist in the model
  (`ono_spatial_core::scope`) and no place ever carries them. A marker written now could never
  fire. The prompt is one line away from it — compare the current place's scope with the
  session's and print `container:<id>` or `ns:<kind>/<id>` when they differ — and the increment
  that makes a container's processes carry the container scope is the one that should write it.
- The `/` search of §23.3 searches the *drawn* map. §23.3 says "search visible/global map"; the
  global half is `find place`, which already exists as a command, and wiring it into the view's
  search line is a real increment with its own test.
- Completion asks no provider anything (§34's 50 ms budget), so `enter <TAB>` inside a collection
  nobody has looked at offers the declared geography and no members. A background pre-observation
  of the current neighbourhood would fix it and is exactly the "background discovery" of §34.1.
- Two boxes in `docs/ACCEPTANCE.md` §4.7 — "full-screen map works on supported interactive
  terminals" and "PTY interaction tests pass" — now have all their *unit* proofs green, but both
  name case `099`, which is still `.case.v04`. They are S11's to tick with the rename.
- **`spatial_map_missing::should_only_remove_{edges,nodes}_...` are flaky under a parallel run,
  and were before this increment.** Both compare a map from one `ono` run against a map from a
  second; every sibling test in the binary spawns shells between the two, and a process that
  appeared in between is `recently_changed`, ranks into the second map and is absent from the
  first. They pass with `--test-threads=1` and failed about one gate run in three here; seen
  green in the gate before this increment and green again after it, and nothing in S6 touches
  map ranking. Same family as the two ADR-0165 defers and as the two topology fixtures S4b left.
  The fix is a fixture the two runs are guaranteed to agree on — not a change to either test.
- **The twelve PTY tests are load-sensitive, and the gate runs them under load.** All twelve pass
  in 47 s when `spatial_interactive_missing` runs on its own, and repeatedly; in a
  `cargo test --workspace` on a machine also running two other worktrees (load average 16 on
  8 CPUs) three of them exceeded their own 8 s screen budget waiting for `map` to open at
  COMPUTE, because opening the view costs one full projection — the same providers `map` asks,
  including systemd over D-Bus. Two things are true and both are worth writing down: the view is
  unresponsive while a projection is in flight, which §34.2's view budget will eventually have to
  answer for (S11 owns the budgets); and the picker's own fixture copies `/bin/sleep` and can hit
  `ETXTBSY` when a sibling test forks across the copy, which
  `spatial_contracts_missing::should_refuse_an_ambiguous_selector_in_a_script_rather_than_open_a_picker`
  already documents and avoids by using a symlink instead. Run the file alone before believing a
  failure in it.

---
**S8 — remote systems as space — is complete (2026-08-28, agent `S8`).** ADR-0168 to ADR-0172;
gate green; acceptance case `docker/acceptance/cases/106-spatial-remote.case` added (51
assertions, all proved locally against the real binary).

Delivered:

1. **A host's geography is its own** (ADR-0168). `ono_spatial_core::space` now keeps the
   geographies this process knows: `stand_in` moves into one, `learn` registers one without
   moving, `space_of_id` says which space an id names *and whose*. `SpatialIdentity::space_in`
   adds the host to a canonical space's identity for a remote scope and nothing at all for a
   local one, so every id built before S8 is unchanged and `testbox`'s `COMPUTE` is not this
   machine's. Twenty-odd call sites became host-correct without being edited.
2. **`jump <link>` crosses the boundary, visibly** (§19.2, §53). The destination is the linked
   host's root `SystemPlace`; the crossing is stated in words on stderr, so a colourless terminal
   sees it and a script's object stream stays objects; the trail step carries both ends and the
   `scope_crossing` naming the scope entered; the prompt takes the host in `local`'s place
   (§21.1, §21.3) whether `enter link` or `jump` put the session there.
3. **The session's host follows the place.** `enter`, `follow`, `up`, `back` and `jump` all call
   `SpatialSessionState::arrive_at`, which moves the geography, the provider bridge and — through
   `Session::pipeline_context` — the provider registry to wherever the place actually is (§14.4).
4. **Remote identity does not merge with local** (§43.7, ADR-0169). A remote scope is named by
   the *link*, never by what the far side calls itself, and its boot identity is honestly unknown;
   the provider bridge is per host, so its key memory cannot bridge a link; and §27.1 step 4 is
   now the *current host's* index, so `enter process/1` on `testbox` is not answered with the
   local pid 1 the index still holds.
5. **The link map** (§19.1). `ono.link-place/1` is a new contract, and `ono.place-view/1` gains a
   nullable `links` field, present at the root of a host. A link that is not connected stays in
   the map with the state that says so.
6. **A link that is gone is `stale`, never empty** (§35.2, §43.7, ADR-0171). `detach link` keeps
   its v0.2 meaning and adds one: this session stops *following* the link. Standing on such a
   host, `look` and `near` ask nothing at all — every exit is withheld `stale` with the link
   named, the place's `permission` and `freshness` are `stale`. That is not only about age:
   provider calls fall back to the local registry when no link is reachable, so asking would
   answer a question about `testbox` with this machine's objects.
7. **Provenance and confidence on every far-side relation** (§19.4, §11.4). A relationship edge
   observed across a link carries `Provenance::remote(provider, host, …)`, and so does the
   declared geography of a linked host — a remote observation is never indistinguishable from a
   local one.
8. **The federated map** (§19.3, ADR-0172). `map links` is its own command, `ono.place.map-links`,
   with the target word §19.3 writes; it draws this host's root beside every linked host's root,
   joined by `host.linked_to` edges whose confidence is the evidence's — `exact` for a link this
   session negotiated, `user_declared` for a definition nobody has connected. The default `map`
   mentions no linked host at all, which is §19.3's other half.

Green from `crates/ono-cli/tests/spatial_remote_missing.rs` — **all thirteen**, none left ignored:
`should_list_a_linked_host_among_the_places_when_looking_at_the_local_root`,
`should_give_a_linked_host_a_root_place_distinct_from_the_local_root`,
`should_announce_the_boundary_in_plain_text_when_jumping_to_a_linked_host`,
`should_mark_the_remote_host_in_the_prompt_after_a_jump`,
`should_record_the_host_and_the_scope_crossing_of_every_step_in_the_trail`,
`should_return_home_to_the_local_root_from_a_remote_place`,
`should_keep_a_remote_process_place_distinct_from_the_local_one_with_the_same_pid`,
`should_report_a_place_behind_a_detached_link_as_stale_rather_than_empty`,
`should_keep_a_detached_link_visible_with_its_state_in_the_link_map`,
`should_carry_provenance_and_confidence_on_every_relation_that_comes_from_the_far_side`,
`should_refuse_to_jump_to_a_hostname_that_is_not_a_known_link`,
`should_not_expand_a_remote_graph_into_the_default_root_map`,
`should_show_the_linked_hosts_when_the_federated_map_is_asked_for`.

**Two RED tests of this tranche contradict each other, and S7 owns the other one.** ADR-0170 has
the trace in full. `spatial_remote_missing::should_return_home_to_the_local_root_from_a_remote_place`
is only satisfiable if `home` does not push the place it left onto the stack `back` walks;
`spatial_identity_missing::should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`
(still ignored, assigned to S7) is only satisfiable if it does. The two scripts are structurally
identical — `L → P → home(L)` against `L → T → C → home(T)` — so no rule about `home` alone
satisfies both. S8 implemented the S8 reading (`Movement::Home.extends_history() == false`, on the
same argument that already made `back` not a toggle) and recorded the collision. **Whoever
un-ignores the tombstone test reads ADR-0170 first.**

**What S9 and S10 need from this phase** — the three things:

- **A place's host is `SpatialSessionState::current_scope()`, not `scope()`.** The latter is the
  machine the shell runs on and never changes; the former is the host the session is standing on.
  Anything that projects, ranks or signs an observation wants the second.
- **`crate::spatial::links` is the only place that answers "may I cross this link".** Both
  `Session::pipeline_context` (which registry answers) and the spatial views (is this place stale)
  read `links::reachable`. A plugin space or an adapted object that lives on a linked host asks
  the same function.
- **The link name is the scope id.** `remote_host:<link>` is the whole identity of a remote scope
  (ADR-0169), so an adapter or a plugin that wants to place a remote object composes its scope
  from the link name and nothing else.

**Found, not fixed, and deliberately outside this increment:**

- §19.1's link map has no latency and no "last seen": `12ms` and `last seen 3h ago` are in the
  spec's own example, and nothing in this build measures either. `ono.link-place/1` carries no
  field for them rather than a null one nobody fills.
- §19.4's *genuinely* two-sided cross-host correlation — a connection whose remote endpoint maps
  to a linked host (§14.5) — is not built. What is built is the honesty requirement that holds for
  every far-side edge: it says who observed it, from where, and how sure it is. The richer fixture
  §43.3 asks for needs two hosts with a real connection between them, which an unprivileged
  offline container cannot make.
- A neighbour reached by canonical hierarchy rather than by an edge still carries a null
  `confidence` and a null `provider`: there is no relationship to explain, and §2.6 forbids
  inventing one. Every neighbour of a *process* has an edge, which is why the §19.4 test passes;
  a place whose exits are collections would show the nulls.
- `map links` draws one hop. §19.3's picture has `prod/web01 ----- prod/db01`, a link between two
  *remote* hosts, which this session cannot observe: it would have to ask `testbox` for its own
  link table, and nothing in the protocol carries one.
- Two links to the same machine under two names are two scopes and therefore two sets of places.
  That is a false distinction rather than a false merge, and §2.17 prefers it — but a session that
  does it will see the same process twice.

---

**S7 — live topology, tombstones and the change section — is complete (2026-08-28, agent `S7`).**
ADR-0178 to ADR-0181 and ADR-0184; acceptance case `docker/acceptance/cases/108-spatial-live.case`
added (23 assertions, dry-run against the real binary and the real fixtures).

Delivered:

1. `crates/ono-spatial-events` (§45.5) — the change model, §25.3's freshness vocabulary, the
   §25.4 snapshot comparison, the event merge over the v0.2 watch envelope, and §26's landmark
   recalculation trigger. It reaches no provider, no terminal and no clock (ADR-0178).
2. **Tombstones** (ADR-0179). A place becomes one when a provider that was asked about it does not
   answer for it — and only then: `io.not_found` is the object saying it is gone, every other
   error is a reading failure, which §35.2 forbids rendering as absence. The index keeps the entry
   (the identity is what tells a tombstone from a place that never existed), its lifetime closes,
   and the relationships nobody asserts any more are dropped from both ends. `look`/`near`
   describe it, `back` arrives at it, `follow` and `enter` refuse with `spatial.destination_gone`.
   `spatial.tombstone.lifetime` (1m) is what "short-lived" means.
3. **`map --live`** (ADR-0180) through the v0.2 watch runtime rather than a second one
   (`ono_command::watch_events`, §2.16). It waits on events, drains a moment before drawing it,
   re-projects through the still `map`'s own path, and emits only a difference. `live_capable` is
   now answered rather than assumed; every value carries `live`, `freshness`, `change_source` and
   the `ono.spatial-change/1` list §45.5 calls the live map update message.
4. **`look --changes`** (ADR-0181) — the §25.4 comparison against what this session last saw
   around the place, with §24.3's three distinct answers: `unknown` (no baseline), `empty` (a
   baseline and no difference), `available` (the differences). It compares the *complete*
   neighborhood, because comparing the ranked one reports the ranking as change.
5. **`home` extends the navigation history** (ADR-0184), settling the conflict between
   `spatial_identity_missing::should_return_the_tombstone_…` and
   `spatial_remote_missing::should_return_home_to_the_local_root_from_a_remote_place`. §20.1 writes
   a step for `home` and §2.4 makes every movement reversible, so `back` returns through it.
   ADR-0170 is superseded on that point; **the remote test's assertions are unchanged** and only
   the number of `back`s it spends walking its own history moved from two to three.

Green now, all previously `#[ignore]`d — 5 tests:

- `spatial_identity_missing` (4): `should_report_a_tombstone_rather_than_a_live_place_when_the_visited_process_has_exited`,
  `should_refuse_to_traverse_a_relationship_when_the_place_is_a_tombstone`,
  `should_never_resolve_a_tombstoned_place_to_a_live_object`,
  `should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`.
- `spatial_relationships_missing` (1): `should_show_the_connection_edge_appear_and_vanish_when_the_connection_opens_and_closes`.
- 20 new crate-level outcome tests: `crates/ono-spatial-events/tests/{snapshot_comparison,event_merge}.rs`
  (15), `crates/ono-spatial-core/tests/trail.rs` (2), `crates/ono-spatial-index/tests/index.rs` (3).

**Left ignored, with the reason on the test:**

- `spatial_identity_missing::should_distinguish_a_tombstone_from_a_place_that_never_existed` —
  §40's two conditions are delivered and distinct, and the `gone` half of the test passes. The
  `never` half asks for two things this increment does not owe, and ADR-0179 §Spec deviation
  carries both: `enter <target> <identity>` keeps v0.2 §14.3's `resolve.target_not_found` for an
  identity nothing answers to (`identity_missing::should_refuse_to_enter_a_user_that_does_not_exist`
  pins `Ono-Sendai-E0102`), and the script's exit status is its last statement's under ADR-0008,
  while the refused `enter` leaves the place where it was, so the following `look` succeeds.
- `spatial_interactive_missing::should_keep_the_shell_alive_when_ctrl_c_ends_the_live_map` — it
  asserts the alternate screen goes on and off around `map --live`, which is **S6's full-screen
  view**. S7 delivers the live *stream* and the change model; the view that renders it and the
  key that leaves it are S6's, and the test is theirs to un-ignore.

**Found, not fixed, and deliberately outside this increment:**

- **A TIME_WAIT socket has inode 0, and `ono.socket/1`'s identity is `[inode]`,** so every
  TIME_WAIT socket on a host reconciles into one place whose label is whichever record was
  absorbed last. Visible in a live map as a connection node that "appears" carrying an unrelated
  peer. It is a v0.2 identity contract question (which fields make a socket that socket), not a
  spatial one — exit test: two TIME_WAIT sockets are two places.
- An unbounded stream must be bounded and serialised to reach stdout (v0.2 §18.3), and `to json`
  collects, so `map --live --json | take N | to json` prints nothing at all if it is cut off
  before the Nth value. A streaming serializer — `to jsonl`, or `to json` forwarding one document
  per value on an unbounded stream — would make a live view scriptable without knowing in advance
  how many changes to wait for. Exit test: `map --live --json | take 100` prints its first value
  before the second arrives.
- `spatial_map_missing::should_only_remove_{edges,nodes}…` fail on a loaded host and pass on a
  quiet one: each compares two `ono` runs over the whole process collection, so a process started
  between them is a node the earlier map does not have. Same family as the cluster-expansion test
  S5 left; they failed identically on this tree before S7 touched it.

**S4d + S4e — the storage remainder and the configuration behaviour — are complete
(2026-08-28, agent `S4de`).** ADR-0185 to ADR-0188; gate green; acceptance case
`docker/acceptance/cases/109-spatial-storage.case` added (25 assertions) and
**`scripts/acceptance.sh` stands at 74 passed, 0 failed**.

Delivered:

1. **§47 the switch.** `spatial.enabled = false` leaves the typed shell and ordinary commands
   working and answers every `ono.place.*` verb with `spatial.unsupported` (Ono-Sendai-E1009) —
   a named refusal a script can branch on, not a command that vanished, which matters because
   `look` shadows util-linux `look` (ADR-0185). One guard at the point the shell binds a native
   stage, foreground and background alike. The setting is read from the *live* session settings,
   not the `spatial.*` snapshot, because it is the one key whose purpose is to be flipped. The
   spatial side effects of ordinary commands stop with the verbs: `enter <target>` still pushes
   its v0.2 context frame and no longer moves the place, `cd` no longer synchronises one, and
   §9.4's completion offers no neighbourhood.
2. **§33.1/§34 the warm view.** The session remembers what each provider *query* answered and
   when; a command inside that target's §33.3 lifetime reads it back instead of asking again
   (ADR-0186). The lifetime is the index's own TTL policy over the kinds of place the target
   produced, shortest first. `look --json`'s `freshness` is now `cached` when every target was
   recalled and nothing was asked — §25.3's own word — and stays `polled` where it did ask.
   Marginal cost of a repeated root `look` in a **debug** build on a loaded machine: ~70 ms →
   ~44 ms, with no provider asked at all in the repeat. S11 owns the number as a release gate.
3. **§15.3 the mount boundary as a place.** `ono.place-view/1` carries a nullable `boundary` of
   the new `ono.mount-boundary/1` — local path, filesystem, source, `remote`, plus `read_only`
   and the mount's `spatial_id` — every field composed from what `get mount` answered (§2.16),
   and `ono-spatial-render` prints the block §15.3 draws. `remote` is decided from the filesystem
   type and the shape of the source, conservatively in both halves (ADR-0187).
4. **§3.2/§2.18 the crossing.** `movement::crossing_between` asks the two places' own scopes
   first — a host or a container must not be understated — and only then whether the two paths
   sit on different mounts, recording a `filesystem` `ScopeBoundary` that does not claim to have
   left the host. `enter`, `jump` and `up` all go through the one function.
5. **§15.1 the path tree keeps its shape.** `parent_rules(Directory)` is now
   `[path.parent, mount.backs_directory]`, with `docs/spec/providers/linux-procfs.yaml` saying
   the same. §15.1 is unconditional, so the parent of `/mnt/backup` is `/mnt`; the mount is
   where the path tree runs out (`/` has no directory above it), which is where §15.2's
   MOUNTS -> DIRECTORY ROOTS meets the Unix tree. Recorded as a **spec deviation** in ADR-0187.
6. **§15.4 the directory place.** Children are hierarchy, not a relation (§3.4):
   `SpatialIndex::path_children` is the reverse of `set_path_parent` and the neighbourhood puts
   them first. The **read is whole and the view is bounded** — a 400-entry directory counts four
   hundred, shows eight and says "392 more not shown" (ADR-0188). `storage::observe_place_at` is
   the one seam every path spelling reaches: the object, the mount table, and the enclosing
   directory — which is what makes **`up` from a file** reach it, the gap S4c left open.

Green now, all previously `#[ignore]`d — 14 tests:

- `spatial_storage_missing` (3, the suite is now fully green):
  `should_show_the_source_device_and_filesystem_when_the_place_is_a_mount_boundary`,
  `should_record_the_boundary_crossing_when_traversing_from_the_root_into_a_mounted_directory`,
  `should_summarize_a_large_directory_instead_of_enumerating_it` — the last of which passed
  before because nothing was ever read, and passes now because a bound was applied.
- `spatial_contracts_missing` (7): `should_keep_the_typed_shell_working_when_the_spatial_layer_is_disabled`,
  `should_answer_repeated_looks_far_inside_the_look_budget`, and **five S4 tests that were
  delivered by S4b/S4c and left `#[ignore]`d by mistake** — `should_refuse_to_go_back_or_up_from_the_root_with_a_named_spatial_error`,
  `should_start_every_session_at_the_local_system_root`,
  `should_keep_a_scripts_navigation_out_of_the_callers_place`,
  `should_keep_the_trail_session_local_while_a_pin_survives_the_session`,
  `should_resolve_repeated_observations_of_one_object_to_the_same_spatial_id`.
- `spatial_navigation_missing` (2, the suite is now fully green):
  `should_stream_neighbors_that_compose_with_the_pipeline_when_near_runs_in_a_script`,
  `should_keep_running_external_commands_when_spatial_navigation_has_happened`.
- `spatial_topology_missing` (1, the suite is now fully green):
  `should_follow_the_parent_relation_from_a_discovered_process_to_its_spawner` — its `#[ignore]`
  said "un-ignored by the increment that delivers `trail`", and S4c delivered it.

Each of the eight late un-ignores was run twice on its own before the ignore was removed.

**Still ignored across the nine spatial suites at this commit — 6 tests, none of them S4's**
(S7's tombstones and S8's remote federation landed in the integration between S4d's work and its
rebase, and un-ignored the rest):

| Suite | Test | Owed by |
|---|---|---|
| contracts | `should_keep_a_package_relation_out_of_the_map_until_its_capability_is_granted` | S9 |
| contracts | `should_carry_the_contributing_package_as_the_origin_of_every_plugin_edge` | S9 |
| contracts | `should_reconcile_an_adapted_object_with_its_native_twin_into_one_place` | S10 |
| contracts | `should_never_let_raw_command_output_become_a_place` | S10 |
| identity | `should_resolve_the_adapter_view_and_the_native_view_of_one_process_to_one_spatial_id` | S10 |
| identity | `should_distinguish_a_tombstone_from_a_place_that_never_existed` | S7 |

`spatial_storage_missing`, `spatial_navigation_missing`, `spatial_topology_missing`,
`spatial_map_missing`, `spatial_relationships_missing`, `spatial_remote_missing` and
`spatial_interactive_missing` carry no `#[ignore]` at all.

Two of the S9/S10 tests pass when run with `--ignored`. They were left alone on purpose: a test
that passes because the condition it describes cannot arise yet is not delivered, and S9/S10
should be the increments that decide it.

**Found, not fixed, and deliberately outside this increment:**

- ~~`… | select <field> | to text` refuses with `Ono-Sendai-E0201`~~ — **fixed by S11a**
  (`fix(data)`): a record `select` has narrowed to one field is that field's line, and
  `get mount | select target | to text` prints one path per line. `--field` still projects a
  dotted path or one field out of a full record, and a record of several fields is refused
  exactly as before.
- `spatial_map_missing::should_only_remove_{edges,nodes}_…` failed about one gate run in three
  here, as S6 already recorded; they are green with `--test-threads=1`. **A gate run on this
  machine now needs `RUST_TEST_THREADS=1` to be reliable**, and that is a fixture problem in
  those two tests, not a harness one.
- **`ono-sendai:acceptance` is one image tag shared by every worktree.** A concurrent
  `scripts/acceptance.sh` in another worktree overwrites it, and a later `--no-build` run then
  tests the *other* agent's binary — which cost an hour here before it was spotted. Set
  `ONO_ACCEPTANCE_IMAGE=ono-sendai:acceptance-<agent>` while several agents share a machine.
- `options_and_selectors_missing::should_trace_nothing_else_when_no_connection_has_the_requested_remote`
  fails whenever *something else on the machine* holds a socket to 192.0.2.1 — a sibling
  worktree running `test port 192.0.2.1 443` does exactly that, and the connection stays
  `syn-sent` for two minutes. The test's premise ("this machine holds no connection to it") is
  the thing that broke, not the shell. It is green on an idle machine.
- §15.4's other optional neighbours are not delivered and say so rather than showing zero:
  `open-by processes` needs an `lsof`-shaped provider, `owned-by users` is an expensive relation
  nobody has asked to load, `changed recently` is a snapshot difference (§25.4).
- §8.2 clustering of directory entries — grouping them by kind or by name instead of counting
  them — is the next increment on top of ADR-0188; the field it would fill already exists on
  `ono.map-cluster/1`.
- An object place (a process, a socket, a directory) still expands its relationship providers on
  every `look` and honestly says `polled`. Caching relationship edges is a later increment with
  its own test; §34.1's background discovery needs the update channel S7 builds.

**S11 — release hardening: the ten §44 acceptance scenarios — is complete (2026-08-28, agent
`S11a`).** The ten `docker/acceptance/cases/09x-spatial-*.case.v04` files are renamed to `.case`
and the referee runs all 87 cases green, twice in a row. Ten commits, gate green:

| Commit | What it fixes | Proof |
|---|---|---|
| `fix(data)` | `… \| select f \| to text` refused a record `select` had narrowed to one field | `to_text` renders a one-field record; exit test `get mount \| select target \| to text` |
| `fix(spatial)` collections | a collection said `unsupported` while the index held its members (ADR-0197) | `spatial_contracts_missing::should_show_a_place_only_an_adapter_observed…` |
| `fix(spatial)` permission | a denied path was reported as missing, and an unreadable directory became the cwd (ADR-0198) | `spatial_identity_missing::should_refuse_a_path_this_user_may_not_read…` +2 |
| `fix(spatial)` paths | `enter /srv/app/..` made a cycle in the path tree and the next `look` overflowed the stack (ADR-0199) | `spatial_storage_missing::should_stand_in_the_directory_a_path_names…`, `ono-spatial-query::resolution::should_answer_a_place_path_rather_than_looping…` |
| `fix(spatial)` evidence | an edge said who observed it and never what they saw (ADR-0200) | `spatial_relationships_missing::should_carry_the_raw_evidence_of_an_edge…` |
| `fix(spatial)` find | `find place --where` read the providers and not the index (ADR-0201) | `spatial_contracts_missing::should_find_a_place_by_its_properties…` |
| `fix(spatial)` find record | a search result left `state` and the §24.1 summary null where `look` filled them | `::should_describe_a_search_result_and_a_place_view_with_the_same_record` |
| `fix(spatial)` relations | a relation §32.1 declined for cost was reported as one nobody serves | `spatial_relationships_missing::should_say_a_costly_relation_is_unknown…` |
| `fix(shell)` cwd | `cd` did not move the process, so `find file .` walked the launch directory | `builtins::should_change_the_directory_a_native_command_sees_when_cd_has_run` |
| `fix(spatial)` denial | a map of a denied place called itself `complete`; `find --near <path>` never reached the filesystem | `spatial_identity_missing::should_not_call_a_map_complete…`, `::should_refuse_a_search_anchored_on_a_path…` |
| `feat(spatial)` listeners | §13's `listeners` group was missing from a service place | `spatial_relationships_missing::should_offer_the_listeners_of_a_service…` |

ADRs: 0197 (a collection shows the places it holds), 0198 (denied is not missing), 0199 (one
directory however the path spells it), 0200 (an edge carries what the provider saw), 0201 (`find`
searches the index too).

**Found by S11a, not fixed, and recorded rather than faked:**

- **A tombstone's `replacement:` candidate (§10.3's example, §40's "actionable next steps") is
  never computed and answers `null`.** The field is on `ono.spatial-place/1`'s `tombstone` record
  and `Tombstone::replaced_by` exists; nothing calls it. It cannot be answered at the moment the
  old place ends: the source of the relation that reached it — the unit that controlled the
  process — has not been observed again, so the index holds no candidate to name. Two honest
  routes: re-observe that one source when a tombstone is rendered (a targeted query, not an
  enumeration), or fill the tombstone lazily when a later observation records an edge from the
  same source by the same relation to a live object of the same kind. Offer a candidate only when
  that source reaches **exactly one** such object — a choice among several is a guess, not a
  candidate (§2.17, §53). Exit test: after the §44.7 restart, `look --json` at the tombstone
  carries `tombstone.replacement` equal to the new process's `spatial_id` and
  `replacement_via` naming `service.controls_process`; `docker/acceptance/cases/096-…` `44.7e`
  then asserts it instead of what it asserts now.
- **`enter process <pid>` answers `spatial.not_found` for a process started with `setsid`.**
  **Corrected by S11b: this does not reproduce — the report's `$!` is `setsid`'s own pid, and
  `setsid` exits at once. See the S11b section below.** As reported:
  Reproducible: `setsid sleep 60 & ono -c "enter process $!"`. The same pid entered without
  `setsid` resolves. A session leader in its own session is an ordinary process and §12 makes it
  a place; the selector or the provider query is filtering on something it should not. Exit test:
  a `setsid`-started process is enterable by pid.
- **Two gate runs in a row went red on tests whose premise the machine broke, and green on the
  next.** `spatial_topology_missing::should_complete_the_relations_available_from_the_current_place_when_tab_follows_follow`
  waits 8 s for the completion after a walk it recognises by its own *echo*, so a busy host makes
  it wait for a walk that has not run yet — S6's note about it is exact and still true.
  `::should_show_the_mounts_the_mount_provider_answers_for_when_entering_storage_mounts` compares
  the mount table two `ono` processes saw, and the acceptance containers running beside it were
  mounting and unmounting overlayfs between the two. Both are green on an idle machine and both
  are premises about the host rather than claims about the shell. Neither is worth weakening; the
  first would be sound if it waited for the *place* rather than for the echo.
- **`ono.socket/1` gives a listener and its accepted connection the same `follow socket` word.**
  §12's "`follow socket :443` MUST traverse to the matching socket" is served, and bare
  `follow socket` on a process holding both is `spatial.ambiguous_selector` — correct, and worth
  knowing before writing a case that assumed one socket.

**S11b — the rest of v0.4 §52: budgets, evidence, the security review, dogfooding and the
checklist — is complete (2026-08-28, agent `S11b`).** Eight commits, gate green on each; the
container ran the new case on image `ono-sendai:acceptance-s11b`.

| Commit | What it delivers |
|---|---|
| `test(spatial)` | the racy half of `should_complete_the_relations_available…` — it read until `parent` was on screen and then asserted `user` in the same breath, which is why the board carried it as "fails under parallel load" |
| `fix(spatial)` | a map filter narrows the bounded map instead of re-selecting it (ADR-0202) |
| `test(spatial)` | the §43.5 renderer snapshots at 40/80/120/200 columns, and §34's 16 ms frame budget at a real PTY |
| `docs(decisions)` | ADR-0203, the spatial enumeration review: ADR-0015's table extended with seven rows, each naming a passing test |
| `test(xtask)` | `xtask/tests/spatial_evidence.rs`, the guard that keeps §4.7 from rotting |
| `test(acceptance)` | `docker/acceptance/cases/100-spatial-performance-budgets.case`, the §34 budgets at their real figures |
| `feat(help)` | `help spatial` (§38.1, a MUST that was missing), found by dogfooding |
| `docs` | `docs/dogfood/v0.4-2026-08-28.md`, and §4.7 ticked from the evidence |

**The §34 budgets, measured in the container on the §43.3 fixtures.** None is violated, so no ADR
documents a violation:

| Budget (§34) | Measured |
|---|---|
| interactive startup to usable prompt < 150 ms | 0 ms over `bash` under the same `script(1)` harness (272 ms both) |
| basic `look` local cached < 50 ms | 178 µs per repetition |
| `near` cached < 50 ms | 343 µs |
| map L0/L1 cached < 100 ms | 334 µs |
| map L2 ordinary host < 250 ms | 472 µs |
| search common indexed objects < 100 ms | 1 803 µs |
| focus/navigation in a rendered map < 16 ms/frame | 88 µs median at a real PTY (slowest 386 µs) |
| §34.1 discovery does not block the prompt | unchanged at 0 ms with 200 extra processes and a 20 000-entry directory |

The startup figure is measured **against a baseline of the same harness running `bash`**, because
`script(1)` costs about 270 ms of its own in that image — `bash` under it takes as long as `ono`
does, to the millisecond — so an absolute figure would be a measurement of the harness. A whole
non-interactive `ono -c true` run takes 18.5 ms there.

**Found by dogfooding (`docs/dogfood/v0.4-2026-08-28.md`), one fixed, the rest under *Next up*.**
The honest verdict on §52.3's statement is in that file: it holds for orientation and hierarchy
and breaks at the first permission boundary, because a group the provider answered `null` for is
rendered as `0` rather than as unknown.

**Two entries on this board are closed by S11b's own evidence:**

- **The bounded/filtered map defect is fixed** (ADR-0202). It now has a deterministic reproducer
  rather than a host-dependent one: `ono-spatial-query::properties::should_keep_every_node_and_edge_a_filter_left_alone_and_invent_none`
  is red at seed 1 on the old projection.
- **"`enter process <pid>` cannot reach a process started with `setsid`" does not reproduce.** Its
  reproducer — `setsid sleep 60 & ono -c "enter process $!"` — records `setsid`'s own pid, and
  `setsid` forks and exits immediately, so the pid looked for belongs to a process that is gone.
  Started properly (`setsid tail -f /dev/null &`, then the child's real pid) `find place --type
  process --where pid == <pid>` finds it and `enter <pid>` enters it. No defect; the entry is
  removed.

**One thing S11b made slightly worse and did not hide.** The interactive suite gained a
thirteenth PTY test (the frame budget), and one full-workspace gate run then failed
`should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open`, whose 8 s
budget for closing a full-screen map of COMPUTE (500 processes) is tight when several PTY
sessions run beside it. The file is green four runs in a row on its own and green on the
following full gate. It belongs to the same family as the two host-premise flakes S11a recorded,
and the fix is theirs: wait for the *place*, not for a byte count.

**S11c measured that family and closed it.** The picker test joined it:
`spatial_interactive_missing.rs::should_open_a_picker_and_make_the_choice_current_when_a_selector_is_ambiguous`
failed roughly one run in four **with and without** that session's changes — four runs at
`079aa98` gave one failure, three runs with the working tree on top gave one — so it was a
premise about the host, not a claim about the shell, and two full gate runs in a row died on it
and on the resize test. A referee that fails one run in two is not a referee (AGENTS.md §14), so
the premise was fixed rather than the flake tolerated: `BUDGET` and `STARTUP` in that file are
**liveness bounds, not performance assertions** — they exist so a screen change that never comes
fails instead of hanging — and they are now 45 s and 60 s. No assertion changed, and the file
still finishes in 14.7 s, because a bound that is never reached costs nothing. The §34 figures
are asserted where they belong and are untouched:
`::should_repaint_a_focus_move_far_inside_the_frame_budget_when_the_map_is_open` (16 ms per
repaint) and `docker/acceptance/cases/100-spatial-performance-budgets.case`.

What that leaves standing is the observation underneath, which is about the shell and stays on
this board: **opening a full-screen map of COMPUTE on a 500-process host is unresponsive while
one whole projection is in flight**, which §34.2's view budget will eventually have to answer for.

A fourth member of the family, same treatment:
`spatial_remote_missing.rs::should_refuse_to_jump_to_a_hostname_that_is_not_a_known_link` gave the
run ten seconds to refuse `jump prod/web01.invalid`, and the refusal costs eight of CPU in a
debug build on this host — measured at 10.02 s on `76adb95` and 8.6 s with S11c's changes, so it
was the machine it raced, not a resolver. What proves nothing was dialled is the error name
(`spatial.not_found`, never a resolve or connect failure); the budget is only the hang guard, and
it is 60 s now.


## Next up (ordered)

- [ ] **`get process | count` exits 1 on a busy host, and a test premise depends on it not to.**
  A process the enumerator listed exits before its `/proc/<pid>/stat` can be read, so v0.2 §9's
  partial-failure semantics report `Ono-Sendai-E0401 provider.unavailable /proc/<pid>/stat: No
  such process` and the run exits 1. The *shell* is right; what is wrong is
  `spatial_topology_missing.rs::should_bound_the_root_horizon_instead_of_listing_every_known_object`
  (and `remote_missing.rs::should_answer_again_from_a_detached_link_when_it_is_entered_again`),
  which assert a successful run of a command that enumerates every process. Seen once in a gate
  run and once in a `release-check` run by S11c, green on the next of each. Exit test: those two
  tests are green on a host that is churning processes — by asking a bounded question, or by
  asserting the answer rather than the exit status.
- [ ] **Every read-only mount is a "storage pressure" landmark (§26.2, §2.11).** At STORAGE the
  landmark list is twenty snaps at `100% used`; a squashfs image is full by definition. Exit
  test: a read-only filesystem at 100 % is not a storage-pressure landmark, and a writable one
  above the threshold still is.
- [ ] **A map of an object is a flat list with duplicate labels (§23.5, §11.4).** `map` at a
  desktop process draws `also here` with eight rows all called `/run/user/1000/wayland-0`, one
  row called `4026531836` (an unlabelled pid-namespace inode) and no relation on any of them, so
  the view cannot be used to choose a neighbour. The root map is unaffected and is excellent.
  Exit test: every row of an object map names the relation it stands in, and two neighbours that
  share a display name are distinguishable in it.
- [ ] **`cpu` is `null` in every one-shot run (§26.2's high-CPU landmark, §34).** CPU is a rate
  and `ono-provider-linux` has nothing to subtract from on a first observation, so all 493
  processes answer `null` from `ono -c` and `find place --where cpu > 5` is silence; a second
  `get process` in the same session does produce values. Honest, and it means "what is busy?"
  cannot be asked non-interactively. Exit test: a single `ono -c "get process | where cpu > 0 |
  count"` answers more than zero on a busy host, by whatever route — a second sample after a
  short interval, or a separate lifetime-average field.
- [ ] **`help here` (§38.2) does not exist.** A SHOULD: "at any place, `help here` shows the
  spatial operations supported by that place". `help spatial` (§38.1, a MUST) was delivered by
  S11b; this is its other half. Exit test: `help here` at a process place names the relations
  that place actually offers.
- [ ] **Two small ones from the same session.** `near --relation process` reports the four
  options and never mentions that the relation is a positional selector (`near process`), which
  is the spelling that works; and `near <relation>` with no neighbours prints nothing at all, so
  "no such neighbour" and "the command did nothing" look the same.
- [ ] **A tombstone never names its replacement candidate (§10.3, §40).** `tombstone.replacement`
  and `tombstone.replacement_via` are on the place record and always `null`, and
  `Tombstone::replaced_by` is called from nowhere. The detail, the two routes and the exit test
  are under *Found by S11a* above; `docs/ACCEPTANCE.md` §4.7.3's §44.7 box names the gap, and
  case `096`'s `44.7e` is the assertion that changes when it is delivered.
- [ ] **Found by the dead-check sweep of `xtask/` (2026-08-28, `harness`, ADR-0159).** Same
  family as the repaired argument-mode check, but each is a different *kind* of change, so none
  was fixed there (AGENTS.md §4):
  - `xtask::contracts::check_commands` skips the verb, target and capability cross-checks for any
    command that omits the field (`!verb.is_empty() && …`). No command omits one today, so
    nothing is dead yet — but a command written without a `verb` would be checked against
    nothing and pass. Making the field required is a new contract rule (`feat`), and it must be
    decided together with the bare-name spatial commands of ADR-0124 — exit test: a fixture
    command with no `verb` is reported.
  - `docs/spec/kuang/*.v1.yaml` (seven contracts) reach `spec-check` only through the generic
    sweep, which proves they are non-empty valid YAML and nothing else. No check holds them
    against `crates/ono-kuang-*`, the way `check_provider_claims` holds the providers. Phase I
    work — exit test: a KUANG manifest field the SDK does not implement is reported.
  - `xtask::scan::rust_sources` walks `tests/`, `fuzz/` and `examples/` at the top level;
    `tests/` and `fuzz/` do not exist, so those walks find nothing. Harmless today (the suites
    live under `crates/*/tests/`), and it becomes real the moment AGENTS.md §2's `tests/` or the
    §35.6 `fuzz/` targets are created.
  - `xtask::scan::is_scanner_source` excludes all of `xtask/tests/` from the unfinished-work
    scan, not only the file that necessarily names the markers, so a `todo!()` in an xtask test
    is invisible to the gate. Narrowing it to `xtask/tests/scan.rs` is a `fix` of its own.
- [ ] **The v0.4 enhancement specification is unimplemented.**
  `docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` (3835 lines, "Spatial Systems
  Interface") arrived on `main` on 2026-08-27, after the v0.3 tranche was complete. It is
  checksummed and enumerated in AGENTS.md §2/§5.2, so the gate covers it, and nothing in the
  0.3.0 release implements it. Starting it means the loop of AGENTS.md §7 from the top: read
  §0 (relationship to the earlier specs) and §2 (core spatial invariants) first, decompose into
  increments, write the RED suites — a tranche the size of the v0.3 one, not a follow-up.
- [ ] Inside a link frame `get link` is sent to the other side (spec §14.4) and lists the
  remote agent's empty link table, so `get link | detach link` cannot be spelled from inside the
  link it would detach (seen while writing ADR-0118's detach case). Decide whether `get link`,
  `get job`, `get context` always describe *this* session — exit test: `link host x; enter link
  x; get link | count` answers 1
- [ ] **Found by the wiki verification pass (2026-08-27), each reproducible with `ono -c`**
  ((1)–(4) fixed, see *Done*): (5) `diff` of two fresh provider snapshots
  of the same object reports `changed` (user, group, file, mount, service) although nothing
  changed — volatile fields need excluding from the comparison — exit test: a data case;
  (6) `get process <gone-pid> | count` prints E0301 *and* `VALUE 0` with exit 0; `each {
  restart service @ }` drops the ActionResult rows; `get log`'s example `where level >=
  error` compares strings alphabetically (level should order as severity); cosmetic:
  `trace group root` → "command not found: trace", `get interface lo | stop interface` refuses
  the piped record, `get config --problems | select code` fails, the E0701 bulk message has
  runs of spaces, `trace connection --remote` without selector prints an empty name
- [ ] One label rule for an object: `ObjectRef::of` (first default-view column outside the
  identity — a mount's source device, a service's state) and `ono_graph::label_of` (a form per
  schema — the mount point, `nginx.service`) disagree, so a resolved selector's ActionResult
  row and a piped record's read differently for the same object (ADR-0088 §4, ADR-0116) —
  exit test: `unmount filesystem /` and `get mount / | unmount filesystem` render the same
  `target`
- [ ] `select error.name` on an `ono.action-result/1` row yields the whole `ono.error/1` under
  the key `name` instead of the string — a dotted path does not descend into an error value
  (seen while writing case 042; the case selects `error` whole instead). Reproduce:
  `unmount filesystem / | select error.name | to json`. `fix` candidate in `ono-command`'s
  `select`. Exit test: a `data_missing`-style outcome test on the projection.
- [ ] `unmount filesystem <dir>` for a directory that is no mount point answers E0301 with the
  seam's wording "no filesystem answers to target …" (ProviderMutation resolving the `filesystem`
  target); the provider's own "nothing is mounted at …" only reaches piped input. Cosmetic;
  the code is right (ADR-0098 §1).
- [ ] `read file` streams a large file as several `bytes` values instead of one whole-file
  value (ADR-0083 §3 defers chunking) — exit test: a files case reading a file larger than the
  pipeline's chunk size through `| count`
- [ ] `tail file` through inotify instead of the 100 ms poll (ADR-0083 §3) — exit test: the
  existing follow test with the poll interval removed
- [ ] `explain remove file *.txt` shows the resolved target count of the plan (ADR-0081) —
  exit test: an explain case inside a scratch directory
- [ ] `--preserve` timestamps on a directory copied with `--recursive` (`set_times` needs a
  writable handle; directories are skipped today, ADR-0082 §6) — exit test: a files case
  comparing `modified` of the copied tree

- [ ] network write paths, privileged conformance — ADR-0088 delivers the nine mutations and
  proves only the unprivileged refusal; a root run in the container (`add route … --dry-run`,
  then a real add/remove on a dummy interface and its removal) should prove the request layouts
  against a live kernel — exit test: a new case under `docker/acceptance/cases/` run with
  `CAP_NET_ADMIN`.
- [ ] `resolve dns --server <ip>` — refused as provider.unsupported (ADR-0087 §1); needs a DNS
  client (UDP/TCP query builder and parser) beside the system resolver.
- [ ] `select error.code` on an ActionResult row projects the whole error under `code` rather
  than the code string: reading a field *of* an error value (`FieldAccess::Failed`) yields the
  error — decide whether an `ono.error/1` field is navigable as a record (spec §10.5 keeps
  "could not read" apart from data, but the row's `error` *is* data).
- [ ] `explain get process` inside a frame prints the narrowed spelling (`get process 1`,
  `get process --user root`) — ADR-0023 promised it, ADR-0076 made the arguments available —
  exit test: a context.rs case explaining inside `enter process 1`
- [ ] `watch` composes `producer::ambient_selector` into its query on top of the narrowed
  arguments of ADR-0076 §5; move it onto the argument seam and delete the query-level form —
  exit test: a watch.rs case inside `enter process 1` (watch family)

- [ ] `to bytes --field body` refuses a record ("a record has no raw byte form") although
  `--field` names the one bytes field to emit — the natural way to write an adapted `curl`
  body to a file (`adapt curl url | to bytes --field body > page.html`) — exit test: a
  data_codecs case emitting one bytes field verbatim
- [ ] `explain` resolves a head by the registry alone, so `printf x | sort` is planned as
  `ono.data.sort` while the executor runs `/usr/bin/sort` (ADR-0028); the plan must use the
  executor's resolution — fixed with ADAPT-002, which needs it anyway (ADR-0052) — exit test:
  a builtins.rs case explaining `printf x | sort`
- [ ] A `SIGPIPE`d stdout (`ono -c '… | to json' | head -c 100`) reports io.permission_denied
  where every other shell exits quietly — treat EPIPE on stdout as normal termination — exit
  test: a cli case piping into head
- [ ] `sort` over a stream of scalars requires a key; identity should be the default key so
  `from json | sort` works on numbers — exit test: a transforms case
- [ ] Registry integration of contributions with origin `plugin(id, version)` (§31.64) so
  `get command` answers for a lazily loaded package; `always` grants and leases persisted under
  spec §31.19's policy store; `--scope`/`--duration` on `grant capability` — exit test: a
  plugins case granting with a scope and reading it back in a second session
- [ ] Phase H remainder: agentless mode (spec §21.3), trust-store location + first-contact key
  UX for a future authenticated transport (F12 rides along: TrustPolicy::Required records an
  unknown key — TOFU — where ADR-0015 T5 wants refusal; decide when the TCP transport exists,
  since both current transports are Unauthenticated-by-name per ADR-0037), eager surfacing of
  remote watch refusals — ADR-0036/0037 carry the details
- [ ] Phase I remainder (ADR-0040): wasm-component tier, objects/streams/views/models host
  domains, install/verify/signing, on-disk state + migrations, hot reload, binary frame encoding
  (a `perf` increment)
- [ ] Remaining `*-event/1` schemas (container, link, host) — each un-deferred as its watch is
  exercised; service, socket, interface, route, mount, file, user and group are written
  (ADR-0034, ADR-0078..0080) — exit test: a watch case per target
- [ ] `trace mount` propagation peers (storage.yaml promises them; ADR-0079 leaves them out
  until a peer group has an object to stand for it) — exit test: a storage case over a bind
  mount with `shared:` in mountinfo
- [ ] `watch file` through inotify and `watch interface|route` through the rtnetlink multicast
  groups, switching `source` to `subscription` (ADR-0078, ADR-0080) — exit test: a file
  created between two polls arrives before the next poll would
- [ ] `kill %N` for a native job (today: `fg` then Ctrl-C collects it) — exit test: a
  jobs_native.rs case
- [ ] Provider-native subscriptions (netlink, D-Bus signals) switching `source` to
  `subscription` (ADR-0034) — exit test: a watch.rs case against a subscribing fixture
- [ ] Retained results and secrets: spec §17.5 policy must reach the retention of §20.2
  (ADR-0033 consequences) — exit test: a redacted field stays redacted in `@-1`
- [ ] Provider options are silently ignored (`get process --user root` answers everything):
  audit every declared option against what providers honour, then make ignoring impossible the
  way selectors now cannot be ignored — exit test: a conformance case per optioned command
- [ ] `--name=value` in expression mode — ADR-0032 pairs an option with the following
  expression; the `=` spelling stays words-only until an increment adds it — exit test:
  a parse_expressions case for `reduce $acc + @ --initial=10`
- [ ] JSON object key order is alphabetical, not schema order (ADR-0030) — enabling
  serde_json `preserve_order` reorders the protocol too; decide and pin — exit test:
  data_codecs asserting §33.5 field order
- [ ] Surface `ono-pipeline` `Diagnostics` counters (`excluded_unknown`, `skipped_null`) to the
  user; ADR-0029 chose silence over an unread field — exit test: a case showing the count
- [ ] Streaming the byte carry across a native/external join (ADR-0028 buffers it) — exit test:
  `find / | from text | take 1` answers before the walk finishes
- [ ] Backgrounding a pipeline with native stages (ADR-0028 defers it) — exit test: `get process |
  count &` becomes a job `fg` can resume

Phase A is decomposed to increment level. Later phases are listed at their coarse shape and are
decomposed by the agent that starts them — decomposing early would invent detail the spec does
not fix yet.

### Phase A — Language and Unix shell foundation

**Phase A is complete.** Its exit criterion from spec §37 is proven by the acceptance case
`010-replaces-bash-for-ordinary-work`, and `docs/ACCEPTANCE.md` §4.1 A is ticked. The performance
budgets of §34 are tracked under *Cross-cutting*, not here.

- [x] A1 — Lexer: tokens, spans, quoting and escaping corpus — spec sections 6, 26 —
      exit test: `crates/ono-parser/tests/lexer.rs` golden corpus
- [x] A2 — Parser and AST with recoverable errors and precise spans — spec sections 24.4, 26 —
      exit test: golden AST snapshots + diagnostics snapshots
- [x] A3 — Incremental/partial parse for a line being typed — spec section 24.4 —
      exit test: partial-input parse tests
- [x] A4 — Evaluator skeleton: run an external command, propagate exit status — spec section 29 —
      exit test: acceptance `020-runs-external-commands`
- [x] A5 — Environment variables, `cd` and working directory — spec section 19 —
      exit test: acceptance `021-cwd-and-environment`
- [x] A6 — Redirection: `>`, `>>`, `<`, fd duplication, deterministic non-TTY behaviour —
      spec sections 12, 29 — exit test: acceptance `022-redirection`
- [x] A7 — External pipelines and exit status of a pipeline — spec section 11 —
      exit test: acceptance `023-external-pipelines`
- [x] A8 — PTY execution for full-screen programs — spec section 29 —
      exit test: acceptance `024-pty-applications`
- [x] A9 — Signals, process groups and foreground/background job control — spec section 18 —
      exit test: acceptance `025-job-control`
- [x] A10 — Line editor: keymap, editing, syntax highlight from the incremental parse —
      spec section 24.1 — exit test: editor behaviour tests + latency budget
- [x] A11 — History persistence and recall — spec section 20 — library done
      (`crates/ono-history/tests/history.rs`); wiring and acceptance case
      `026-history-survives-restart` land with the REPL
- [x] A12 — Configuration loading, with no eager plugin load and no network at startup —
      spec section 30 — exit test: acceptance `027-startup-is-quiet`
- [x] A13 — Prompt with location URI and privilege indication — spec sections 4, 17 —
      exit test: acceptance `028-prompt-shows-context`
- [x] A14 — Structured error model and exit-status contract — spec sections 16, 43 —
      exit test: error taxonomy tests
- [x] A15 — Phase A gate: `ono` as a login shell doing a real working session —
      exit test: acceptance `010-replaces-bash-for-ordinary-work` — **Phase A complete**

### Phase B — Value system and native pipelines (spec §10, §11, §12, §13, §25)

- [x] B3 — Stream engine: bounded channels, backpressure, cancellation, the streaming/blocking
      distinction — `crates/ono-pipeline/tests/{backpressure,boundedness,cancellation}.rs`
- [x] B6 — Conversion `to`/`from` json, yaml, csv, text, bytes — `crates/ono-value/tests/`
- [x] B7 — Renderer separated from data: table, stacked, list, tree, raw, hex; width-aware
      layout; visible truncation; semantic theme tokens — `crates/ono-render/tests/`
- [x] B1 — Value model: scalars, semantic scalars, units, `Record`, `Map`, `List`, provenance —
      `crates/ono-value/tests/` — ADR-0016 — commit d020129
- [x] B2 — Schema model and registry, the canonical schemas of spec §28, compatibility rules —
      `crates/ono-value/tests/{builtin_schemas,schema_compatibility}.rs` — commit d020129
- [x] B4 — Transforms `where`, `select`, `take`, `skip`, `each` (streaming) — spec §53 —
      `crates/ono-pipeline/tests/streaming_transforms.rs`, `crates/ono-command/tests/transforms.rs`
      — the acceptance case lands with the evaluator wiring
- [x] B5 — Transforms `sort`, `group`, `count`, `measure`, `reduce`, `join`, `diff` (bounded) —
      `crates/ono-pipeline/tests/blocking_transforms.rs` — acceptance case with the wiring
- [x] B9 — Pipeline type-checking before execution: `where cpy > 20` reports
      `type.unknown_field` with a suggestion from the contract's output schema, before anything is
      enumerated — `crates/ono-command/src/check.rs` — acceptance case with the wiring
- [ ] B8 — Object-to-external and external-to-object boundaries: structured input to an external
      command is a structured error suggesting `to json`; external stdout enters as bytes/text
      without loss — spec §12.2, §12.3 — exit test: acceptance `035-interop-boundary`
- [ ] B9 — Pipeline type-checking before execution where schemas are known: `where cpy > 20`
      reports `type.unknown_field` with a suggestion, before enumeration starts — spec §11.3 —
      exit test: acceptance `036-typo-caught-before-execution`
- [ ] B10 — `ActionResult` and partial failure: bulk mutation reports per-target results and
      never collapses them — spec §11.5, §16.5 — exit test: acceptance `037-partial-failure`

### Phase C — Linux core providers (spec §23, §28, §35.3)

Every provider answers from the kernel, systemd or NSS — never by parsing unstable human text
(spec §50, AGENTS.md §6). Every provider ships its conformance case in the same increment.

- [x] C1 — `ono-provider-api`: the provider trait, capability declarations, and the
      `snapshot` / `subscribe` / `watch` triple with the `ObjectEvent` envelope of spec §31.14,
      shaped so KUANG/11 consumes it without special cases (spec §31 preamble, §31.13)
- [x] C2 — `process` from procfs: enumeration, `ono.process/1` fields, CPU as a rate not a
      cumulative, permission-denied fields as errors not zeros — spec §23.1, §28.1 —
      exit test: acceptance `040-process-provider`
- [x] C3 — `file`/`dir`: metadata, recursion, symlinks, permissions, xattrs where present —
      spec §23.4, §28.2 — exit test: acceptance `041-file-provider`
- [x] C4 — `user`/`group` from NSS, `env` — spec §23.6, §28.7 —
      exit test: acceptance `042-identity-provider`
- [x] C5 — `mount`/`filesystem` — spec §23.5, §28.6 — exit test: acceptance `043-mount-provider`
- [x] C6 — `interface`/`route`/`neighbor` over netlink — spec §23.2, §28.5 —
      exit test: acceptance `044-network-provider`
- [x] C7 — `socket`/`connection` over netlink sock_diag, joined to owning process —
      spec §23.2, §28.4 — exit test: acceptance `045-socket-provider`
- [x] C8 — `service` over the systemd D-Bus API, degrading to `provider.unavailable` where
      systemd is not running — spec §23.3, §28.3 — exit test: acceptance `046-service-provider`
      plus a D-Bus fixture test for the positive path (see *Deferred*)
- [ ] C9 — Generated provider conformance suite from `docs/spec/providers/*.yaml` — spec §35.3

### Phase D — Language consistency and discoverability (spec §15, §27, §36, §47)

- [x] D0 — The registries themselves: `docs/spec/{verbs,targets,errors,capabilities,language}.yaml`,
      `schemas/*.v1.yaml`, `commands/*.yaml` — ADR-0012 — commit 6b107d0
- [x] D1 — `xtask spec-check` validates the registries and cross-checks them against the
      implementation: undocumented stable command, metadata without implementation, doc example
      that no longer parses, schema break without version bump, provider output outside its
      advertised schema — spec §36.5
- [ ] D2 — The command registry drives dispatch: one stable id per command, bound to an
      implementation, verified by `spec-check` — spec §27.2
- [ ] D3 — `help` generated from metadata for every command, target and topic — spec §15.2
- [ ] D4 — Completion from metadata: commands, verbs, targets, options, argument positions, and
      live values where a provider is cheap — spec §15.1 — exit test: first results < 50 ms
- [ ] D5 — `type` and `inspect`, showing schema, provenance and the causal chain — spec §15.2
- [ ] D6 — `explain`: the resolution and execution plan without executing, in the shape of
      spec §42 — spec §15.3
- [ ] D7 — Fuzzy command discovery and the suggestion path of `resolve.command_not_found` —
      spec §15.4
- [x] D8 — Generated documentation under `docs/reference/`, reproducible from the registries and
      checked by the gate — spec §36.2, §46

### Phase E — Contextual systems interface (spec §14, §20)

- [ ] E1 — Context stack, `enter`/`leave`, filesystem and object contexts — spec §14.1–§14.3
- [ ] E2 — Implicit selectors from context — spec §14.3
- [ ] E3 — Prompt as a HUD: link, privilege, context, path, vcs, jobs — spec §4.2
- [ ] E4 — Interactive selection over rendered collections, never altering pipeline data —
      spec §13.5
- [ ] E5 — Semantic history and bounded structured result retention; `@`, `@-1`, `@3` —
      spec §20.1, §20.2, §6.4

### Phase F — Live system semantics (spec §18)

- [ ] F1 — `watch` over a query, event/snapshot model, explicit polling metadata — §18.2
- [ ] F2 — In-place rendering keyed by stable object identity — §18.3
- [ ] F3 — Native background jobs, `get job`, the prompt's job segment — §18.4
- [ ] F4 — Cancellation through native pipelines and into external processes — §18.5

### Phase G — Relationship graph (spec §22)

- [ ] G1 — Graph value type with provenance and confidence — §22.1, §22.2
- [ ] G2 — Exact relationship providers: process tree, socket to process, service to process,
      mount to device — §22.3
- [ ] G3 — `trace` for process, service and socket — §22.3
- [ ] G4 — Tree and ASCII graph renderers; the graph view never fabricates edges — §22.4

### Phase H — Remote links (spec §21)

- [ ] H1 — `ono-protocol`: typed transport, framing, versioning, multiplexed streams — §21.2
- [ ] H2 — `ono-agent`: the remote endpoint — §21.4
- [ ] H3 — Agentless SSH fallback — §21.3
- [ ] H4 — Provider negotiation and capability discovery — §21.2
- [ ] H5 — Security model: host key pinning, `remote.host_key_changed` — §21.5, §49
- [ ] H6 — Remote context and prompt — §14.4, §4.2

### Phase I — KUANG/11 extension runtime (spec §31)

- [ ] I1 — `docs/spec/kuang/` contracts: manifest, capability, protocol schemas — §31.78
- [ ] I2 — `ono-kuang-protocol`: the typed host/plugin protocol — §31.12
- [ ] I3 — Package identity, layout, manifest validation, verification — §31.5–§31.7, §31.9
- [ ] I4 — Supervisor: install/enable/load/run states, lifecycle, isolation — §31.8, §31.10
- [ ] I5 — Capability broker, scopes, grant UX, storage and policy, audit — §31.16–§31.19, §31.33
- [ ] I6 — Host API domains: objects, streams, schemas, commands, relations, views, context,
      history, filesystem, network, process, secrets, models, state, audit, clock — §31.12
- [ ] I7 — Backpressure, quotas and overflow policy — §31.15
- [ ] I8 — Contribution model: commands, targets, schemas, relations, views, annotations — §31.22–§31.27
- [ ] I9 — `ono-kuang-sdk` and the deterministic test host — §31.73
- [ ] I10 — Plugin conformance suite — §31.74
- [ ] I11 — `ono-model-broker`: operator-approved inference, no LLM in a privileged path — §31.12

### Phase J — Advanced TUI views (spec §37 Phase J, §13.6)

- [ ] J1 — Navigable graph view — §22.5
- [ ] J2 — Multi-pane inspect/watch — §37
- [ ] J3 — Timeline/history exploration — §20.3
- [ ] J4 — Object pickers — §13.5
- [ ] J5 — Remote link overview — §37

### Cross-cutting, tracked to the release checklist

- [ ] Performance budgets of spec §34 measured in the container on the pathological fixtures
- [ ] Fuzzers over parser, serializers, remote protocol, plugin protocol, procfs/netlink
      decoders — spec §35.6
- [ ] A test for each risk in the threat model of spec §49
- [ ] Theme and semantic visual tokens — spec §44
- [ ] The per-capability quality bar of spec §50 for every advertised command

---

## Done

**The orphaned-shell leak is fixed (2026-08-28, agent `leak`, ADR-0160).** The 160 shells were
not spinning and not deadlocked on a lock: every one of them held the *master* side of its own
controlling terminal. `nix::pty::openpty` is glibc's `openpty(3)`, which opens `/dev/ptmx` and
the slave without `O_CLOEXEC`, and `PtySession::start` passed them straight to `spawn`, so every
program the shell started under a terminal inherited that terminal's master —
`/proc/<pid>/fdinfo/4` said `tty-index: 29` while `/proc/<pid>/fd/0` was `/dev/pts/29`. The last
reference to the master was therefore held by the shell reading from it, closing it in the caller
could never produce end of file, and the shell waited in `ep_poll` for a byte nobody could send.
Marking both descriptors `FD_CLOEXEC` in `PtySession::start` is the whole fix; the child still
gets the terminal as the three `dup2` duplicates `plan::prepare_pty` makes. `pgrep -c -x ono`
after a full `scripts/gate.sh` run is now 0, where it used to grow by a shell per PTY test.
Proven by `crates/ono-cli/tests/session_lifetime.rs`
(`should_exit_when_the_terminal_it_was_given_goes_away`,
`should_not_hold_the_terminal_that_drives_it`), both RED before the fix.



- [x] `fix(remote)` a shell ends the agent processes it started (ADR-0161): `link host` spawns
  `ono --agent` (or `ssh … ono --agent`) as its own child, and nothing waited for it — the shell
  exited first and the agent reparented to whatever init the machine runs. Measured from a
  process with `PR_SET_CHILD_SUBREAPER`: the shell was reaped, and a second, still-running
  process reparented onto the subreaper in the same millisecond, every run. In the container
  that init is `script` (`bash -lc 'script …'` execs it), whose `SIGCHLD` reaping took the
  orphan for its own child and hung up the `bash` under it — case 049's exit 129. Now
  `Session::hang_up` says the goodbye explicitly (`Link::hangup`) and waits for the process
  through a `ChildProcess` handle that outlives the transport, escalating `SIGTERM`/`SIGKILL`
  only after a 2 s grace it never reaches; `impl Drop for Session` does it for every link still
  held, before the runtime field is dropped. Every teardown path — `remove link`, `detach link`
  of a one-shot, `leave` of a one-shot frame, `add link` replacing a name, a handshake that
  failed after the child was spawned — goes through it. RED first in
  `crates/ono-cli/tests/session_lifetime.rs::should_end_the_agent_it_started_before_it_exits`.
  Proof: 20 consecutive `scripts/acceptance.sh --keep-image remote-link` runs green while a full
  69-case suite ran beside them; the subreaper probe sees no orphan; a linked `ono -c` costs the
  same as before (10 runs, 1.64 s, unchanged)

- [x] `fix(process)` an interactive `ono` no longer outlives the terminal it was given
  (ADR-0160): `PtySession::start` marks the `openpty` master and slave `FD_CLOEXEC`, so a program
  the shell starts under a terminal no longer inherits that terminal's master and end of file on
  the shell's input becomes possible at all. RED first in
  `crates/ono-cli/tests/session_lifetime.rs`; `pgrep -c -x ono` after a full gate run went from
  "one per PTY test, forever" to 0. Case 049's exit 129 was a second, separate leak of the same
  kind — the link's agent, not the shell — and is fixed by ADR-0161 below

- [x] installable `.deb`/`.rpm` for x86_64 and aarch64 (docs/ACCEPTANCE.md §4.5, ADR-0121,
  ADR-0122, ADR-0123): package metadata and maintainer scripts in `crates/ono-cli/Cargo.toml`
  + `crates/ono-cli/packaging/deb/`, shape pinned by `xtask/tests/packaging.rs` — commit
  cbc7612; `scripts/package.sh` (container builds via `cross`, `dist/ono_<v>_<arch>.deb`,
  `dist/ono-<v>-1.<arch>.rpm`, reproducible bytes) — commit 8608c1c;
  `scripts/package-check.sh` (install/run/login-shell/remove in fresh `debian:bookworm` and
  `fedora:latest`, structural check for a foreign arch) — commit a16633e; release workflow on
  `v*` tags with native x86_64 and aarch64 runners plus a `packaging` job in `ci.yml` — commit
  e6f10f1; the §4.5 box, `scripts/release-check.sh` running both scripts, README install
  section. Local aarch64 packages are structural proof only; their runtime proof is the release
  workflow on `ubuntu-24.04-arm`.
- [x] wiki-verification defect (1): piped forms of shell-answered commands answered by their
  seams, `input: null` refused with the head form named (ADR-0118) — commit 1e98be0
- [x] wiki-verification defect (2): `get env` reads the session's live environment — commit 8ca9aa7
- [x] wiki-verification defect (3): a watch over an empty listing begins with its snapshot — commit ed75190
- [x] wiki-verification defect (4): `let` in a block rebinds the enclosing binding (ADR-0119) — commit cc339ee
- [x] CI-red symlink walk: `--follow-symlinks` lists a directory under every name that reaches it;
  a cycle is an ancestor on the walk path (ADR-0120) — commit 25c1985

### The RED-suite run (2026-08-27): per-family notes, kept for their open items

- [identity | 2026-08-27] **identity family remainder** — **all 25 tests of
  `crates/ono-cli/tests/identity_missing.rs` green and un-ignored** on branch
  `implementation-identity` (ADR-0100–0102). Acceptance case
  `docker/acceptance/cases/043-identity-sessions-and-accounts.case` is written and dry-run
  against the binary; **the integrator runs it in the container** when merging. Left open (not
  in the RED suite): a privileged conformance run of the account tools (the workspace's tests
  never change the developer's accounts); `select error.code` on an ActionResult row projects
  the whole error under `code` instead of its field.
- [remote | 2026-08-27] **remote family** (`crates/ono-cli/tests/remote_missing.rs`) on branch
  `implementation-remote` — **all 36 tests green and un-ignored**; the gate is green at every
  commit. Delivered: `link`/`host` as tables of the session provider, `ono.host/1` and its
  three sources (ADR-0103); `add/set/rename/remove/detach link`, `connect host`, `test host`
  and `add/set/remove host` (ADR-0104); `watch`/`trace` for link and host with
  `ono.link-event/1`, `ono.host-event/1`, `ono.provider/1` (ADR-0105); `--agentless` recorded
  and visible, `explain`'s EXECUTION CONTEXT and MUTATION blocks (ADR-0106). Acceptance case
  `docker/acceptance/cases/044-remote-links-as-objects.case` is written and dry-run against the
  binary; **the integrator runs it in the container** when merging the branch. Case 049 now
  matches the typed `get link` table by regular expression. Left open (not in the RED suite):
  the piped forms `get link | remove link` / `… | detach link`; the agentless provider set of
  ADR-0037 §6 (today `mode: agentless` is recorded and the agent answers, visibly); a
  `watch host` that probes reachability; the multiplexed streams of `trace link`; the
  execution context in the `ono.execution-plan/1` value.
- [containers | 2026-08-27] **container and package families** (`crates/ono-cli/tests/containers_packages_missing.rs`)
  on branch `implementation-containers` — files: `crates/ono-provider-container/`,
  `crates/ono-provider-linux/src/packages.rs`, `crates/ono-graph/src/kernel/container.rs`,
  `crates/ono-command/src/impls/{mod,meta}.rs`, `crates/ono-cli/src/providers.rs`,
  `docs/spec/commands/{container,package}.yaml`, `docs/spec/schemas/{container,image,package,container-event}.v1.yaml`,
  `docs/spec/providers/{container-engine,linux-packages}.yaml`, acceptance case
  `046-containers-and-packages`. ADR-0112–0115.
  Increment 1 done: `ono-provider-container` — the engine API over the runtime's Unix socket,
  `get container`/`get image`, E0401 naming the sockets tried (4 tests green, ADR-0112).
  Increment 2 done: start/stop/restart/remove/set container as engine requests, the engine's
  status as the per-target outcome (8 tests green, ADR-0113). Increment 3 done: `enter
  container` as a `container` frame, `watch container` over the engine listing, `trace
  container` with the exact `image` edge (4 tests green, ADR-0114). Increment 4 done:
  `linux.packages` — `get package`/`find package` from `dpkg-query -W -f` and `apt-cache
  search`, E0401 naming dpkg and rpm, E0403 for a listing outside the machine format (5 tests
  green, ADR-0115). Increment 5 done: `add`/`remove`/`set package` through `apt-get` and
  `apt-mark`, the unprivileged refusal as a failed E0302 row before anything runs (4 tests
  green, ADR-0115 §5). **All 25 tests green and un-ignored**; the gate is green at every
  commit. Acceptance case `docker/acceptance/cases/046-containers-and-packages.case` is
  written and dry-run against the binary; **the integrator runs it in the container** when
  merging the branch. Left open (not in the RED suite): an rpm/dnf package provider (the
  refusal names it); `trace container` edges to namespaces, cgroups, mounts and processes
  (need `State.Pid` joined to procfs); `watch container` over the engine's `/events` instead
  of polling; `enter container` as an execution context (`container.exec`); a root acceptance
  case for the package mutations' success path.

- [plugins | 2026-08-27] **plugins family** (`crates/ono-cli/tests/plugins_missing.rs`) on
  branch `implementation-plugins` — **all 32 tests green and un-ignored**; the gate is green at
  every commit. Delivered: `ono.plugin/1` records from the session provider `ono.shell`
  (ADR-0107); `verify`/`inspect`/`find plugin` and the K11 family folded into
  `ono_core::ErrorCode` (ADR-0108); `install`/`remove plugin` (ADR-0109); `unload`/`set plugin`,
  enablement on disk, hot reload (ADR-0110); `get/grant/revoke capability`, `get audit`, and the
  typed empty `assistant`/`model`/`finding` tables (ADR-0111). Acceptance case
  `docker/acceptance/cases/045-plugins-lifecycle.case` is written and dry-run against the
  binary; **the integrator runs it in the container** when merging the branch. Left open (not in
  the RED suite): `always` grants and leases on disk (spec §31.19), `--scope`/`--duration` on
  `grant capability`, `capability_grants` inside `inspect plugin`, instance memory/cpu figures
  (null today), the interactive install prompt under a PTY case.
- [meta | 2026-08-27] **meta family** (`crates/ono-cli/tests/meta_config_missing.rs`, plus the
  `--human` and uid/gid cases of `options_and_selectors_missing.rs`) on branch
  `implementation-meta` — files: `crates/ono-cli/src/{meta,resolve,settings,config,eval,native}.rs`,
  `crates/ono-command/src/impls/{meta,convert}.rs`, `docs/spec/schemas/command.v1.yaml`,
  `docs/spec/commands/identity.yaml`, acceptance case `041-config-and-resolve`. ADR-0093–0095.
  Increment 1 done: `resolve command` (6 tests green, ADR-0093). Increment 2 done: the typed
  settings catalogue, `get config` with layers/source/line/`--overridden`/`--problems`, typed
  `set config` with E0202/E0201 and its ActionResult (15 tests green, ADR-0094). Increment 3
  done: `render.table.max_rows` reaches the sink, redirected output and `format table`
  (3 tests green; the file has no `#[ignore` left). Increments 4–5 done: `--human` reaches
  record fields (2 tests), `uid`/`gid` declared before `name` so numeric selectors bind
  (2 tests, ADR-0095). Acceptance case `041-config-and-resolve` written and dry-run against the
  binary; **the integrator runs it in the container** when merging.

- [language | 2026-08-27] **language family** (`crates/ono-cli/tests/language_missing.rs`) on
  branch `implementation-language` — **all 31 tests green and un-ignored**; the gate is green
  at every commit. Delivered: `let`/`( … )`/`$( … )` capture (ADR-0069); callable functions and
  `alias` (ADR-0070); `now()`, the RFC 3339 timestamp literal, prefix assignment
  `NAME=value cmd`, `each { … }` blocks, string `+`, keyless `sort`, `kill %N` (ADR-0071).
  Acceptance case `docker/acceptance/cases/035-scripting-language.case` is written and
  dry-run against the binary; **the integrator runs it in the container** when merging the
  branch. Left open (not in the RED suite): `explain` of a `NAME=value cmd` stage, functions
  and aliases in completion candidates, a function in a non-head pipeline position.
- [watch | 2026-08-27] **`watch`/`trace` for the declared-but-unbound targets** — done for
  file, user, group, interface, route and mount (ADR-0078..0080; commits on
  `implementation-watch`). Left ignored: the remote five in `remote_missing.rs` (`watch
  link|host`, `trace link|host`) — they need `link`/`host` as provider-backed records first
  (context.rs `get_link` renders by hand); the remote family picks them up.

- [processes | 2026-08-27] **process family remainder** (`crates/ono-cli/tests/processes_missing.rs`;
  `--tree`/`--user` in `options_and_selectors_missing.rs`) on branch `implementation-processes`
  — **all 18 process tests and the 3 option tests green and un-ignored**; the gate is green at
  every commit. Delivered: `get job` from the session provider `ono.shell` (ADR-0090);
  `inspect process` → `ono.process-detail/1` and `get process --tree` (ADR-0091); `set process
  --priority` via setpriority(2) and `send signal` as the pipeline spelling of a signal
  (ADR-0092). Acceptance case `docker/acceptance/cases/040-processes-inspect-jobs-signals.case`
  is written and dry-run against the binary; **the integrator runs it in the container** when
  merging the branch. Left open (not in the RED suite): a tree renderer for `--tree` at the
  terminal (the table shows the roots' columns; spec §22.4's tree view is the graph family's);
  `link`/`host` rows in `SessionTables` (remote family, ADR-0090 §3).

- [agent | 2026-08-27] **RED suites for everything v0.2 declares but does not build** (user
  request; wiki pages "Command Index" and "What Is Not Built Yet"). 329 outcome tests, every
  one `#[ignore = "REASON: …"]` (AGENTS.md §7) so the tree stays green; **the increment that
  delivers a family removes the ignore lines of its tests in the same commit** — a family is
  done when its file has no `#[ignore` left and the gate is green. Work order: cross-cutting
  seams first (registry-dispatched `set`/`remove`, ActionResult exit status and error shape,
  generic `enter`/`watch`/`trace` for object targets), then the families. Each file is one
  family; each test asserts the behaviour the contract promises, never mere presence:
  - `crates/ono-cli/tests/files_missing.rs` (34) — read/write/copy/move/remove/set/open/tail/
    watch/trace/enter file, remove/set dir, globs for native selectors — **done** by
    [files | 2026-08-27] on branch `implementation-files` (ADR-0081–0083) for everything
    except the four watch/trace tests, which stay `#[ignore` for the watch/trace family;
    the four `find file` option tests of `options_and_selectors_missing.rs` are green too.
    Acceptance: `docker/acceptance/cases/037-files-read-write-remove.case` (written, not yet
    run in the container by this agent)
  - `crates/ono-cli/tests/language_missing.rs` (31) — `let` capturing a pipeline, `$(…)`/`(…)`
    values, callable `fn`, `alias`, `now()`, timestamp literals, `FOO=bar cmd`, `each { … }`,
    string `+`, keyless `sort`, `kill %N`
  - `crates/ono-cli/tests/options_and_selectors_missing.rs` (15) — `--user/--tree`, `find file`
    options (**done**, files family), `--mounted`, `trace socket --port`, `--human`,
    `get user 0`, `where local.port`
  - `crates/ono-cli/tests/meta_config_missing.rs` (24) — `resolve command`, `get config` layers/
    source/line, `set config` typed + effective (`render.table.max_rows`)
  - `crates/ono-cli/tests/processes_missing.rs` (18) — `inspect process`, `get job`, `enter
    process`, `set process --priority`, `send signal`, failed ActionResult ⇒ exit 1 (ADR-0006)
  - `crates/ono-cli/tests/identity_missing.rs` (25) — `get session`, user/group mutations,
    watch/trace/enter user|group
  - `crates/ono-cli/tests/network_missing.rs` (31) — `resolve dns`, `test port`, watch/trace/
    enter interface|route|socket, route/interface/socket mutations
  - `crates/ono-cli/tests/services_logs_missing.rs` (15) — `set service`, `get journal`,
    `tail journal`, `get log` — **done, 15/15** by [services | 2026-08-27] on branch
    `implementation-services` (ADR-0084–0086, ADR-0096, case 038)
  - `crates/ono-cli/tests/storage_missing.rs` (22) — `get device`, mount/unmount, mount verbs,
    watch/trace/enter mount — **done** by [storage | 2026-08-27] on branch
    `implementation-storage` (ADR-0097–0099, case 042-storage-devices-and-mounts); nothing left
    ignored. `should_return_only_unmounted_filesystems_when_mounted_is_false` in
    `options_and_selectors_missing.rs` is green on the same branch.
  - `crates/ono-cli/tests/data_missing.rs` (15) + `crates/ono-command/tests/completion_missing.rs`
    (6) — `tail`, `join`, `diff`, stacked records on narrow terminals, fields after `where`
    — **done** by [data | 2026-08-27] on branch `implementation-data` (ADR-0072–0074); no
    `#[ignore` left in either file
  - `crates/ono-cli/tests/remote_missing.rs` (36) — `get link` as data, host commands, link
    definitions, detach/rename, agentless visibility, mutations across a link — **done** by
    [remote | 2026-08-27] on branch `implementation-remote` (ADR-0103–0106, case 044); no
    `#[ignore` left
  - `crates/ono-cli/tests/plugins_missing.rs` (32) — `ono.plugin/1` records, inspect/find/
    verify/install/unload/set/remove plugin, capabilities, audit, reload, assistants/models
  - `crates/ono-cli/tests/containers_packages_missing.rs` (25) — a fake engine-API socket and
    fake package managers on PATH; E0401 when none answers

  Wiki claims found stale while writing them (already work, no test added): `get route
  --table/--family`, `format --max-rows`, backgrounding native stages, `let i = $i + 1`.

  Contract gaps the suites had to resolve by reading — each needs an ADR (or a registry change)
  before its GREEN increment: `alias` statement syntax (grammar.ebnf/language.yaml have none);
  `ono.command/1` resolution `kind` field; `set config` unknown key ⇒ E0202; `ono.device/1`
  shape (path/kind/major/minor); `ono.session/1` fields; `ono.link/1` lacks a `host` field;
  `ono.container/1`, `ono.image/1`, `ono.package/1` schemas and the runtime knobs
  (`DOCKER_HOST`/`CONTAINER_HOST`, managers found on PATH); `get journal`/`get log` referenced
  `ono.log-record/1` which neither existed nor was deferred (resolved: ADR-0085/0086);
  `join`/`diff` output shape and
  `--identity [pid]` spelling; failed ActionResult rows nest the error as
  `error.error.code = "io.permission_denied"` instead of `error.code = "Ono-Sendai-E…"`, and
  `operation` carries the bare verb instead of the command id; K11 codes not folded into
  `Ono-Sendai-K11xxx`; `--agentless` is accepted and ignored by `context.rs::link`.

### Everything else

- [x] remote family — `get link`/`get host` from the session provider (ADR-0103) — commit
  19dce98; link definitions add/set/rename/remove/detach (ADR-0104) — commit fb10641;
  `connect host`, `test host`, ssh `-F` — commit 89879f3; watch/trace link and host
  (ADR-0105) — commit 5ced427; `--agentless` visible, `explain` context and mutation blocks
  (ADR-0106) — commit 91539b5; `add/set/remove host` and acceptance case 044 — see the
  `implementation-remote` log; `remote_missing.rs` (36) un-ignored
- [x] plugins family — `get plugin` records (ADR-0107) — commit f7a487a; verify/inspect/find and
  the K11 fold (ADR-0108) — commit 2cababb; install/remove (ADR-0109) — commit ca68ab0;
  unload/set/enablement (ADR-0110) — commit 7757006; hot reload — commit 4835eae;
  capabilities and audit (ADR-0111) — commit de2f831; assistants/models/findings and case 045
  — this commit; `plugins_missing.rs` (32) un-ignored
- [x] process family — `get job` (session provider, ADR-0090) — commit 0cc0730; `inspect process`
  (`ono.process-detail/1`, ADR-0091) — commit d512f03; `get process --tree` — commit b3f91a4;
  `set process --priority` (ADR-0092) — commit 730b1a3; `send signal` — commit d9cd7f8;
  `processes_missing.rs` (18) and the three `--user`/`--tree` tests un-ignored
- [x] File family — globs for native selectors, `read`/`write`/`copy`/`move`/`remove`/`set`/
  `open`/`tail file`, `remove`/`set dir`, `find file --name/--depth/--kind/--follow-symlinks`
  (ADR-0081, ADR-0082, ADR-0083) — branch `implementation-files`, commits b27b0a5, 7c41a09,
  a9b1f2f, c7e0e15, c26466f and the find-options commit after them

- [x] network family — `resolve dns` (system resolver, `ono-provider-net`, ADR-0087), `test port`
  (probe result, ADR-0087 §3), the nine route/interface/socket mutations over rtnetlink and
  sock_diag with the unresolved-target and `confirmation: always` seams (ADR-0088), null
  through a schema-known field and port/int comparability (ADR-0089), `--remote` on
  `trace connection`; a serializer no longer writes `[]` for a stream that only failed
  (ADR-0028) — commits 24f7968, baf53e2, e1d5a73, 3c54b30 and the two fixes after it;
  `network_missing.rs` (17 tests), `options_and_selectors_missing.rs` (3 tests),
  `docker/acceptance/cases/039-network-dns-port-mutations.case`. The eight watch/trace tests of
  `network_missing.rs` belong to another agent.
- [x] storage 1 — `get device` from /dev + sysfs, `ono.device/1` written — commit 0f9a36a
  (ADR-0097; `storage_missing.rs` ×4)
- [x] storage 2 — `get filesystem --mounted`, unmounted filesystems from udev's probe — commit
  2e588f4 (ADR-0097 §3; `options_and_selectors_missing.rs` ×1, provider fixture ×3)
- [x] storage 3 — `mount`/`unmount filesystem` through mount(2)/umount2(2); creating verbs name
  their object — commits e818770 (test form), 5a90ea8 (ADR-0098; `storage_missing.rs` ×5)
- [x] storage 4 — `set`/`add`/`remove`/`start`/`stop mount`: remount, fstab definitions, systemd
  mount units — commit e2e2f03 (ADR-0099; `storage_missing.rs` ×5, provider fixture ×5)
- [x] identity 1 — `get session` from systemd-logind over D-Bus, `ono.session/1` written,
  `--user` filter, E0401 where no login manager answers — ADR-0100;
  `identity_missing.rs` (2 tests) un-ignored, `crates/ono-provider-systemd/tests/session.rs` (4)
- [x] identity 2 — `add`/`remove`/`set user` through shadow-utils by exit status, E0302 from the
  euid check before any tool runs; `add` acts unresolved and an ambiguous name is narrowed by
  the input type — ADR-0101, ADR-0102; `identity_missing.rs` (6 tests) un-ignored
- [x] identity 3 — `add`/`remove`/`set group` and `--member` membership through
  `groupadd`/`groupdel`/`groupmod`/`gpasswd`, same privilege gate — ADR-0101;
  `identity_missing.rs` (5 tests) un-ignored; the file has no `#[ignore` left; acceptance case
  `043-identity-sessions-and-accounts` written
- [x] seams 1 — `set`/`remove` of system targets dispatch through the registry — commit 7ec0d83
  (ADR-0068 §1; `crates/ono-cli/tests/builtins.rs`)
- [x] seams 2 — ActionResult contract: a failed row exits 1, a missing target is an E0301 row,
  `operation` is the command id, `error` is a flat `ono.error/1` — ADR-0068 §2;
  `processes_missing.rs` (3 tests), `remote_missing.rs` (2 tests) un-ignored
- [x] seams 3 — a mutating verb binds when a provider advertises its capability
  (`builtin_commands_for`, ADR-0068 §3); `crates/ono-command/tests/mutations.rs` (4 tests).
  Families deliver a mutation by advertising the capability and answering the verb in `act`:
  `set service` now reaches the systemd provider, which reports it has no `set` operation
  (E0402 row) until the services family maps `--enabled` onto enable/disable and reports a
  missing property as E0201 naming `--enabled`
  (`services_logs_missing.rs::should_refuse_set_service_without_a_property…` stays ignored).

- [x] services 1 — `set service <unit> --enabled true|false` reaches the systemd provider as the
  `set` operation with the property as an argument (EnableUnitFiles/DisableUnitFiles); a `set`
  with no property is E0201 naming `--enabled` before anything is resolved (ADR-0084);
  `services_logs_missing.rs` (4 `set service` tests un-ignored),
  `ono-provider-systemd/tests/service.rs` (2 tests)
- [x] services 2 — `get journal [--since --boot]` and `tail journal [--lines]` as
  `ono.journal-event/1` through `journalctl --output=json` and the systemd adapter pack's
  decoder (ADR-0085); a provider-kind stream failure exits 1; `StreamSink::closed()` lets a
  following producer stop when `take` is satisfied; the decoder reads journalctl's byte-array
  and multi-valued strings (fix); `services_logs_missing.rs` (6 journal tests un-ignored)
- [x] services 4 — expression-valued options reach the provider query (`--since (now() - 1h)`
  evaluated in the producer, fix); a bare word compared with an enum field is that field's
  value — `where state == failed`, `where level >= error` run (ADR-0096); `services_logs_missing.rs`
  15/15, `ono-command/tests/expressions.rs`, `ono-cli/tests/native.rs`
- [x] services 3 — `get log [--service <ref>] [--level <name>] [--since --until]` as
  `ono.log-record/1` (journal-event plus `level`, the severity name) from the same journal
  provider (ADR-0086); case 038
- [x] data family (ADR-0072) — `tail N [--follow]` (commit 0f68fe0), `join <right> --on key
  --kind inner|left|right|outer` with `$variables` and pre-run `(pipelines)` visible to native
  stages (1616fe1), `diff <right> [--identity [fields]]` by schema identity (1761cc9),
  stacked records once a cut column would drop below eight cells (ADR-0073, 98437d4),
  schema fields with their docs after `where`/`select` (ADR-0074) —
  `crates/ono-cli/tests/data_missing.rs` (15/15), `crates/ono-command/tests/
  completion_missing.rs` (6/6), case 036
- [x] the context stack for every object target — `enter` of process/user/group/interface/
  socket/mount/file by word or by pipe (`get socket 443 | enter socket`), frames narrowing
  every later command at the command-table seam (`pid 1`, `--user root`, `--interface lo`,
  `--port 443`), `--user`/`--group` honoured by the procfs provider, `--interface` declared on
  `get route`, `--port` honoured by `trace socket` — ADR-0075, ADR-0076 — 24 tests un-ignored
  in `crates/ono-cli/tests/{processes,identity,network,storage,files}_missing.rs`

- [x] v0.3 step 1 — ADAPT-001 OutputDemand computed backwards from the consumer, reported
  by `explain` (ADR-0052) — cases 070, 071
- [x] v0.3 step 2 — the `adapter.*` error family E0901–E0911 in `docs/spec/errors.yaml` and
  `ono_core::ErrorCode` (ADR-0053) — `error_taxonomy.rs`; the box in ACCEPTANCE §4.6.2 stays
  open until an adapter emits one with the §1.65 payload
- [x] v0.3 step 3 — ADAPT-003 the `raw` keyword; `adapt` spelled for §1.18 (ADR-0054) —
  case 072
- [x] v0.3 step 4 — ADAPT-009 the declarative adapter contract, the util-linux pack with
  fixtures, `ono.block-device/1` and `ono.namespace/1`, the validator and the spec-check rule
  (ADR-0055) — `ono-adapter/tests/contracts.rs`, `xtask/tests/contracts.rs`
- [x] v0.3 step 5 — ADAPT-002 registry, negotiation states, identity pinning, conflict
  resolution, the probe cache of ADAPT-006, and `explain`'s `adaptation`/`argv`/`candidates`
  rows (ADR-0056) — `ono-adapter/tests/negotiation.rs`, case 073
- [x] v0.3 step 6 — ADAPT-004/007/010 and COMPAT-LSBLK/FINDMNT/LSNS: adapted execution
  through `ono-process`, the json decoder, adapter provenance in `inspect`, the fixture
  harness in `spec-check`, util-linux end to end (ADR-0057) — `ono-cli/tests/adapters.rs`,
  cases 074, 075. ADAPT-005's streaming half waits for the first line-protocol tool.
- [x] v0.3 step 7 — COMPAT-IP: the iproute2 pack, `ono.interface-address/1`, the field-map
  derivations children/template/first/infer/literals/require (ADR-0058) — case 076
- [x] v0.3 step 8 — ADAPT-005 streamed adaptation (`Decoding`, `Output::Pipe`,
  `start_piped`/`finish_foreground`, cancellation to the producer), the systemd pack
  (journalctl jsonl, systemctl list-units/show with the `properties` decoder),
  `ono.journal-event/1`, the live view absorbing plain records; the image gains git, curl,
  lsof (ADR-0059) — case 077
- [x] v0.3 step 9 — COMPAT-PS: the procps pack, whitespace columns, `first` on strings,
  `program-name`/`started-from-elapsed` inferences, streaming `lines` (ADR-0060) — case 078
- [x] v0.3 step 10 — COMPAT-STAT/DF/FIND: the coreutils and findutils packs, trailing argv,
  header lines, basename, NUL records with the path last, typed-order pass-through
  (ADR-0061) — case 079
- [x] v0.3 step 11 — COMPAT-GIT/LSOF: builtin decoders `git-status-v2` and `lsof-fields-v1`,
  `ono.git-status-entry/1`, `ono.commit/1`, `ono.open-file/1`, hex escapes (ADR-0062) — case 080
- [x] v0.3 step 12 — COMPAT-SS: combined flags, nested record coercion, the `ss-text-v6`
  version-constrained parser, required flags as specificity (ADR-0063) — case 081
- [x] v0.3 step 13 — the `adapt` keyword of §1.18 (E0911 when nothing answers) and
  COMPAT-CURL: `ono.http-exchange/1`, the `curl-exchange-v1` decoder with the body kept as
  exact bytes, secrets never adapting (ADR-0064) — case 082
- [x] v0.3 step 14 — ADAPT-008: `contributions.adapters`, the `executables`/`argv_policy`
  scope of `process.exec`, packs loaded disabled under default deny and enabled by
  `--grant process.exec` (experimental packs by `--allow-experimental` besides), the test
  host's `check_adapter_package`, the SDK's example package (ADR-0065) — case 083
- [x] v0.3 step 15 — ADAPT-011: the `start-adapt` frame, the agent negotiating, running and
  decoding on its side, records marked with the host, `explain … on <host>`, visible
  degradation (ADR-0066) — case 084
- [x] v0.3 step 16 — integration surfaces: adapted stages are producers for the pre-flight
  check, `type`, completion and history; text tools pinned raw; the §1.71 session, script
  determinism and the muscle-memory diff as cases (ADR-0067) — cases 085, 086, 087
- [x] v0.3 step 17 — release evidence: generated adapter reference pages and the compatibility
  matrix, live conformance for every first-party adapter (case 088), measured overhead
  (case 089), the README section with examples that parse and run under xtask — all §4.6
  boxes ticked

- [x] `get service <name>` reaches unloaded on-disk units, and the listing no longer reports
      `not-found` stubs. Investigation showed the by-name path already resolved through
      `LoadUnit`; the real defect behind the CI flake was the inverse — `ListUnits` enumerates a
      stub for a referenced unit whose file is gone, and the enumeration reported it as a
      service the by-name path then rightly denied. Both paths now agree — tests:
      `should_find_a_unit_on_disk_when_systemd_has_not_loaded_it`,
      `should_report_no_service_when_a_listed_unit_is_only_a_dangling_reference`
- [x] Bootstrap: Cargo workspace (`ono-cli`, `ono-core`, `ono-testkit`, `xtask`), pinned
      toolchain, lint configuration, first outcome tests — ADR-0001
- [x] Quality gate `scripts/gate.sh` and contract check `cargo xtask spec-check` — ADR-0001
- [x] Containerised acceptance harness: `docker/Dockerfile`, `docker/acceptance/cases/`,
      `scripts/acceptance.sh`, verified green with four cases — ADR-0002
- [x] Release gate `scripts/release-check.sh` and the stopping rule in `docs/ACCEPTANCE.md` —
      ADR-0002
- [x] CI running the gate and the acceptance suite on every push — ADR-0002
- [x] Specification immutability enforced by checksum in `cargo xtask spec-check` — ADR-0003
- [x] Branch policy: implementation on a disposable `implementation` branch, guarded in
      `scripts/gate.sh` — ADR-0004
- [x] Acceptance harness extended: `|` block scripts, stdin, `pty:`, `columns:`/`lines:`, `env:`,
      `timeout:` and repeatable assertions, with a self-test case — commit 036f89c
- [x] The gate refuses untracked unfinished work: `todo!()`, `unimplemented!()`, untracked
      `TODO`/`FIXME`, `#[ignore]` without a reason — `xtask/tests/scan.rs` — commit 6f7c308
- [x] `ono-testkit`: real-binary runs with a deadline, scratch directories, and a reproducible
      generator for fuzz-style tests — commits b2a0d2d, a20056c
- [x] `ono-render`: width-aware table and stacked-record layout, semantic theme tokens, the
      presentation contract of spec §4.6, and the ASCII tree of §22.4 — commits bb2d825,
      a3d3fac, 37f78de
- [x] `ono-history`: semantic entries, restart survival, secret policy — commit 0b1def8
- [x] A0 — Shared vocabulary in `ono-core`: `Span`, the complete error taxonomy of spec §43,
      the exit-status contract — ADR-0005/0006/0008 — commit 5551654 —
      tests `crates/ono-core/tests/{error_taxonomy,exit_status,span}.rs`
- [x] A0 — The concrete grammar: ADR-0009 and `docs/spec/grammar.ebnf`, resolving the
      command/expression ambiguity of spec §26.1 with the two argument modes

---

## Known defects (found by adversarial review, 2026-08-26)

Two independent reviewers were asked to falsify the implementation rather than describe it, as
AUTONOMOUS_IMPLEMENTATION.md §18 requires. Between them they found 27 things, each with a
reproduction they ran.

**Everything release-blocking is fixed**, each with a regression test that fails without the fix
(commits 0742918, aeae961). A ticked box below means fixed *and* guarded. What remains unticked is
should-fix or unbuilt, and each entry says which.

- [x] **R1 — nested blocks overflow the stack.** `if true { if true { … } }` nested about 2000
      deep aborts the process with SIGABRT. `MAX_DEPTH` in `crates/ono-parser/src/parser.rs` is
      consulted in `parse_stage` and in the expression parser but not in `parse_block`, so
      statement recursion is unguarded. The parser claims never to panic and always to return a
      tree, and it runs on every keystroke in the editor — one pasted line kills a login shell.
      `crates/ono-parser/tests/robustness.rs` has a test named for this that repeats `{` 2000
      times, which never enters block recursion: it passes while the thing it names is broken.
- [x] **R2 — `exit` in a configuration file hijacks the whole session.** `config::load` runs the
      config in the same `Session`, so `exit 3` there sets `session.leaving`, which is never
      cleared. Every later statement short-circuits and every command's status is replaced.
      Breaks ADR-0008 ("an external command's status is passed through unchanged") and ADR-0010
      ("a bad setting never stops the shell from starting").
- [x] **R3 — configuration mode stops external commands only.** The single-builtin fast path in
      `crates/ono-cli/src/eval.rs` returns before the `Mode::Config` check, so `cd`, `remove env`,
      `help`, `jobs`, `fg`, `bg` and `exit` all run from a config file. The error text the code
      itself prints says configuration "runs nothing". `028-config-is-restricted` only tries
      `touch`, so it does not prove what it claims.
- [ ] **R4 — a builtin ignores its redirections and cannot be piped.** `help > out.txt` prints to
      stdout and writes no file; `help | cat` reports `resolve.command_not_found` for `help` and
      then reports success.
- [x] **R5 — an unterminated `${` eats the rest of the word.** `printf '[%s]' a${HOMEb` yields
      `[a$]`. `crates/ono-cli/src/expand.rs` drains the iterator looking for `}` and drops what it
      consumed, while its own comment says the text is kept as typed. Silent data loss inside an
      argument, which is the class of surprise ADR-0019 exists to remove.
- [x] **R6 — background children are only reaped when `jobs`/`fg`/`bg` runs.** A script that
      backgrounds 100 commands leaves 100 zombies, because `poll_jobs` is called only from the
      interactive loop and from the `jobs` builtin.
- [x] **R7 — a bad shebang reports 127 rather than 126.** `crates/ono-process/src/spawn.rs` maps
      every `ENOENT` from `exec` to `NOT_FOUND` without distinguishing the program from its
      interpreter. ADR-0008's table and every other shell say 126.
- [x] **R8 — a parse error echoes the whole source line.** A 100 000-character line produces a
      98 KB error message; the shown line needs a budget and an ellipsis.

What the review tried hard to break and could not, which is worth keeping: ADR-0019's rule that a
value's content never becomes a command's structure held under filenames containing spaces,
newlines, quotes, `$`, `*`, backslashes and raw escape bytes; file-descriptor hygiene is correct
including the fd-shuffle most hand-written shells get wrong; and the `pre_exec` SAFETY claim of
ADR-0007 is accurate as written.

### From the security review (ADR-0015 checklist)

Each was reproduced by the reviewer against the built binary. The release-blocking ones are fixed
and guarded; the rest stay open with their reproduction.

- [x] **F1 — `explain` prints attacker-controlled escape sequences raw.** A program name on `PATH`
      containing an OSC sequence retitles the terminal when `explain` reports it, and the bytes
      survive redirection into a file. `crates/ono-cli/src/builtin.rs` and
      `crates/ono-command/src/explain.rs` echo stage source and resolved paths without sanitising.
      ADR-0015 T1/T9/T11. The row's named acceptance case uses the benign name `ls`.
- [x] **F2 — structured error messages are not sanitised.** Only the code and the help line are
      painted through the theme; `error.message()` is written raw
      (`crates/ono-cli/src/report.rs`). `cd` into a directory whose name carries an OSC sequence
      retitles the window. ADR-0015 T1.
- [x] **F3 — a parse diagnostic sanitises the echoed line but not its own message.**
      `crates/ono-cli/src/report.rs`. ADR-0015 T1.
- [x] **F4 — `sanitise` lets `\n` and `\t` through, so a value forges a table row.** A cell
      containing `"evil\nroot      1"` renders as two terminal lines, the second indistinguishable
      from a real row. Widths are also measured on unsanitised text, so escapes misalign columns.
      `crates/ono-render/src/theme.rs`. ADR-0015 T1.
- [x] **F6 — resolution and execution disagree about a relative `PATH` entry.** `explain` stats a
      relative entry against the *process* working directory while the command runs with the
      *session's*, so `explain foo` reports one binary and `foo` runs another after a `cd`.
      `crates/ono-cli/src/resolve.rs` versus `crates/ono-cli/src/eval.rs`. ADR-0015 T10/T11 — it
      defeats that row's only stated mitigation.
- [x] **F7 — the history file is world-readable and ships with no redaction patterns.** Created at
      the ambient umask (0644, in a 0755 directory), and `Policy::default()` has an empty pattern
      list, so `deploy --password=hunter2` is stored verbatim. ADR-0015 T8; the row's named test
      supplies its own pattern, so it proves the mechanism rather than the product.
      `crates/ono-history/src/{store,policy}.rs`, `crates/ono-cli/src/repl.rs`.

Should-fix:

- [x] **F9 — fixed.** The prompt derives elevation from the kernel's effective uid: a root shell
      shows ` root` in `ui.prompt.root` and prompts with `#` (spec §17.2). Pinned from both
      sides in `ono-cli/tests/signals.rs::should_make_an_elevated_prompt_impossible_to_miss`.
- [x] **F10 — fixed** (as a side effect of the depth-guarded block recovery landed in the
      security sweep): every hostile wall — parens, brackets, blocks, `if`-chains — now parses
      20 000 deep in under 40 ms debug. The regression guard is
      `ono-parser/tests/robustness.rs::should_stay_linear_on_a_wall_of_unbalanced_parentheses`.
      Previously: **quadratic on unbalanced nesting** (24.8 s at 20 000).
- [x] **F11 — fixed.** The frontier holds paths, not descriptors: each directory re-opens from
      the held root through `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)`, so at most two
      descriptors are ever open and the T14 no-redirect property survives the change — a
      swapped component fails loudly instead of being followed. Pinned under a real 64-fd limit
      in `ono-cli/tests/native.rs::should_walk_a_wide_tree_without_hoarding_descriptors`.
      Previously: **one open descriptor per pending directory.**
- [ ] **F12 — the trust store's default policy is trust-on-first-use**, which contradicts ADR-0015
      T5's "an unknown key is refused, not prompted past". `crates/ono-protocol/src/trust.rs`.
      Either the ADR or the default has to move.
- [x] **S1 (F13) — fixed.** `ProviderMutation` refuses a selection over the bulk threshold (10,
      a constant until configuration reaches invocations) with `safety.confirmation_required`
      naming the scope, before the first action; `--confirm` proceeds. `stop process` declares
      the option too. Pinned in `ono-command/tests/mutations.rs`. Previously: **the contract
      advertised a bulk-mutation guard nothing implements.** Four command
      contracts (`docs/spec/commands/file.yaml` twice, `network.yaml`, `kuang.yaml`) declare a
      `confirm` option documented as "without it, a selection over the configured threshold fails
      with `safety.confirmation_required` in a script (spec §11.6, §17.4)".
      `ProviderMutation::run` in `crates/ono-command/src/impls/mutate.rs` forwards it verbatim as
      an opaque argument and contains no threshold and no `safety.confirmation_required` path. A
      documented safety guard that does not exist is worse than no guard, because someone will
      rely on it. This is why `docs/ACCEPTANCE.md` §4.4's "destructive operations show scope
      before acting" cannot be ticked.
- [x] **S2 (F8) — fixed.** The systemd dry-run branches now answer `skipped` with what would
      have happened — the contract `ono-provider-linux` always kept — and the test that asserted
      a claimed change asserts the report; a declared `--dry-run` option travels in the action's
      own field rather than as an ignorable argument. Previously: **`Action::as_dry_run()` was
      unreachable, and one test encoded the wrong contract.**
      Nothing constructs a dry run: both call sites in `crates/ono-command/src/impls/mutate.rs`
      leave it false, no contract declares the option, and the `is_dry_run()` branches in
      `crates/ono-provider-systemd/src/provider.rs` are dead. Latent rather than live — but
      declaring `--dry-run` on a contract would make the flag arrive as an ordinary argument and
      the mutation would *run*. The systemd branches also report a completed change rather than
      `skipped`, and `crates/ono-provider-systemd/tests/service.rs` asserts that, so the wrong
      behaviour is currently guarded by a passing test. `ono-provider-linux` does it correctly.

Accepted for now, with the reason recorded so the decision is not re-made by accident:

- **F14** — bidirectional and other format characters pass the sanitiser, because
  `char::is_control()` covers only the `Cc` category. Trojan-Source display spoofing of a
  filename. Proposed as an extension of T1.
- **F15** — an empty `PATH` element resolves to the working directory. Deliberate, matches every
  other shell, and `explain` prints the absolute path it reached.
- **F16** — the history and trust-store temporary files are predictable and opened without
  `O_EXCL`. Only reachable in a directory another user can write, which F7 makes likelier than it
  should be; fix alongside F7.
- **F17** — a residual TOCTOU window remains between confirming a process's identity and
  signalling it. `pidfd_open`/`pidfd_send_signal` would close it; T13 claims only "re-read before
  signalling", which the code does.
- **F18** — `O_NOFOLLOW` does not stop `openat` descending into a bind mount;
  `openat2(RESOLVE_NO_XDEV)` would. T14 claims only that the walk cannot leave the tree *by name*,
  which holds.
- **F19** — `is_executable_file` tests `mode & 0o111` rather than `access(X_OK)`.
- **F20** — `FdPlan::normalise` opens `/dev/null` in a loop up to the target descriptor, so
  `9999>file` costs ten thousand opens. Self-inflicted.

### What the security review attacked and could not defeat

Worth keeping, because a mitigation that survived a real attempt is the most useful line in a
security review — and because re-testing these later costs nothing if they are written down:

- **T1/T9 at the render boundary.** `Theme::paint` sanitises *before* choosing colour, so a pipe
  and a file are covered as well as a terminal; `View::Raw` re-sanitises; every cell, tree node
  and key goes through it; no setting disables it. `\n` (F4) was the only hole found.
- **T4, poisoned completion.** Candidates are filenames, never executed, and painted before
  display.
- **T7, decoder bombs.** JSON and YAML nesting refused past their depth limits at 200 and beyond;
  a 3^N YAML alias fan-out refused at N=8; the netlink decoders check every length against the
  remaining slice and advance by at least one aligned header per step. No overflow, no unbounded
  allocation, no non-terminating input found.
- **T13, identity completeness.** No path reaches a signal with a bare pid: every target carries
  `(pid, started)` from a record or from `providers.resolve()`, and a mismatch refuses.
- **T14, symlink swap.** Each directory is opened once relative to its parent's held descriptor
  with `O_NOFOLLOW`, and no path is ever re-resolved. Could not escape the tree by name.
- **T5/T6, refusal semantics.** A changed key is `remote.host_key_changed` carrying both
  fingerprints, with no continue-anyway; re-trusting is a separate deliberate act.
- **ADR-0019, no word splitting.** `has_pattern` is computed from the *source* characters, so a
  `*` arriving inside a variable's value cannot glob.
- **Environment propagation.** A child gets the session environment and nothing internal.
- **ADR-0007's `unsafe` audit.** Seven blocks, all in `ono-process`. The `pre_exec` path calls only
  `dup2`, `setsid`, `ioctl(TIOCSCTTY)` and `signal`; the one non-libc call,
  `io::Error::last_os_error()`, builds a non-allocating representation. No `format!`, no lock, no
  Rust I/O, no panicking index. No signal-mask inheritance across `exec` and no descriptor leak.

---

## Deferred / blocked

**Two declared relations have no provider evidence (2026-08-28, S2, ADR-0135).** Both are
declared in `docs/spec/spatial/relations.yaml`, claimed by no provider in `docs/spec/providers/`,
and produce no edges. Not faked, not removed:

- `service.depends_on` — `ono-provider-systemd` reads `ListUnits`, which carries no dependency
  information; `Requires`/`Wants`/`After` need a `Get` per unit over D-Bus. §13 lists
  dependencies and dependents among a service place's groups, so this is real work with its own
  cost class, its own `ono.service/1` surface and its own acceptance case — exit test: a service
  place whose `dependency` exit names `network-online.target` on the container fixture.
- `socket.accepts_connection` — neither `sock_diag` nor procfs relates an accepted connection to
  the listener it came from, and matching by local port would be a guess §11.5 has no value for.
  Exit test: none until a kernel interface supplies the link.

**The v0.4 RED suites are delivered (2026-08-28, S1–S11b).** The nine
`crates/ono-cli/tests/spatial_*_missing.rs` files (175 tests) and the ten
`docker/acceptance/cases/09x-spatial-*.case` scenarios (139 assertions) are un-ignored, renamed
and green; `xtask/tests/spatial_evidence.rs` fails the gate if a `*.case.v04` file returns or if
a test `docs/ACCEPTANCE.md` §4.7 names as a proof is missing or ignored. Nothing in this section
is deferred any more; the files keep their `_missing` names because renaming them would rename
113 proofs the checklist points at, which is a `refactor` of its own.

**The three questions the suites could not settle are decided** (2026-08-28, confirmed by the
user), so the first increment starts from a fixed contract rather than from an assumption:

1. **ADR-0124** — spatial verbs resolve by v0.2 §6.5 and take the bare name, except where a
   widely used program already answers to it: `find` keeps its target word, so the spatial
   search is **`find place`** beside the existing `find file`/`find command`, and bare `find`
   keeps reaching findutils through the v0.3 adapter (acceptance case 087 stays green). `look`
   shadows util-linux `look`, which stays reachable as `exec:look`. **The RED suites assume the
   bare `find`: the increment delivering §6.8 rewrites those assertions in the same commit**
   (`spatial_navigation_missing.rs`, `spatial_topology_missing.rs`, cases 090/091/097).
2. **ADR-0125** — the fourteen `spatial.*` conditions of §40 become the family `spatial`,
   `Ono-Sendai-E1001`–`E1014`, in `docs/spec/errors.yaml`; no separate `spatial-errors.yaml`,
   because one taxonomy in two files is the drift `spec-check` exists to catch.
3. **ADR-0126** — the registry lives in `docs/spec/spatial/{spatial,spaces,relations,landmarks}.yaml`,
   following `docs/spec/kuang/` rather than §41's flat spelling.

Further readings the suites fixed, each written at its test: the `PlaceView`/`SpatialMap` field
names §22 and §20.1 give verbatim are pinned, the nesting §6.1 leaves open is not; `map --zoom`,
`map --expand`, `map --focus`, `map --live`, `map links` are the non-interactive spellings for
§8.1, §8.3, §23.4, §25 and §19.3; the full-screen map is observable as the alternate screen
buffer, with Enter/Backspace/Esc/Ctrl-C from §23.3 and §43.4; `spatial.landmark.*` and the
eleven `spatial.*` settings of §47 ride the typed settings catalogue of ADR-0094.

---

## Notes for whoever starts phase A

- Switch to `implementation` before your first edit. The gate refuses to run on `main`.
- The workspace is green as delivered. Confirm it (`scripts/gate.sh`) before your first edit, so
  a later red gate is unambiguously yours.
- `crates/ono-cli/src/main.rs` is scaffolding: it answers `--version` and `--help` and refuses
  everything else. Replacing its argument handling with the real interpreter is expected and
  needs no ADR; the three acceptance cases guarding it must keep passing.
- Crate names not yet created (`ono-parser`, `ono-value`, `ono-pipeline`, …) come from spec
  section 24.2 with the `ono-` prefix. Create them as the phase needs them, not upfront.
- Add the acceptance case in the same increment as the capability. A feature without a case in
  the container does not count as delivered (`docs/ACCEPTANCE.md` section 2).
- The specification is read-only and checksum-enforced. When it is ambiguous, wrong or in your
  way, write an ADR with a `Spec deviation` heading and implement your decision — never edit the
  spec (AGENTS.md section 5.1, ADR-0003).
