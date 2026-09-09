#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! `plannable_operations:` as §6.1 requires it, and as the shell can read it.

use ono_change_actions::registry::{OperationRegistry, RebootDimension};
use ono_change_core::{
    ActionRole, EffectConfidence, EffectDomain, Idempotency, PreconditionKind, VerificationClass,
};
use ono_command::CommandRegistry;

fn registry() -> &'static OperationRegistry {
    OperationRegistry::embedded().expect("the embedded operation registry typechecks")
}

#[test]
fn should_read_the_embedded_registry_when_the_binary_starts() {
    assert!(
        !registry().is_empty(),
        "§6.1: an empty registry makes every operation unplannable, which reads as conformance \
         and is a broken build"
    );
}

#[test]
fn should_name_only_commands_the_shell_declares() {
    let commands = CommandRegistry::embedded().expect("the command registry typechecks");
    let unknown: Vec<&str> = registry()
        .operations()
        .iter()
        .map(ono_change_actions::Operation::id)
        .filter(|id| commands.get(id).is_none())
        .collect();
    assert!(
        unknown.is_empty(),
        "§6.1 resolves an operation to a provider contract, so a row naming a command that does \
         not exist is a row nothing can use: {unknown:?}"
    );
}

#[test]
fn should_declare_only_mutating_commands_as_plannable() {
    let commands = CommandRegistry::embedded().expect("the command registry typechecks");
    for operation in registry().operations() {
        let contract = commands.get(operation.id()).expect("the id is a command");
        let verb = commands
            .verb(contract.verb())
            .expect("the verb is declared");
        assert!(
            verb.is_mutating(),
            "`{}` is planned as a change and `{}` does not change anything (§3.3)",
            operation.id(),
            contract.verb()
        );
    }
}

#[test]
fn should_give_every_operation_the_mutate_role() {
    for operation in registry().operations() {
        assert_eq!(
            operation.role(),
            ActionRole::Mutate,
            "§3.3: `{}` is a change to target system state",
            operation.id()
        );
    }
}

#[test]
fn should_carry_at_least_one_effect_per_operation() {
    for operation in registry().operations() {
        assert!(
            !operation.effects().is_empty(),
            "§6.1 makes expected direct effects part of what a plannable operation declares, and \
             `{}` declares none",
            operation.id()
        );
    }
}

#[test]
fn should_carry_at_least_one_verification_contract_per_operation() {
    for operation in registry().operations() {
        assert!(
            !operation.verification().is_empty(),
            "§23.1: every plan containing a MUTATE action carries at least one verification \
             contract, and `{}` supplies none",
            operation.id()
        );
    }
}

#[test]
fn should_state_recovery_semantics_or_their_explicit_absence() {
    for operation in registry().operations() {
        assert!(
            operation.recovery_semantics().len() > 40,
            "§6.1 asks for recovery semantics or an explicit lack of them, and `{}` says nothing \
             a reader could act on",
            operation.id()
        );
    }
}

#[test]
fn should_declare_preconditions_sufficient_to_detect_drift() {
    for operation in registry().operations() {
        assert!(
            !operation.preconditions().is_empty(),
            "§7.2: `{}` declares no preconditions, so no drift is detectable",
            operation.id()
        );
    }
}

#[test]
fn should_require_the_recovery_provider_to_still_be_available() {
    for operation in registry().operations() {
        assert!(
            operation
                .preconditions()
                .contains(&PreconditionKind::ProviderAvailable),
            "§7.2 lists `recovery provider still available` beside every other precondition, and \
             `{}` omits it",
            operation.id()
        );
    }
}

#[test]
fn should_never_promise_more_than_expected_outside_the_named_object() {
    // §8.1: a guarantee MUST be scoped to the provider's observable domain. A guaranteed effect
    // in a domain the operation does not itself own would be exactly that overreach.
    for operation in registry().operations() {
        for effect in operation.effects() {
            if effect.confidence() == EffectConfidence::Guaranteed {
                assert_ne!(
                    effect.domain(),
                    EffectDomain::Unknown,
                    "`{}` guarantees an effect in the unknown domain, which §8.1 cannot scope",
                    operation.id()
                );
            }
        }
    }
}

#[test]
fn should_carry_no_probability_anywhere_in_the_document() {
    // §8.3: v0.6 MUST NOT invent percentages such as `82% likely`. There is no field for one, and
    // the prose must not smuggle one in either.
    let yaml = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/contracts/change/actions.yaml"
    ))
    .expect("the contract document is in the tree");
    let section = yaml
        .split_once("plannable_operations:")
        .expect("the document declares plannable operations")
        .1;
    for (number, line) in section.lines().enumerate() {
        assert!(
            !line.contains('%'),
            "§8.3 forbids probability theatre, and line {} reads `{line}`",
            number + 1
        );
    }
}

#[test]
fn should_embed_exactly_what_is_on_disk() {
    let yaml = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/contracts/change/actions.yaml"
    ))
    .expect("the contract document is in the tree");
    let document: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&yaml).expect("the document is valid YAML");
    let json = serde_json::to_string(&document).expect("it transcodes to JSON");
    let from_disk = OperationRegistry::load(&json).expect("it parses");
    assert_eq!(
        &from_disk,
        registry(),
        "ADR-0571: the embedded JSON is a transcoding of the YAML, so the two must never disagree"
    );
}

#[test]
fn should_refuse_a_row_that_declares_no_effect() {
    let document = r#"{"plannable_operations":[{"id":"ono.service.restart","role":"mutate",
        "idempotency":"idempotent","recovery_semantics":"none","effects":[],
        "verification":[{"class":"required","subject":"s","expression":"exists"}]}]}"#;
    let error = OperationRegistry::load(document).expect_err("§6.1 requires effects");
    assert!(
        error.help().is_some_and(|help| help.contains("§6.1")),
        "the refusal names the section that requires them"
    );
}

#[test]
fn should_refuse_a_row_that_declares_no_verification() {
    let document = r#"{"plannable_operations":[{"id":"ono.service.restart","role":"mutate",
        "idempotency":"idempotent","recovery_semantics":"none",
        "effects":[{"domain":"process-runtime","kind":"replace","confidence":"expected",
        "irreversible":false,"explanation":"x"}],"verification":[]}]}"#;
    let error = OperationRegistry::load(document).expect_err("§23.1 requires a contract");
    assert!(
        error.help().is_some_and(|help| help.contains("§23.1")),
        "the refusal names the section that requires it"
    );
}

#[test]
fn should_refuse_a_row_in_a_role_that_changes_nothing() {
    let document = r#"{"plannable_operations":[{"id":"ono.service.restart","role":"verify",
        "idempotency":"idempotent","recovery_semantics":"none",
        "effects":[{"domain":"process-runtime","kind":"replace","confidence":"expected",
        "irreversible":false,"explanation":"x"}],
        "verification":[{"class":"required","subject":"s","expression":"exists"}]}]}"#;
    OperationRegistry::load(document).expect_err("§6.1's list is about operations that mutate");
}

#[test]
fn should_refuse_a_word_outside_a_closed_vocabulary() {
    let document = r#"{"plannable_operations":[{"id":"ono.service.restart","role":"mutate",
        "idempotency":"probably-fine","recovery_semantics":"none",
        "effects":[{"domain":"process-runtime","kind":"replace","confidence":"expected",
        "irreversible":false,"explanation":"x"}],
        "verification":[{"class":"required","subject":"s","expression":"exists"}]}]}"#;
    let error = OperationRegistry::load(document).expect_err("§41.1's list is closed");
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("probably-fine")),
        "the refusal names the word nobody defined"
    );
}

#[test]
fn should_refuse_two_contracts_for_one_operation() {
    let row = r#"{"id":"ono.service.restart","role":"mutate","idempotency":"idempotent",
        "recovery_semantics":"none","effects":[{"domain":"process-runtime","kind":"replace",
        "confidence":"expected","irreversible":false,"explanation":"x"}],
        "verification":[{"class":"required","subject":"s","expression":"exists"}]}"#;
    let document = format!("{{\"plannable_operations\":[{row},{row}]}}");
    OperationRegistry::load(&document)
        .expect_err("§6.1 answers one question, so one operation has one contract");
}

#[test]
fn should_answer_that_an_undeclared_operation_is_not_plannable() {
    assert!(
        !registry().is_plannable("ono.plugin.install"),
        "§6.2: an operation with no contract is not plannable, and silence is the answer"
    );
    assert!(registry().is_plannable("ono.service.restart"));
}

#[test]
fn should_keep_reboot_a_provider_question_only_where_it_is_one() {
    assert_eq!(
        registry()
            .get("ono.package.add")
            .expect("packages are plannable")
            .reboot(),
        RebootDimension::ProviderReported,
        "§30.5: the provider marks reboot requirement or recommendation for a package change"
    );
    assert_eq!(
        registry()
            .get("ono.service.restart")
            .expect("services are plannable")
            .reboot(),
        RebootDimension::None,
        "a restart has no reboot dimension, which is a different statement from `unknown`"
    );
}

#[test]
fn should_weaken_the_idempotency_class_when_an_argument_changes_the_semantics() {
    let write = registry()
        .get("ono.file.write")
        .expect("writing a file is plannable");
    assert_eq!(write.idempotency(), Idempotency::Idempotent);
    let appending = [(std::sync::Arc::from("append"), ono_value::Value::Bool(true))];
    assert_eq!(
        write.idempotency_for(&appending),
        Idempotency::NonIdempotent,
        "§41.1: appending twice is not appending once, so §41.2 must not rerun it blindly"
    );
    assert!(
        !write.idempotency_for(&appending).permits_blind_retry(),
        "§41.2: Ono MUST NOT blindly rerun a non-idempotent action"
    );
}

#[test]
fn should_leave_the_class_alone_when_the_argument_was_not_given() {
    let write = registry().get("ono.file.write").expect("plannable");
    let overwriting = [(
        std::sync::Arc::from("overwrite"),
        ono_value::Value::Bool(true),
    )];
    assert_eq!(write.idempotency_for(&overwriting), Idempotency::Idempotent);
}

#[test]
fn should_declare_the_tolerance_a_service_restart_lives_with() {
    let restart = registry().get("ono.service.restart").expect("plannable");
    assert!(
        restart
            .tolerances()
            .iter()
            .any(|tolerance| tolerance.field() == "cpu"),
        "§7.4's own example: CPU usage moving while a service restart is planned does not \
         invalidate the plan, and the tolerance is contract-declared"
    );
}

#[test]
fn should_give_a_verification_class_every_row_can_be_read_by() {
    for operation in registry().operations() {
        let required = operation
            .verification()
            .iter()
            .filter(|spec| spec.class() == VerificationClass::Required)
            .count();
        assert!(
            required <= operation.verification().len(),
            "`{}` declares more required contracts than contracts",
            operation.id()
        );
    }
}
