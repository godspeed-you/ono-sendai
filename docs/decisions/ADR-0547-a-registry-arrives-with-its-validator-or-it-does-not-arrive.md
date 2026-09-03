# ADR-0547: A registry arrives with its validator, or it does not arrive

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §9.3, §9.5, §12.1–§12.4, §32.4, §37.2, §52.1, §52.2, §52.3, §54.3, §55.1,
  Appendix A, Appendix C, Appendix F; ADR-0451, ADR-0456, ADR-0489, ADR-0490, ADR-0501, ADR-0546
- Issues: #117
- Decided by: agent (autonomous)

## Context

§52.3 is one sentence with one hard word in it:

> `scripts/gate.sh` MUST validate every machine-readable contract for schema correctness and
> cross-reference integrity.

**Every.** §52.1 listed seven contract domains as *"required or strongly recommended"*. By the time
this issue was worked, `docs/spec/hardening/` held **fifteen** files, because the rest arrived with
the phases that needed them — a streaming classification for Appendix E, a capture inventory for
§26.1, a module architecture for Appendix I, a terminology contract for §19, a verification
sequence for §47.5, three performance registries for §32 and §37.

Each was validated by whichever crate happened to consume it. That is adequate for a registry a
crate consumes and it is *silence* for one nothing does, and the repository had an instance.
`docs/STATE.md` recorded it while H3 was running:

> `remote_limits.yaml` is not validated by `xtask spec-check`; only
> `crates/ono-remote/tests/limits.rs` checks its cross-references, from the crate that enforces
> them.

Two more items were on the same board and belong here:

- §52.3's *first* named failure — an unknown capability id in an authorization fixture — had no
  checker. The second, an unknown control id in a KUANG tier definition, has been checked since H4.
- `crates/ono-cli/src/limits.rs`'s doc comment claimed `unit` and `enforced_by` come from the
  settings catalogue. `SettingSpec` has neither field.

## Decision

**`docs/spec/hardening/registries.yaml` indexes the directory, and a contract with no row in it
fails the gate.**

### 1. The index is what makes "every" checkable

Seventeen rows, one per file, each naming the `xtask` check that validates it, the crates that
consume it, what it holds and the sections that ask for it. `check_registry_inventory` then reads
both directions:

- a `.yaml` or `.json` file in the directory with no row is a contract nobody claimed;
- a row whose `validated_by` names a function that does not exist is a claim nobody honoured;
- a row's `file` that does not exist is an index that outlived its registry.

`registries.yaml` indexes itself, because an index that exempts itself is one more contract nobody
validates.

**`validated_by` must live under `xtask/`.** This is the decision that keeps the index from
becoming a restatement of the status quo. §52.3 puts the validation *in the gate*; a crate test
that reads the same file is a consumer agreeing with itself, and it disappears the day the crate
stops reading the file. Where a crate does consume a registry it is recorded in `consumed_by`,
because §52.2's single source of truth is a claim about consumers — *"runtime defaults, generated
help/reference and tests consume the same source"* — and a registry with no consumer is a table
nothing acts on.

Two registries needed a gate validator written for them: the three performance files got
`xtask::perf::check_registries`, and `remote_limits.yaml` got `check_remote_limits`.

### 2. §52.1's seven domains each say where they live, including the one that is not a file

The index's `domains` block maps each of §52.1's seven names to this repository's answer, and two
of the answers are not one-to-one:

- **`materialization_limits` is `limits.yaml`.** §55.1 gives every hardening limit one dotted
  configuration key and §52.2 gives every figure one home, so materialization, capture, history and
  the remote ceilings share one catalogue (ADR-0456). Splitting the catalogue by subsystem to match
  §52.1's naming would be §52.2's second copy, one directory level up.
- **`release_inputs` is not a committed registry at all.** Appendix H's manifest is a property of
  one build — the commit, the container digests, the action SHAs, the tool versions, the workflow
  run identity that produced a specific set of bytes. `cargo xtask build-manifest` writes it and
  the release workflow publishes it beside the packages (ADR-0451). A committed copy would be a
  statement about a build nobody ran. What *is* committed is the input side, the pins
  `xtask::supply_chain` scans on every gate run, and `provenance::check_manifest_is_emitted` fails
  the gate if the workflow stops emitting the manifest.

Stating both in the index is the point: §52.1 names seven domains, and a domain nothing accounts
for reads as a domain nobody did.

### 3. `remote_limits.yaml` is the shape §52.2 wants, and now the pointers are followed

H3 built it holding **no numbers** — one row per ceiling, pointing at the `limits.yaml` key that
holds the figure — which is exactly §52.2's *"a number such as `max_connections = 32` MUST not be
independently typed into five files"*. Nothing in the gate followed those pointers.
`check_remote_limits` resolves all four per row:

| Pointer | Resolved against |
| --- | --- |
| `limit_key` | the keys `docs/spec/hardening/limits.yaml` declares |
| `field` | the accessor `ono_protocol::Limits` answers with |
| `refusal` | the stable error names `docs/spec/errors.yaml` declares (`null` where the peer is dropped before a frame can carry one) |
| `audit` | the §14.1 event class the protocol crate spells |

And it enforces the shape itself: **a numeric value anywhere in a ceiling row is a failure.** A row
that states a figure has stopped being a pointer, and §52.2's five copies start with the second one.

### 4. §52.3's first named failure now costs something

> Unknown capability IDs in an authorization fixture … MUST fail the gate.

It looked covered and was not. `ActionGrant` refuses a malformed id at construction, so `*` and
`process.` cannot be stored; an id naming nothing is denied at dispatch (Appendix C). Both are
*runtime* behaviour, proved by tests. A fixture that grants `process.invented` is a test asserting
against a capability the product does not have, and the day the id is a typo for a real one, the
test still passes and nobody is told.

`check_authorization_fixtures` reads every `actions=` grant in `crates/**` and
`docker/acceptance/**` — the store line of §9.3, which is how an authorization fixture is written
in a test, in a doc comment and in an acceptance case alike — and resolves each id against
`docs/spec/capabilities.yaml`.

Some fixtures name an invalid id on purpose, because the refusal *is* the subject: §9.5 forbids a
wildcard, and `actions=process.*` exists to prove the parser refuses it. Those are **listed** in
the index with a reason and the test that names them, rather than pattern-matched — and the gate
requires a listed id to be genuinely undeclared, so the exemption cannot become a hiding place for
a typo.

The scan is textual and covers the stored form. A capability id passed as a Rust literal into a
helper that later parses it is not caught; what that fixture builds is a grant the runtime checks
and dispatch denies, and reaching it would need a Rust parser in `xtask` for a question this size.
The stored fixture is what §52.3 names, and it is what is checked.

### 5. The doc comment says what `inspect limits` actually answers

`crates/ono-cli/src/limits.rs::rows` emits key, value, bytes, type, layer, min, max and
description. It never emitted `unit` or `enforced_by`, and `SettingSpec` has neither field. The
comment now says so and says why: `unit` is a rendering hint the type already implies for a byte
size, and `enforced_by` names a crate, which is a fact about the product rather than about this
session's configuration. §54.3 asks what the shell will enforce; who enforces it is the registry's
job and, for a boundary, `security_boundaries.yaml`'s.

## Consequences

Easy: adding a registry now has an obvious shape — write the file, write the `xtask` check, add the
row. The gate refuses the first two without the third, which is the property §52.3 asked for and
the reason `remote_limits.yaml` could sit unvalidated for a phase.

Hard: `validated_by` must be under `xtask/`, so a registry a crate validates well is still owed a
gate check. That is deliberate, and it cost this increment two new validators. The alternative —
accepting a crate test as the validator — would have made the index a description of the state the
issue exists to change.

Also: `check_authorization_fixtures` reads every `.rs` and `.case` file under `crates/` and
`docker/acceptance/`, which is the same sweep `check_declared_options` already does. It is fast
enough to be unmeasurable beside `cargo test` and it is the only way to catch a fixture wherever
somebody writes one.

Encoded by: `xtask/tests/contracts.rs::should_validate_every_machine_readable_hardening_contract_in_the_gate`,
`::should_report_a_hardening_registry_no_gate_check_validates`,
`::should_report_a_registry_index_naming_a_gate_check_that_does_not_exist`,
`::should_hold_every_hardening_limit_against_the_value_the_shell_uses`,
`::should_report_a_remote_ceiling_whose_limit_key_names_nothing`,
`::should_report_a_remote_ceiling_that_types_its_number_instead_of_pointing_at_it`,
`::should_reject_an_unknown_capability_id_in_an_authorization_fixture`,
`::should_find_every_capability_this_repositorys_authorization_fixtures_grant`,
`::should_report_a_deliberately_invalid_fixture_id_the_registry_actually_declares`.

## Alternatives considered

**Let each registry be validated by whichever crate consumes it, and record that in the index.**
Cheaper, and it describes what the repository already did. Rejected by §52.3, which names
`scripts/gate.sh` as the validator, and by the concrete counter-example: `remote_limits.yaml` has
no runtime consumer that reads the *file*, so its cross-references were checked by one crate's
integration test and by nothing else.

**Split `limits.yaml` into `materialization_limits.yaml` and the rest, to match §52.1's naming.**
Rejected: §52.2 is the stronger rule, and one catalogue with one key per figure is what obeys it.
The index records the mapping instead, which costs a sentence and no duplication.

**Generate a `release_inputs.yaml` in the repository so all seven domains are files.** Rejected: it
would be a manifest describing a build nobody ran, and ADR-0451 already established that the
manifest is derived from the build that produces the bytes. A registry that is wrong by
construction is worse than a domain the index explains.

**Pattern-match deliberately invalid fixture ids — anything containing `*`, anything ending in a
dot.** Rejected: the class of ids a fixture may legitimately name is not a syntax, it is a set of
decisions. `process.invented` is well-formed and must still be declared as deliberate. A list with
a reason per entry says which test needs it; a pattern says nothing and grows.
