# ADR-0488: A profile is declared once, and built where it fits

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §32.1, §32.2, §32.4, §37.1, §52.1, §52.2, Appendix F.1, F.2, F.3; ADR-0431;
  issue #82
- Decided by: agent (autonomous)

## Context

§32.1 is the requirement, and it is a negative one:

> v0.4.1 MUST stop treating one small fixture passing a latency budget as sufficient proof that a
> spatial operation is performant.

§32.2 and Appendix F supply the positive half — three topology profiles, three socket profiles,
three payload sizes — and one constraint that decides how they may be built:

> Fixtures MAY synthesize entities where real host creation is impractical, but provider/planner
> code exercised by the benchmark MUST match production logic.

ADR-0431 already settled the shape of a population: real child processes the production provider
reads out of `/proc`, killed and reaped on drop, with `PROFILE_S`/`PROFILE_M`/`PROFILE_L` carrying
§32.2's four numbers each. Two things were left open, and issue #82 is where they close.

The first is where the numbers live. ADR-0431 put them in Rust constants, which is one home but
not a machine-readable one; §52.2 asks for the opposite arrangement — *"a number … MUST not be
independently typed into five files if one contract can generate the others"* — and §32.4 ties a
baseline record to the profile it was measured at, which only means something if the profile is a
declaration rather than a habit.

The second is Profile L, and Profile M with it. ADR-0431 measured a `sleep` child at about
1.35 MB of kernel memory, so ten thousand of them is roughly 13 GB: not a `cargo test`. A
thousand is about 1.35 GB, which a developer machine can do and a gate run should not do on every
increment. Neither observation is a reason to lower a number — Appendix F's cardinalities are the
specification's — so the question is not *what* to build but *where*.

## Decision

**Appendix F is a registry, and every profile declares where a fixture of its size is built.**

### 1. `docs/spec/hardening/performance_profiles.yaml`

The three topology profiles of Appendix F.1 with their Appendix F.2 socket cardinality beside
them, and Appendix F.3's three payload sizes. It joins the six registries §52.1 already asks for,
and `ono_testkit::profile` embeds it with `include_str!`, so a fixture and its declaration cannot
be separately edited.

The specification's own numbers are typed out once more, in
`crates/ono-spatial-query/tests/profiles.rs::should_build_every_declared_profile_at_the_cardinality_the_registry_states`,
because a check that read the registry to check the registry would agree with itself. That test
compares three things pairwise: Appendix F's table, the registry, and the `PROFILE_*` constants
the fixtures are built from. A number can now be wrong in exactly one place before the gate says
so.

### 2. `built_by` says where, and it is not a permission to skip

Each profile declares `gate`, `benchmark` or `container`:

| Profile | `built_by` | Built by |
| --- | --- | --- |
| S | `gate` | `cargo test`, on every gate run |
| M | `benchmark` | `cargo xtask perf` (§37.1) and the `--ignored` watchdog of ADR-0431 |
| L | `container` | `docker/acceptance/fixtures/perf/many-processes.pl` and `many-sockets.pl` |

The profiles test honours the declaration rather than ignoring it: a `gate` profile is built and
counted, a `benchmark` profile must have a constant something can build it from, and a
`container` profile must name fixture scripts that exist. So a profile that stopped being
buildable anywhere fails, and the reason a profile is not built by `cargo test` is written down
in the registry instead of being inferred from its absence.

This is deliberately not a skip (§38.3, §65.10). Nothing here returns early or announces an unmet
precondition; every profile is checked, and what is checked differs because where it is built
differs.

### 3. Sockets are real listeners, for the same reason processes are real processes

`ono_testkit::SocketPopulation` opens `profile.sockets` listening unix sockets in a directory of
its own and closes them on drop. Unix rather than inet, for the reason `many-sockets.pl` already
gives: the acceptance container runs with networking disabled, and a unix listener is the one kind
that always exists. `sock_diag` reports them exactly as it reports any other listener.

`crates/ono-cli/tests/spatial_first_output.rs::should_show_a_placed_socket_population_to_the_socket_provider_when_a_profile_fixture_is_built`
is the §32.2 proof that the fixture is visible to production logic, and it is written as a
*difference*: `ono.socket/1` carries no path for a unix socket, so the fixture cannot name its
members, and the host is running its own listeners throughout. Counting before and after and
requiring the delta is the honest form, and it is the form that does not break when the machine
running the suite opens a socket of its own.

### 4. Payloads are deterministic, and exactly the size they claim

Appendix F.3's three sizes exist so a materialization benchmark measures the byte budget rather
than the generator. `ono_testkit::payload(bytes)` fills from a fixed 32-character cycle, so two
runs at one profile serialize to the same bytes and a figure is comparable across them.

## Consequences

Easy: issue #83's baseline records can name a profile and mean something checkable by it, and
issue #84's `cargo xtask perf` inherits the cardinalities rather than inventing them. A benchmark
that wants Profile M now has one line to write.

Hard: `built_by` is a statement about this repository's machines, and it will age. A profile
declared `benchmark` today because a gate run cannot afford 1.35 GB is a profile that could be
`gate` on a larger runner, and nothing detects that — the registry would simply be conservative.
The alternative, deciding at run time from available memory, would make the gate's coverage depend
on the machine, which is the failure §32.1 is about.

Also hard, and worth stating: `SocketPopulation` needs a descriptor limit above the profile's
socket cardinality. Profile S wants 1 000, which is above the 1 024 soft limit only in company;
Profile M wants 10 000, which is why it is `benchmark`. The panic names the limit, so a host that
cannot host the fixture says so rather than opening fewer sockets and measuring a smaller system.

Encoded by `crates/ono-spatial-query/tests/profiles.rs::should_build_every_declared_profile_at_the_cardinality_the_registry_states`,
`::should_rebuild_the_same_profile_from_the_same_declaration`, and
`crates/ono-cli/tests/spatial_first_output.rs::should_show_a_placed_socket_population_to_the_socket_provider_when_a_profile_fixture_is_built`.

## Alternatives considered

**Generate the constants from the registry with a build script.** One home in the strongest
sense, and it makes the numbers unreadable in the source that uses them. The registries this
repository already has — `limits.yaml`, `streaming_classification.yaml` — all use the
declare-twice-and-compare shape, and a fourth spelling of the same idea is worse than a
consistent one.

**Build Profile L in `cargo test` and accept the memory.** Thirteen gigabytes of kernel memory on
a developer machine that is also compiling. The container already forks that population, and
`many-processes.pl` exists precisely because a fork shares its parent's pages until it writes.

**Lower Profile M or L so a gate run can build them.** The numbers are Appendix F's, and lowering
one to fit a runner is the defect §32.1 names, arriving through the fixture instead of through the
budget.

**Synthesize socket records instead of opening sockets.** It measures the synthesizer. §32.2's
sentence about production logic forbids exactly this, and the container fixture had already
settled the question in the other direction.
