//! The wire shapes of v0.6 §48 against the domain types they mirror, and §49.3's boundary.
//!
//! `ono-kuang-protocol` deliberately does not depend on `ono-change-core`: the wire is JSON and
//! the protocol crate stays free of the domain (spec §31.61). That leaves a field renamed on one
//! side and not the other as a value that silently stops arriving, so the two are held together
//! here, in the one crate that may depend on both.
//!
//! §49.3 is the other half: "An AI suggestion that a change is reversible MUST NOT change
//! `RecoveryCoverage` unless a real recovery provider proves it." The tests below assert the
//! absence of that path in a way that would fail if somebody added one.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_core::{
    ConsistencyClass, EffectDomain, RecoveryCandidate, RecoveryObjective, RecoveryScope,
    RestoreMethod, RiskClass, RiskDimension, RiskFinding,
};
use ono_kuang_protocol::{
    RecoveryCandidateWire, RecoveryScopeWire, RiskFindingContribution, VerificationObserveParams,
};
use serde_json::json;

// --- the wire mirrors the domain, field for field ----------------------------------------------

#[test]
fn should_carry_every_recovery_scope_field_from_the_wire_into_the_domain_type() {
    // `RecoveryScope` is four facts, and every one of them matters: §13.4 and §14.3 turn on
    // `covers` being what the mechanism actually holds rather than what a path suggested.
    let document = json!({
        "domain": "tank/var",
        "domain_kind": "zfs-dataset",
        "covers": ["/var/lib/postgresql", "/var/log"],
        "host": "deck-01",
    });
    let wire: RecoveryScopeWire = serde_json::from_value(document).expect("the wire shape reads");
    let domain = RecoveryScope::new(
        wire.domain_kind.as_str(),
        wire.domain.as_str(),
        wire.host.as_str(),
    )
        .covering(wire.covers[0].clone())
        .covering(wire.covers[1].clone());
    assert_eq!(domain.domain(), "tank/var");
    assert_eq!(domain.domain_kind(), "zfs-dataset");
    assert_eq!(domain.host(), "deck-01");
    assert_eq!(domain.covers().len(), 2);
}

#[test]
fn should_carry_every_recovery_candidate_field_from_the_wire_into_the_domain_type() {
    // Appendix A.3's candidate, round-tripped. A field the wire dropped would show up here as a
    // domain value that could not be built from what arrived.
    let document = json!({
        "provider": "dev.example.pg.recovery-provider.database",
        "scope": {
            "domain": "main",
            "domain_kind": "postgres-database",
            "covers": ["main.public"],
            "host": "deck-01",
        },
        "domain": "application-persistent",
        "objective": "preserve-exact",
        "consistency": "application-consistent",
        "restore_method": "provider-native-restore",
        "cost": {"estimated": true, "requires_reboot": false, "requires_offline": false},
        "exclusions": [{"subject": "large objects", "reason": "the dump excludes them by policy"}],
        "creation_requirements": ["a quiesce window"],
        "restore_requirements": ["the database offline"],
        "detail": "a logical dump of the whole database",
    });
    let wire: RecoveryCandidateWire = serde_json::from_value(document).expect("reads");
    let objective =
        RecoveryObjective::from_name(&wire.objective).expect("Appendix A.2's objectives");
    let effect_domain = EffectDomain::from_name(&wire.domain).expect("Appendix A.1's domains");
    let consistency = ConsistencyClass::from_name(&wire.consistency).expect("§11.3's classes");
    let method = RestoreMethod::from_name(&wire.restore_method).expect("Appendix C.1's methods");
    let scope = RecoveryScope::new(
        wire.scope.domain_kind.as_str(),
        wire.scope.domain.as_str(),
        wire.scope.host.as_str(),
    );
    let mut candidate = RecoveryCandidate::new(
        wire.provider.clone(),
        scope,
        effect_domain,
        objective,
        wire.detail.clone(),
    )
    .at_consistency(consistency)
    .restored_by(method);
    for exclusion in &wire.exclusions {
        candidate = candidate.excluding(ono_change_core::RecoveryExclusion::new(
            exclusion.subject.clone(),
            exclusion.reason.clone(),
        ));
    }
    for requirement in &wire.creation_requirements {
        candidate = candidate.needing_to_create(requirement.clone());
    }
    for requirement in &wire.restore_requirements {
        candidate = candidate.needing_to_restore(requirement.clone());
    }
    assert_eq!(
        candidate.provider(),
        "dev.example.pg.recovery-provider.database"
    );
    assert_eq!(candidate.domain(), EffectDomain::ApplicationPersistent);
    assert_eq!(candidate.objective(), RecoveryObjective::PreserveExact);
    assert_eq!(
        candidate.consistency(),
        ConsistencyClass::ApplicationConsistent
    );
    assert_eq!(
        candidate.restore_method(),
        RestoreMethod::ProviderNativeRestore
    );
    assert_eq!(candidate.exclusions().len(), 1);
    assert_eq!(
        candidate.creation_requirements()[0].as_ref(),
        "a quiesce window"
    );
    assert_eq!(
        candidate.restore_requirements()[0].as_ref(),
        "the database offline"
    );
    assert_eq!(candidate.detail(), "a logical dump of the whole database");
    assert_eq!(candidate.scope().domain(), "main");
}

#[test]
fn should_carry_every_risk_finding_field_from_the_wire_into_the_domain_type() {
    // §40.2 prints `reason` instead of "Are you sure?", so a finding that lost it on the wire
    // would produce a gate that cannot say why it is gating.
    let wire: RiskFindingContribution = serde_json::from_value(json!({
        "dimension": "downtime",
        "class": "high",
        "rule": "dev.example.pg.risk.database-restart",
        "reason": "every connected session is dropped for the length of the restore",
    }))
    .expect("reads");
    let finding = RiskFinding::new(
        RiskDimension::from_name(&wire.dimension).expect("§19.1's dimensions"),
        RiskClass::from_name(&wire.class).expect("§19.2's classes"),
        wire.rule.clone(),
        wire.reason.clone(),
    );
    assert_eq!(finding.dimension(), RiskDimension::Downtime);
    assert_eq!(finding.class(), RiskClass::High);
    assert_eq!(finding.rule(), "dev.example.pg.risk.database-restart");
    assert!(finding.reason().contains("connected session"));
}

#[test]
fn should_carry_every_verification_result_field_from_the_wire_into_the_domain_vocabulary() {
    let wire: VerificationObserveParams = serde_json::from_value(json!({
        "check": "check-1",
        "status": "unknown",
        "equivalence": "external-side-effect",
        "detail": "the webhook endpoint did not answer within the timeout",
    }))
    .expect("reads");
    assert_eq!(
        ono_change_core::VerificationStatus::from_name(&wire.status),
        Some(ono_change_core::VerificationStatus::Unknown),
        "§23.5: a check that ran and could not answer is UNKNOWN, never a pass"
    );
    assert_eq!(
        wire.equivalence
            .as_deref()
            .and_then(ono_change_core::EquivalenceDomain::from_name),
        Some(ono_change_core::EquivalenceDomain::ExternalSideEffect),
        "§25.1: a recovery verification names the scope it is evidence about"
    );
}

#[test]
fn should_refuse_a_recovery_candidate_whose_field_the_domain_type_does_not_carry() {
    // The mechanism that makes the mirror hold: the wire shape is closed, so a field invented on
    // one side is a parse failure rather than a value nothing reads.
    let refused = serde_json::from_value::<RecoveryCandidateWire>(json!({
        "provider": "p", "scope": {"domain": "d", "domain_kind": "k", "host": "h"},
        "domain": "filesystem-persistent", "objective": "preserve-exact",
        "consistency": "byte-consistent", "restore_method": "selective-file-restore",
        "detail": "d", "reversible": true,
    }));
    assert!(
        refused.is_err(),
        "`reversible` is exactly the boolean §10.1 forbids a plan from carrying"
    );
}

// --- section 49.3: a model statement is not provider truth -------------------------------------

#[test]
fn should_carry_no_recovery_vocabulary_on_a_model_response_when_one_is_serialised() {
    // §49.3: an AI suggestion that a change is reversible MUST NOT change `RecoveryCoverage`
    // unless a real recovery provider proves it. The structural half of that guarantee is that a
    // model answer has nowhere to put such a claim: `ono-model/1` carries a protocol name, parts
    // and a provider failure, and nothing that names a candidate, an asset or a coverage.
    //
    // A field added to `ModelResponse` that named any of them would fail here.
    let response = ono_model_broker::ModelResponse {
        protocol: "ono-model/1".to_owned(),
        parts: vec![ono_model_broker::Part::Text {
            text: "This change is fully reversible; recovery coverage is PROTECTED.".to_owned(),
        }],
        error: None,
    };
    let json = serde_json::to_value(&response).expect("json");
    let fields: Vec<&String> = json.as_object().expect("a record").keys().collect();
    assert_eq!(
        fields,
        vec!["protocol", "parts", "error"],
        "§49.3: a model answer carries no field a recovery claim could travel in"
    );
    let text = json.to_string();
    for forbidden in [
        "recovery_coverage",
        "protection_level",
        "recovery_candidate",
        "protection_action",
        "consistency",
        "restore_method",
    ] {
        assert!(
            !text.contains(&format!("\"{forbidden}\"")),
            "§49.3: `{forbidden}` must not be a field of a model answer"
        );
    }
}

#[test]
fn should_refuse_to_read_a_model_answer_as_a_recovery_report_when_one_is_attempted() {
    // The behavioural half. A model that answers in exactly the words §49.3 warns about produces
    // a document that no recovery wire shape will accept: the shapes are closed, and none of
    // them has a field a free-form answer could land in. There is no coercion step between the
    // model broker and the recovery domain, and this is what its absence looks like.
    let answer = json!({
        "protocol": "ono-model/1",
        "parts": [{"kind": "text", "text": "reversible: the snapshot covers everything"}],
        "error": null,
    });
    assert!(
        serde_json::from_value::<RecoveryCandidateWire>(answer.clone()).is_err(),
        "§49.3: a model answer is not a recovery candidate"
    );
    assert!(
        serde_json::from_value::<ono_kuang_protocol::RecoveryPrepareParams>(answer.clone())
            .is_err(),
        "§49.3: a model answer is not a protection action"
    );
    assert!(
        serde_json::from_value::<ono_kuang_protocol::RecoveryDiscoverParams>(answer).is_err(),
        "§49.3: a model answer does not discover protection"
    );
}

#[test]
fn should_keep_the_model_and_recovery_host_calls_behind_different_capabilities() {
    // §49.1 and §49.3 together: a model may propose, and proposing reaches no recovery
    // authority. The two surfaces are separate families, so a grant of `model.infer` buys
    // nothing in the recovery domain — and the recovery families are not read-risk families a
    // model grant could be confused with.
    use ono_kuang_protocol::Capability;
    assert_eq!(Capability::ModelInfer.risk(), ono_kuang_protocol::Risk::Mutate);
    for recovery in [
        Capability::RecoveryDiscover,
        Capability::RecoveryPrepare,
        Capability::RecoveryRestore,
        Capability::RecoveryCleanup,
    ] {
        assert_ne!(
            recovery.id(),
            Capability::ModelInfer.id(),
            "§49.3: recovery authority is never reached through the model broker"
        );
        assert!(
            !Capability::ModelInfer.scope_keys().iter().any(|key| {
                recovery.scope_keys().iter().any(|other| other.name == key.name)
            }),
            "a `model.infer` scope key that also scoped `{}` would make one grant read as the \
             other",
            recovery.id()
        );
    }
}

#[test]
fn should_leave_a_coverage_claim_unprovable_by_anything_but_a_provider_capability() {
    // The positive statement §49.3 implies: what changes `RecoveryCoverage` is a candidate from
    // a provider that declared `recovery.discover`, and the capability is what the host checks.
    // A build in which a model answer could produce a candidate would need a second way in, and
    // the only way in is a call whose capability is one of these.
    use ono_kuang_protocol::Capability;
    let recovery_families: Vec<&str> = Capability::ALL
        .iter()
        .filter(|family| family.id().starts_with("recovery."))
        .map(|family| family.id())
        .collect();
    assert_eq!(
        recovery_families,
        vec![
            "recovery.discover",
            "recovery.prepare",
            "recovery.restore",
            "recovery.cleanup",
            "recovery.estimate-cost",
            "recovery.quiesce",
            "recovery.transaction",
        ],
        "§12.2 fixes the seven, and a new one would be a new way to affect coverage"
    );
}
