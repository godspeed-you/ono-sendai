//! A contributed command's declared risk is a registration fact the host acts on (spec §31.22,
//! §31.46, §31.75; `docs/contracts/kuang/contributions.v1.yaml` → `registration_checks.risk-metadata`;
//! ADR-1000).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_command::{ContributedCommand, Origin};
use ono_core::ErrorCode;

/// A command `dev.example.echo` declares, needing `capabilities` and saying `risk` about itself.
fn declared(capabilities: &[&str], risk: Option<&str>) -> ContributedCommand {
    ContributedCommand {
        id: "dev.example.echo.command.wipe".to_owned(),
        verb: "remove".to_owned(),
        target: "echo-record".to_owned(),
        summary: "Remove the records the package keeps.".to_owned(),
        input: None,
        output: "stream<string>".to_owned(),
        capabilities: capabilities.iter().map(|id| (*id).to_owned()).collect(),
        argument_mode: "expression".to_owned(),
        selectors: Vec::new(),
        options: Vec::new(),
        risk: risk.map(str::to_owned),
        examples: Vec::new(),
        origin: Origin::plugin("dev.example.echo", "0.1.0"),
    }
}

#[test]
fn should_refuse_a_contribution_that_declares_a_mutating_capability_and_no_risk() {
    // The reproduction of #156: `network.connect` is a `mutate` capability, and a command that
    // needs it and says nothing about its risk used to register with no risk shown anywhere.
    let error = declared(&["network.connect"], None)
        .into_contract()
        .expect_err("a mutating command without a risk is not registered");
    assert_eq!(error.code(), ErrorCode::KuangPackageInvalid);
    let said = error.message();
    assert!(
        said.contains("dev.example.echo.command.wipe")
            && said.contains("network.connect")
            && said.contains("risk"),
        "the refusal names the command, the capability that makes it mutating and the missing \
         declaration, got {said:?}"
    );
}

#[test]
fn should_refuse_a_contribution_that_declares_a_destructive_capability_and_no_risk() {
    let error = declared(&["schema.read", "filesystem.write"], None)
        .into_contract()
        .expect_err("a destructive command without a risk is not registered");
    assert_eq!(error.code(), ErrorCode::KuangPackageInvalid);
    assert!(
        error.message().contains("filesystem.write"),
        "the strongest capability is the one named, got {:?}",
        error.message()
    );
}

#[test]
fn should_refuse_a_contribution_whose_risk_is_lower_than_its_capabilities() {
    // `filesystem.write` is `destructive` in `docs/contracts/capabilities.yaml`. A package may
    // raise the risk its capabilities imply, never lower it (ADR-1000).
    let error = declared(&["filesystem.write"], Some("mutate"))
        .into_contract()
        .expect_err("a risk below the capability's is not registered");
    assert_eq!(error.code(), ErrorCode::KuangPackageInvalid);
    assert!(
        error.message().contains("filesystem.write") && error.message().contains("destructive"),
        "the refusal names the capability and the risk it carries, got {:?}",
        error.message()
    );
}

#[test]
fn should_register_a_mutating_contribution_that_declares_its_risk() {
    let contract = declared(&["network.connect"], Some("mutate"))
        .into_contract()
        .expect("a declared risk at the capability's level registers");
    assert_eq!(
        contract.declared_risk(),
        Some(ono_provider_api::Risk::Mutate)
    );
}

#[test]
fn should_register_a_reading_contribution_that_declares_no_risk() {
    // Only a mutating command must say its risk (`registration_checks.risk-metadata`); a command
    // that needs nothing or only reads stays as it registered before.
    declared(&[], None)
        .into_contract()
        .expect("a command with no capability needs no risk");
    declared(&["schema.read", "object.read"], None)
        .into_contract()
        .expect("a reading command needs no risk");
}

// --- `destructive` asks for `--confirm` the way a core command does (spec §17.4, ADR-1001) ----

#[test]
fn should_require_a_confirmation_on_every_run_of_a_destructive_contribution() {
    let contract = declared(&["filesystem.write"], Some("destructive"))
        .into_contract()
        .expect("a destructive contribution registers");
    assert_eq!(
        contract.confirmation(),
        ono_command::Confirmation::Always,
        "a declared `destructive` is the host's confirmation policy, not a label"
    );
    let confirm = contract
        .option("confirm")
        .expect("the confirmation it requires can be written");
    assert!(
        matches!(confirm.declared_type(), ono_command::DeclaredType::Bool),
        "`--confirm` is a flag, as on a core command"
    );
}

#[test]
fn should_refuse_an_unconfirmed_destructive_contribution_as_a_core_command_is_refused() {
    // The refusal a core `confirmation: always` command gives, word for word but for its
    // spelling: same code, same "nothing was changed" remedy (spec §17.4, ADR-0088 §3).
    let registry = ono_command::CommandRegistry::load().expect("the registry loads");
    let core = registry
        .get("ono.change.protect")
        .expect("protect is declared")
        .unconfirmed(false)
        .expect("a core destructive command refuses without --confirm");
    let contributed = declared(&[], Some("destructive"))
        .into_contract()
        .expect("a destructive contribution registers")
        .unconfirmed(false)
        .expect("a destructive contribution refuses without --confirm");
    assert_eq!(contributed.code(), ErrorCode::SafetyConfirmationRequired);
    assert_eq!(contributed.code(), core.code());
    assert_eq!(
        contributed.message(),
        core.message().replace("protect", "remove echo-record")
    );
    assert_eq!(
        contributed.help(),
        core.help()
            .map(|help| help.replace("protect", "remove echo-record"))
            .as_deref()
    );
}

#[test]
fn should_leave_a_reading_or_mutating_contribution_unconfirmed() {
    for risk in [None, Some("read"), Some("observe"), Some("mutate")] {
        let contract = declared(&[], risk)
            .into_contract()
            .expect("the contribution registers");
        assert!(
            contract.unconfirmed(false).is_none(),
            "only `destructive` asks every run, as only a core `confirmation: always` does; \
             {risk:?} asked"
        );
    }
}
