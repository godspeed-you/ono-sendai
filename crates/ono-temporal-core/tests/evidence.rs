//! Evidence: the nine source classes of v0.5 §7.1, the five strengths of §7.2 and the rule that
//! "evidence strength MUST NOT be automatically upgraded", and the derivation chains of §7.3.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_temporal_core::{
    Evidence, EvidenceClaim, EvidenceId, EvidenceSource, EvidenceStrength, TimeRange,
};

use common::{instant, provenance, scope, subject};

#[test]
fn should_order_strengths_strongest_first_when_they_are_compared() {
    let ordered = [
        EvidenceStrength::Authoritative,
        EvidenceStrength::Asserted,
        EvidenceStrength::Derived,
        EvidenceStrength::Correlated,
        EvidenceStrength::Observational,
    ];
    assert_eq!(
        EvidenceStrength::ALL,
        ordered,
        "§7.2 lists them strongest first"
    );
    for pair in ordered.windows(2) {
        assert!(
            pair[0] > pair[1],
            "{:?} must outrank {:?}",
            pair[0],
            pair[1]
        );
    }
}

#[test]
fn should_take_the_weaker_strength_when_two_are_combined() {
    assert_eq!(
        EvidenceStrength::Authoritative.weakest_of(EvidenceStrength::Correlated),
        EvidenceStrength::Correlated,
        "a chain is never stronger than its weakest link (§7.2)"
    );
    assert_eq!(
        EvidenceStrength::Observational.weakest_of(EvidenceStrength::Authoritative),
        EvidenceStrength::Observational,
        "combining is symmetric and never raises"
    );
}

#[test]
fn should_offer_no_way_to_raise_a_strength_when_evidence_is_combined() {
    // The contract §7.2 states negatively is checked positively: every combination of two
    // strengths is at most the weaker of the two, so no call sequence can produce a promotion.
    for a in EvidenceStrength::ALL {
        for b in EvidenceStrength::ALL {
            let combined = a.weakest_of(*b);
            assert!(
                combined <= *a && combined <= *b,
                "{a:?} combined with {b:?} produced {combined:?}, which is stronger than an input"
            );
        }
    }
}

#[test]
fn should_accept_every_canonical_source_class_when_it_is_parsed() {
    for text in [
        "ono.session",
        "ono.recorder",
        "linux.procfs",
        "linux.netlink",
        "linux.systemd-dbus",
        "linux.journald",
        "adapter:ps",
        "remote:web01/linux.procfs",
        "kuang:dev.example.packet-eye/flows",
    ] {
        let parsed = EvidenceSource::parse(text)
            .unwrap_or_else(|| panic!("`{text}` is a §7.1 source class"));
        assert_eq!(
            parsed.as_str(),
            text,
            "a source round-trips through its text"
        );
    }
}

#[test]
fn should_refuse_a_source_when_it_is_not_a_canonical_class() {
    for text in [
        "",
        "linux.ebpf",
        "adapter:",
        "adapter:with/slash",
        "remote:web01",
        "remote:/provider",
        "kuang:package",
        "ono.session ",
        "Adapter:ps",
    ] {
        assert!(
            EvidenceSource::parse(text).is_none(),
            "`{text}` is not a §7.1 source class and must not become one"
        );
    }
}

#[test]
fn should_compose_a_remote_source_when_a_link_and_a_provider_are_given() {
    assert_eq!(
        EvidenceSource::remote("web01", "linux.procfs")
            .expect("a link and a provider compose a remote source")
            .as_str(),
        "remote:web01/linux.procfs"
    );
    assert_eq!(
        EvidenceSource::kuang("dev.example.packet-eye", "flows")
            .expect("a package and a provider compose a KUANG source")
            .as_str(),
        "kuang:dev.example.packet-eye/flows"
    );
    assert_eq!(EvidenceSource::session().as_str(), "ono.session");
    assert_eq!(EvidenceSource::recorder().as_str(), "ono.recorder");
}

#[test]
fn should_record_what_a_derived_claim_rests_on_when_it_is_derived() {
    // §7.3's own example: process membership derived from a cgroup reading and a procfs reading.
    let cgroup = evidence(
        EvidenceSource::parse("linux.systemd-dbus").expect("a class"),
        EvidenceStrength::Authoritative,
        EvidenceClaim::RelationHeld {
            relation: "process.member_of".into(),
            other: subject(1),
            over: TimeRange::at(instant("2026-08-31T14:03:13Z")),
        },
        Vec::new(),
    );
    let procfs = evidence(
        EvidenceSource::parse("linux.procfs").expect("a class"),
        EvidenceStrength::Observational,
        EvidenceClaim::ObjectExisted {
            at: instant("2026-08-31T14:03:13Z"),
        },
        Vec::new(),
    );
    let derived = evidence(
        EvidenceSource::recorder(),
        EvidenceStrength::Derived,
        EvidenceClaim::SourceStatement {
            text: "process 2741 appeared as member of nginx.service".into(),
        },
        vec![cgroup.evidence_id.clone(), procfs.evidence_id.clone()],
    );
    assert_eq!(
        derived.derived_from,
        vec![cgroup.evidence_id, procfs.evidence_id],
        "§7.3: a derived claim MUST reference the evidence it derives from"
    );
}

#[test]
fn should_produce_one_evidence_identity_when_one_observation_is_reported_twice() {
    let claim = EvidenceClaim::ObjectExisted {
        at: instant("2026-08-31T14:03:13Z"),
    };
    let first = EvidenceId::of(
        &EvidenceSource::recorder(),
        instant("2026-08-31T14:03:13Z"),
        &scope(),
        Some(&subject(2741)),
        &claim,
    );
    let second = EvidenceId::of(
        &EvidenceSource::recorder(),
        instant("2026-08-31T14:03:13Z"),
        &scope(),
        Some(&subject(2741)),
        &claim,
    );
    assert_eq!(first, second);
    assert!(first.to_string().starts_with("@v"));
}

fn evidence(
    source: EvidenceSource,
    strength: EvidenceStrength,
    claim: EvidenceClaim,
    derived_from: Vec<EvidenceId>,
) -> Evidence {
    let observed_at = instant("2026-08-31T14:03:13Z");
    Evidence {
        evidence_id: EvidenceId::of(&source, observed_at, &scope(), Some(&subject(2741)), &claim),
        source,
        observed_at,
        source_time: None,
        scope: scope(),
        subject: Some(subject(2741)),
        claim,
        strength,
        raw_ref: None,
        derived_from,
        provenance: provenance("ono.recorder"),
    }
}
