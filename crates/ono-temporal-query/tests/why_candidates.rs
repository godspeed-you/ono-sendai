//! What `why <target>` reads from the ledger (v0.5 §16.3, §32.3, §33; review finding COR-T8).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_spatial_core::SpatialType;
use ono_temporal_query::causal::{CausalContext, CausalEngine, WhyOptions, WhyRequest};

use support::{changed, id, instant, ledger, scope, subject};

/// An evidence reference; `why` explains only a transition evidence stands behind (§16.3).
fn evidence(digit: u8) -> ono_temporal_core::EvidenceId {
    ono_temporal_core::EvidenceId::parse(&format!("v{}", digit.to_string().repeat(24)))
        .expect("a well-formed evidence reference")
}

#[test]
fn should_find_the_targets_own_last_transition_when_it_is_older_than_the_candidate_bound() {
    // Review finding COR-T8: `why <target>` read the newest `max_candidates` events of the whole
    // host, so a target whose last transition was older than that many other events answered "no
    // recorded state transition" although the ledger held one. The target's own events are read
    // for — bounded the same way — and its transition is the one explained.
    let target = subject(SpatialType::Service, "nginx");
    let mut transition = changed(
        "2026-08-31T12:00:00Z",
        "linux.systemd",
        target.clone(),
        "state",
        Some("active"),
        Some("failed"),
    );
    transition.evidence = vec![evidence(1)];
    let mut events = vec![transition.clone()];
    for second in 1..=20 {
        let mut other = changed(
            &format!("2026-08-31T12:01:{second:02}Z"),
            "linux.systemd",
            subject(SpatialType::Service, &format!("other-{second}")),
            "state",
            Some("active"),
            Some("inactive"),
        );
        other.evidence = vec![evidence(2)];
        events.push(other);
    }
    let held = ledger(&events);
    let request = WhyRequest::target(&id(SpatialType::Service, "nginx"), "local/service/nginx");
    let at = instant("2026-08-31T12:05:00Z");

    let candidates = ono_temporal_query::causal::why_candidates(&held, &scope(), &request, at, 4)
        .expect("the ledger answers");
    assert!(
        candidates.len() <= 8,
        "§32.3: two reads of at most the bound each, got {}",
        candidates.len()
    );
    let explanation = CausalEngine::builtin()
        .explain(
            &request,
            &candidates,
            &CausalContext::new(Vec::new()),
            &WhyOptions::at(at).with_max_candidates(4),
        )
        .expect("an explanation");
    assert_eq!(
        explanation.explained_event.as_ref(),
        Some(&transition.event_id),
        "§16.3: the target's own most recent transition is explained, however many other events \
         the host has had since"
    );
}
