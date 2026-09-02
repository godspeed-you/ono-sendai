//! The reference cardinality profiles the rest of the performance work is measured against
//! (v0.4.1 §32.1, §32.2, Appendix F).
//!
//! §32.1 is the premise:
//!
//! > v0.4.1 MUST stop treating one small fixture passing a latency budget as sufficient proof
//! > that a spatial operation is performant.
//!
//! A curve needs more than one point, and two runs are only comparable if they used the same
//! points. `docs/spec/hardening/performance_profiles.yaml` is where Appendix F's numbers are
//! written down once; this suite is the comparison that keeps the registry and the fixtures
//! saying the same thing, in both directions.
//!
//! §32.2 fixes the rule that makes a fixture worth anything — *"provider/planner code exercised
//! by the benchmark MUST match production logic"* — so every fixture here creates **objects**:
//! real child processes and real listening sockets the kernel lists, which the production
//! providers read the way they read anything else. That half is proven where a provider can be
//! reached: `crates/ono-cli/tests/spatial_first_output.rs` counts a placed population through
//! `get process` and `get socket`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::collections::BTreeMap;

use ono_testkit::{
    BuiltBy, PROFILE_L, PROFILE_M, PROFILE_S, ProcessPopulation, Profile, ProfileDeclaration,
    SkipReason, SocketPopulation, declared_payloads, declared_profiles, payload, skipped,
};

/// Appendix F.1 and F.2, typed out from the specification rather than read from the registry.
///
/// The registry is the single home of these numbers for everything that *uses* them; a check
/// that read the registry to check the registry would agree with itself. So the specification's
/// own table is written here once, in the one place whose job is to disagree.
const APPENDIX_F: [(&str, usize, usize, usize, usize); 3] = [
    // id, processes, graph nodes, edges, sockets
    ("S", 100, 500, 2_000, 1_000),
    ("M", 1_000, 5_000, 25_000, 10_000),
    ("L", 10_000, 50_000, 250_000, 100_000),
];

/// Appendix F.3's three payload sizes.
const APPENDIX_F3: [(&str, usize); 3] = [
    ("small", 100),
    ("medium", 10 * 1024),
    ("large", 1024 * 1024),
];

#[test]
fn should_build_every_declared_profile_at_the_cardinality_the_registry_states() {
    let declared = declared_profiles();

    // 1. The registry says what Appendix F says.
    let by_id: BTreeMap<&str, &ProfileDeclaration> = declared
        .iter()
        .map(|declaration| (declaration.id.as_str(), declaration))
        .collect();
    for (id, processes, graph_nodes, edges, sockets) in APPENDIX_F {
        let declaration = by_id.get(id).unwrap_or_else(|| {
            panic!(
                "v0.4.1 §32.2 requires Profile {id}; \
                 docs/spec/hardening/performance_profiles.yaml declares {:?}",
                by_id.keys().collect::<Vec<_>>()
            )
        });
        assert_eq!(
            (
                declaration.processes,
                declaration.graph_nodes,
                declaration.edges,
                declaration.sockets
            ),
            (processes, graph_nodes, edges, sockets),
            "Profile {id} is declared at a cardinality Appendix F does not state"
        );
    }
    assert_eq!(
        declared.len(),
        APPENDIX_F.len(),
        "the registry declares a profile Appendix F does not: {:?}",
        by_id.keys().collect::<Vec<_>>()
    );

    // 2. The constants the fixtures are built from say the same thing.
    for constant in [PROFILE_S, PROFILE_M, PROFILE_L] {
        let declaration = by_id
            .get(constant.name)
            .unwrap_or_else(|| panic!("`PROFILE_{}` is declared by nobody", constant.name));
        assert_eq!(
            (
                declaration.processes,
                declaration.graph_nodes,
                declaration.edges,
                declaration.sockets
            ),
            (
                constant.processes,
                constant.graph_nodes,
                constant.edges,
                constant.sockets
            ),
            "`PROFILE_{}` and its declaration are two different profiles; §52.2 allows one home \
             for a number",
            constant.name
        );
    }

    // 3. Every profile is actually built somewhere, at the cardinality it declares.
    for declaration in &declared {
        match declaration.built_by {
            BuiltBy::Gate => {
                let processes = ProcessPopulation::of(declaration.profile());
                assert_eq!(
                    processes.len(),
                    declaration.processes,
                    "Profile {} declares {} processes and its fixture placed {}",
                    declaration.id,
                    declaration.processes,
                    processes.len()
                );
            }
            BuiltBy::Benchmark => {
                // Too large for every gate run, small enough for a developer machine: the
                // benchmark command and the `--ignored` watchdog build it. What this suite can
                // check is that the constant they build from exists and agrees, which step 2 did.
                assert!(
                    Profile::named(&declaration.id).is_some(),
                    "Profile {} is declared `benchmark` and no constant names it, so nothing can \
                     build it",
                    declaration.id
                );
            }
            BuiltBy::Container => {
                for fixture in declaration.fixtures() {
                    let path = repository_root().join(fixture);
                    assert!(
                        path.is_file(),
                        "Profile {} names `{fixture}` as the fixture that builds it and there is \
                         no such file",
                        declaration.id
                    );
                }
            }
        }
    }

    // 3b. The socket axis is built where it declares itself buildable, which is not always where
    // the process axis is: Profile L's ten thousand processes belong to the container and its
    // hundred thousand listening sockets do not.
    for declaration in &declared {
        if declaration.sockets_built_by != BuiltBy::Gate {
            continue;
        }
        let sockets = match SocketPopulation::try_of(declaration.profile()) {
            Ok(sockets) => sockets,
            Err(shortfall) => {
                // The registry states the descriptor limit beside the cardinality that fixes it,
                // and a host below it has not found a defect in the product (ADR-0517).
                skipped(
                    SkipReason::MissingPrivilege,
                    &format!(
                        "Profile {} places {} listening sockets and {shortfall}",
                        declaration.id, declaration.sockets
                    ),
                );
                continue;
            }
        };
        assert_eq!(
            sockets.len(),
            declaration.sockets,
            "Profile {} declares {} sockets and its fixture opened {}",
            declaration.id,
            declaration.sockets,
            sockets.len()
        );
    }

    // 4. Appendix F.3's payload sizes exist, and a payload is the size it claims.
    let payloads = declared_payloads();
    for (id, bytes) in APPENDIX_F3 {
        let declaration = payloads
            .iter()
            .find(|candidate| candidate.id == id)
            .unwrap_or_else(|| panic!("v0.4.1 Appendix F.3 requires the `{id}` payload profile"));
        assert_eq!(
            declaration.bytes, bytes,
            "the `{id}` payload profile is declared at {} bytes and Appendix F.3 states {bytes}",
            declaration.bytes
        );
        assert_eq!(
            payload(declaration.bytes).len(),
            bytes,
            "a payload built for the `{id}` profile must be exactly {bytes} bytes, or the byte \
             budget it exercises is measuring something else"
        );
    }
}

#[test]
fn should_rebuild_the_same_profile_from_the_same_declaration() {
    // §32.4 stores results "in a machine-readable baseline file tied to the reference
    // environment", which is only meaningful if the fixture behind a figure is reconstructible.
    // Two builds of one declaration are the same system, and the first leaves nothing behind.
    let declaration = declared_profiles()
        .into_iter()
        .find(|candidate| candidate.built_by == BuiltBy::Gate)
        .expect("at least one profile is buildable by the gate");
    let profile = declaration.profile();

    let before = ProcessPopulation::of(profile).len();
    let after = ProcessPopulation::of(profile).len();
    assert_eq!(
        before, after,
        "Profile {} built twice from one declaration placed {before} processes and then {after}",
        profile.name
    );

    let sockets_before = SocketPopulation::of(profile).len();
    let sockets_after = SocketPopulation::of(profile).len();
    assert_eq!(
        sockets_before, sockets_after,
        "Profile {} built twice from one declaration placed {sockets_before} sockets and then \
         {sockets_after}",
        profile.name
    );

    // A payload of a stated size is the same bytes every time, so a materialization benchmark
    // measures the budget rather than the generator.
    assert_eq!(
        payload(4096),
        payload(4096),
        "a payload profile must be deterministic"
    );
}

/// The repository root, from this crate's manifest.
fn repository_root() -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path
}
