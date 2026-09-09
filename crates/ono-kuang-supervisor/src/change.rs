//! Load-time validation of the change and recovery contributions of v0.6 §48.
//!
//! §48.4 states the rule the whole module exists for: "A plugin that can describe impact MUST NOT
//! automatically gain permission to execute the change." A rule enforced at the first call is a
//! rule the package has already had a chance to break, so every refusal here happens **at load,
//! before any package code runs** — the same place `validate_action_contribution` refuses a
//! mutating action with no mutating capability (ADR-0594, ADR-0595).
//!
//! The vocabulary is `ono-change-core`'s own. A consistency class, a restore method, a risk
//! dimension and an equivalence domain each have exactly one definition in this workspace, and a
//! package is settled against that definition rather than against a copy of it (the same reason
//! the temporal contributions are settled against `ono-temporal-core`, v0.5 §37.1).

use ono_change_core::{
    ConsistencyClass, EffectConfidence, EffectDomain, EffectKind, EquivalenceDomain, PlanState,
    RecoveryCapability, RestoreMethod, RiskAssessment, RiskClass, RiskDimension, RiskFinding,
};
use ono_kuang_protocol::{
    ActionContribution, Capability, ChangeViewContribution, EffectClassContribution, Hello,
    ImpactProviderContribution, KuangError, KuangErrorCode, RecoveryProviderContribution,
    Risk as CapabilityRisk, RiskRuleContribution, VerificationProviderContribution,
};

/// What a loaded package contributes to plan intelligence (v0.6 §48.2).
///
/// The five contribution types beside `ActionProvider`, kept together so a host that integrates
/// the package into the change registries reads one table rather than five fields of `Hello` it
/// has to re-validate. Everything in here has already passed [`validate`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChangeContributions {
    /// The recovery providers the package registered (§48.2, §12.1).
    pub recovery_providers: Vec<RecoveryProviderContribution>,
    /// The impact providers it registered (§48.2, §9.4).
    pub impact_providers: Vec<ImpactProviderContribution>,
    /// The verification providers it registered (§48.2, §25.1).
    pub verification_providers: Vec<VerificationProviderContribution>,
    /// The risk rules it registered (§48.2, §19.2).
    pub risk_rules: Vec<RiskRuleContribution>,
    /// The plan views it registered (§48.2, §45).
    pub change_views: Vec<ChangeViewContribution>,
}

impl ChangeContributions {
    /// Everything the package's `Hello` carried, after [`validate`] accepted it.
    #[must_use]
    pub fn of(hello: &Hello) -> Self {
        Self {
            recovery_providers: hello.contributions.recovery_providers.clone(),
            impact_providers: hello.contributions.impact_providers.clone(),
            verification_providers: hello.contributions.verification_providers.clone(),
            risk_rules: hello.contributions.risk_rules.clone(),
            change_views: hello.contributions.change_views.clone(),
        }
    }

    /// Whether the package contributes nothing about change or recovery.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.recovery_providers.is_empty()
            && self.impact_providers.is_empty()
            && self.verification_providers.is_empty()
            && self.risk_rules.is_empty()
            && self.change_views.is_empty()
    }

    /// The risk rule with `rule_id`, or `None` for a rule this package never declared.
    ///
    /// §19.2 makes a risk class the output of a registered, inspectable rule. A finding naming a
    /// rule nobody can look up is a class with no justification behind it, so the host resolves
    /// the rule before it accepts the finding.
    #[must_use]
    pub fn risk_rule(&self, rule_id: &str) -> Option<&RiskRuleContribution> {
        self.risk_rules.iter().find(|rule| rule.rule_id == rule_id)
    }
}

fn invalid(detail: String) -> KuangError {
    KuangError::new(KuangErrorCode::PackageInvalid, detail)
}

/// Settles one word against a closed vocabulary of `ono-change-core`.
fn word<T>(
    resolve: impl Fn(&str) -> Option<T>,
    value: &str,
    what: &str,
    owner: &str,
    vocabulary: &str,
) -> Result<T, KuangError> {
    resolve(value).ok_or_else(|| {
        invalid(format!(
            "`{owner}` names the {what} `{value}`, which is not one of {vocabulary}"
        ))
    })
}

/// Settles a contributed id against the package's namespace and the contribution's own prefix.
fn contributed_id(package_id: &str, kind: &str, id: &str) -> Result<(), KuangError> {
    ono_kuang_protocol::validate_contributed_id(package_id, kind, id)?;
    let expected = format!("{package_id}.{kind}.");
    if id.starts_with(&expected) {
        return Ok(());
    }
    Err(invalid(format!(
        "`{id}` is not `<package.id>.{kind}.<kebab-name>` (spec §31.5)"
    )))
}

/// Validates every v0.6 §48.2 contribution the handshake carried.
///
/// `resolve_schema` settles a schema id against the registry this instance will validate its
/// values against — the same function the target and command contributions are settled with, so
/// an impact provider naming a type nothing carries is refused where a target naming one is.
///
/// # Errors
///
/// `package.invalid`, naming the contribution and the rule it broke. Every one of these is a
/// refusal §48.4 asks for at load rather than at the first call.
pub fn validate(
    package_id: &str,
    hello: &Hello,
    resolve_schema: &mut dyn FnMut(&str, &str, &str) -> Result<(), KuangError>,
) -> Result<(), KuangError> {
    for provider in &hello.contributions.recovery_providers {
        validate_recovery_provider(package_id, provider)?;
    }
    for provider in &hello.contributions.impact_providers {
        validate_impact_provider(package_id, provider, resolve_schema)?;
    }
    for provider in &hello.contributions.verification_providers {
        validate_verification_provider(package_id, provider)?;
    }
    for rule in &hello.contributions.risk_rules {
        validate_risk_rule(package_id, rule)?;
    }
    for view in &hello.contributions.change_views {
        validate_change_view(package_id, view)?;
    }
    for command in &hello.contributions.commands {
        if let Some(action) = &command.action {
            validate_effect_classes(&command.id, action)?;
        }
    }
    Ok(())
}

/// §48.5's shape, held to §12.2's capabilities, §39.2's ownership rule and §27.3's non-goal.
fn validate_recovery_provider(
    package_id: &str,
    provider: &RecoveryProviderContribution,
) -> Result<(), KuangError> {
    contributed_id(package_id, "recovery-provider", &provider.id)?;
    if provider.domain_kinds.is_empty() {
        return Err(invalid(format!(
            "recovery provider `{}` covers no persistence domain kind, so nothing it discovers \
             could ever be mapped to a path (v0.6 §11.2, Appendix B.1)",
            provider.id
        )));
    }
    if provider.asset_type.trim().is_empty() {
        return Err(invalid(format!(
            "recovery provider `{}` names no asset type, and §11.1 makes the asset the thing a \
             person inspects",
            provider.id
        )));
    }
    let consistency = word(
        ConsistencyClass::from_name,
        &provider.consistency,
        "consistency class",
        &provider.id,
        "the six of v0.6 §11.3",
    )?;
    for method in &provider.restore_methods {
        word(
            RestoreMethod::from_name,
            method,
            "restore method",
            &provider.id,
            "Appendix C.1's methods",
        )?;
    }
    // Every declared capability is a real family, and a recovery provider declares recovery
    // capabilities: a provider that listed `filesystem.write` here would be describing something
    // the change registries have no place to put.
    let mut declared = Vec::new();
    for id in &provider.capabilities {
        let capability: Capability = id.parse()?;
        if RecoveryCapability::from_name(id).is_none() {
            return Err(invalid(format!(
                "recovery provider `{}` declares `{id}`, which is not one of the seven recovery \
                 capabilities of v0.6 §12.2",
                provider.id
            )));
        }
        declared.push(capability);
    }
    if declared.is_empty() {
        return Err(invalid(format!(
            "recovery provider `{}` declares no capability, so there is nothing the host could \
             ever ask it to do (v0.6 §12.2)",
            provider.id
        )));
    }
    // §48.4, the load-time refusal this module exists for. Offering a restore method or claiming
    // `recovery.restore` is claiming the operation §13.6 and §14.6 make the one that can lose the
    // most; §43.4 lets it need a stronger privilege than the mutation it undoes. A package that
    // claims it while holding nothing of destructive risk is refused before it runs, because the
    // alternative is a candidate an operator relies on and a restore that is denied at the moment
    // they need it.
    let offers_restore = !provider.restore_methods.is_empty()
        || provider
            .capabilities
            .iter()
            .any(|id| id == RecoveryCapability::Restore.as_str());
    if offers_restore
        && !declared
            .iter()
            .any(|capability| capability.risk() == CapabilityRisk::Destructive)
    {
        return Err(invalid(format!(
            "recovery provider `{}` offers to restore and declares no capability of destructive \
             risk; v0.6 §48.4 refuses that at load, and `recovery.restore` is the capability that \
             carries the authority (§12.2, §43.4)",
            provider.id
        ))
        .with_help(
            "a provider that can put state back declares `recovery.restore`; one that only \
             discovers and prepares declares neither the capability nor a restore method",
        ));
    }
    // §39.2: Ono MUST NOT label state `APPLICATION_CONSISTENT` unless an application-aware
    // provider asserts the guarantee, and §16.4 says the provider must own the claim. Owning it
    // means being able to quiesce — a package that cannot pause the application has no mechanism
    // by which the claim could be true.
    if consistency == ConsistencyClass::ApplicationConsistent
        && !provider
            .capabilities
            .iter()
            .any(|id| id == RecoveryCapability::Quiesce.as_str())
    {
        return Err(invalid(format!(
            "recovery provider `{}` claims `application-consistent` and declares no \
             `recovery.quiesce`; v0.6 §39.2 and §16.4 put the claim with the provider that can \
             actually quiesce the application",
            provider.id
        ))
        .with_help(
            "a snapshot of an application's files without quiesce or checkpoint participation is \
             `crash-consistent`, which is a claim this provider can own (§11.3, §39.2)",
        ));
    }
    if let Some(transaction) = &provider.transaction {
        if !provider
            .capabilities
            .iter()
            .any(|id| id == RecoveryCapability::Transaction.as_str())
        {
            return Err(invalid(format!(
                "recovery provider `{}` states an atomicity guarantee and declares no \
                 `recovery.transaction` (v0.6 §12.2, §27.1)",
                provider.id
            )));
        }
        if transaction.guarantee.trim().is_empty() {
            return Err(invalid(format!(
                "recovery provider `{}` states a transaction and says nothing about what it \
                 guarantees; §27.1 puts the guarantee with the provider, so the sentence is the \
                 declaration",
                provider.id
            )));
        }
        if transaction.resources.is_empty() {
            return Err(invalid(format!(
                "recovery provider `{}` states a transaction over no resource; §27.1 scopes \
                 atomicity to the provider's own resource scope, and an empty scope is not one",
                provider.id
            )));
        }
        // §27.3: generic distributed two-phase commit is an explicit non-goal. A provider may
        // state atomicity over its own resource scope and no further; a declaration reaching a
        // domain kind it does not itself cover is exactly the cross-boundary transaction §27.2
        // forbids the word for.
        for resource in &transaction.resources {
            if !provider.domain_kinds.iter().any(|kind| kind == resource) {
                return Err(invalid(format!(
                    "recovery provider `{}` states atomicity over `{resource}`, which is not one \
                     of the domain kinds it covers; v0.6 §27.1 scopes a provider transaction to \
                     its own resources and §27.3 makes generic two-phase commit a non-goal",
                    provider.id
                ))
                .with_help(
                    "a plan spanning two boundaries is a ChangeSet, not a transaction (§27.2); \
                     declare atomicity only over the domain kinds this provider itself covers",
                ));
            }
        }
    }
    Ok(())
}

/// §9.4's edges, and §49.3's rule that nothing here raises a confidence.
fn validate_impact_provider(
    package_id: &str,
    provider: &ImpactProviderContribution,
    resolve_schema: &mut dyn FnMut(&str, &str, &str) -> Result<(), KuangError>,
) -> Result<(), KuangError> {
    contributed_id(package_id, "impact-provider", &provider.id)?;
    if provider.object_types.is_empty() {
        return Err(invalid(format!(
            "impact provider `{}` relates no object type (v0.6 §9.4)",
            provider.id
        )));
    }
    for schema in &provider.object_types {
        resolve_schema(schema, "impact provider", &provider.id)?;
    }
    if provider.relations.is_empty() {
        return Err(invalid(format!(
            "impact provider `{}` contributes no relation label, so an edge it drew would carry \
             no word a reader could look up (v0.6 §9.4, §15.8)",
            provider.id
        )));
    }
    if let Some(ceiling) = &provider.confidence_ceiling {
        word(
            EffectConfidence::from_name,
            ceiling,
            "confidence",
            &provider.id,
            "the four of v0.6 §8.1",
        )?;
    }
    Ok(())
}

/// §25.1's equivalence domains, one per check kind.
fn validate_verification_provider(
    package_id: &str,
    provider: &VerificationProviderContribution,
) -> Result<(), KuangError> {
    contributed_id(package_id, "verification-provider", &provider.id)?;
    if provider.checks.is_empty() {
        return Err(invalid(format!(
            "verification provider `{}` observes no check kind (v0.6 §23.1)",
            provider.id
        )));
    }
    for check in &provider.checks {
        if check.kind.trim().is_empty() {
            return Err(invalid(format!(
                "verification provider `{}` declares a check with no kind",
                provider.id
            )));
        }
        word(
            EquivalenceDomain::from_name,
            &check.equivalence,
            "equivalence domain",
            &provider.id,
            "the three of v0.6 §25.1",
        )?;
    }
    Ok(())
}

/// §19.1's dimensions and §19.2's classes, settled before a rule is registered.
fn validate_risk_rule(package_id: &str, rule: &RiskRuleContribution) -> Result<(), KuangError> {
    if !rule.rule_id.starts_with(&format!("{package_id}.")) {
        return Err(invalid(format!(
            "risk rule `{}` is not namespaced under `{package_id}`; a contributed rule is a rule, \
             and §19.2 makes it inspectable exactly as a built-in one is (spec §31.5)",
            rule.rule_id
        )));
    }
    word(
        RiskDimension::from_name,
        &rule.dimension,
        "risk dimension",
        &rule.rule_id,
        "the ten of v0.6 §19.1",
    )?;
    word(
        RiskClass::from_name,
        &rule.emits,
        "risk class",
        &rule.rule_id,
        "the five of v0.6 §19.2",
    )?;
    if rule.summary.trim().is_empty() {
        return Err(invalid(format!(
            "risk rule `{}` says nothing about what it finds; §40.2 shows that sentence instead \
             of \"Are you sure?\", so a rule without one teaches the flag rather than the risk",
            rule.rule_id
        )));
    }
    Ok(())
}

/// §48.2's `ChangeView`, in the view lifecycle every other contributed view uses.
fn validate_change_view(package_id: &str, view: &ChangeViewContribution) -> Result<(), KuangError> {
    contributed_id(package_id, "change-view", &view.id)?;
    if !matches!(view.mode.as_str(), "interactive" | "static") {
        return Err(invalid(format!(
            "change view `{}` declares the mode `{}` (spec §31.27)",
            view.id, view.mode
        )));
    }
    for state in &view.plan_states {
        word(
            PlanState::from_name,
            state,
            "plan state",
            &view.id,
            "the nineteen of v0.6 §4.1",
        )?;
    }
    if view.fallback.trim().is_empty() {
        return Err(invalid(format!(
            "change view `{}` declares no non-interactive fallback (spec §31.28, §50)",
            view.id
        )));
    }
    Ok(())
}

/// Appendix A.1's domains, §8.2's kinds and §8.1's confidences, on a contributed action.
fn validate_effect_classes(
    command_id: &str,
    action: &ActionContribution,
) -> Result<(), KuangError> {
    for effect in &action.effect_classes {
        validate_effect_class(command_id, effect)?;
    }
    Ok(())
}

/// One effect class, settled against the vocabulary Appendix A.5's coverage algorithm reads.
///
/// # Errors
///
/// `package.invalid` naming the owner and the word that is not in the vocabulary.
pub fn validate_effect_class(
    owner: &str,
    effect: &EffectClassContribution,
) -> Result<(), KuangError> {
    word(
        EffectDomain::from_name,
        &effect.domain,
        "effect domain",
        owner,
        "Appendix A.1's mutation domains",
    )?;
    word(
        EffectKind::from_name,
        &effect.kind,
        "effect kind",
        owner,
        "the seven of v0.6 §8.2",
    )?;
    word(
        EffectConfidence::from_name,
        &effect.confidence,
        "confidence",
        owner,
        "the four of v0.6 §8.1",
    )?;
    if effect.explanation.trim().is_empty() {
        return Err(invalid(format!(
            "`{owner}` declares an effect it does not explain; §9.5 makes an impact claim that \
             cannot be explained one that should not be shown"
        )));
    }
    Ok(())
}

/// Folds contributed risk findings into a class that already holds (v0.6 §19.2).
///
/// The only operation §19.2 defines over risk classes is a maximum, and this is it, applied
/// through `RiskAssessment` rather than beside it. A contributed rule can therefore raise a
/// plan's class and can never reduce it: there is no minimum to reach for, and a `low` finding
/// arriving after a `high` one composes to `high` exactly as a built-in `low` finding would.
#[must_use]
pub fn compose_risk(held: RiskClass, contributed: &[RiskFinding]) -> RiskClass {
    let mut findings = vec![RiskFinding::new(
        RiskDimension::UnknownImpact,
        held,
        "ono.change.risk.composed",
        "the class the plan already carried before this contribution",
    )];
    findings.extend(contributed.iter().cloned());
    RiskAssessment::of(findings).classify()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_keep_the_stronger_class_when_a_contributed_rule_finds_a_weaker_one() {
        // §19.2: the class is the strongest any rule found. A plugin's `low` beside a built-in
        // `high` composes to `high`, because `classify` is a maximum and §19.2 defines no
        // minimum for anything to reach for.
        let contributed = [RiskFinding::new(
            RiskDimension::Scope,
            RiskClass::Low,
            "dev.example.postgres.risk.bounded",
            "one database, one schema.",
        )];
        assert_eq!(
            compose_risk(RiskClass::High, &contributed),
            RiskClass::High,
            "§19.2: a contributed rule may raise a plan's risk class and never reduce it"
        );
    }

    #[test]
    fn should_raise_the_class_when_a_contributed_rule_finds_a_stronger_one() {
        let contributed = [RiskFinding::new(
            RiskDimension::Downtime,
            RiskClass::Critical,
            "dev.example.postgres.risk.primary-failover",
            "the primary would be demoted for the length of the restore.",
        )];
        assert_eq!(
            compose_risk(RiskClass::Moderate, &contributed),
            RiskClass::Critical,
            "§19.2: raising is the direction a contributed rule may move a class in"
        );
    }
}
