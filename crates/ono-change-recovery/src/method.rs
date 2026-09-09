//! Choosing how to restore (spec Appendix C.1, C.2, C.5, C.7, §56.3).
//!
//! `ono_change_core::choose_method` already holds Appendix C.1's order, and this module holds the
//! two things around it that the order alone cannot decide.
//!
//! The first is where the candidates come from. [`offers`] asks each provider that holds one of
//! the assets for its `RecoveryPlanFragment` — the planning half of §12.1, and the only provider
//! method this crate ever calls, because §24.1 makes `recover` produce a plan and change nothing.
//! A provider that cannot run here leaves a `ProviderRefusal` behind rather than a silent gap
//! (§12.2, §55.6 case 29).
//!
//! The second is Appendix C.1's escape clause: *"A lower item MAY be chosen if upper items cannot
//! preserve required metadata or application semantics."* [`select`] walks the order and skips a
//! method that cannot put back what the goal requires, recording why it was skipped — because
//! Appendix C.7 makes missing metadata support a thing that MUST be visible, and a method silently
//! passed over is a decision nobody can inspect (§20.1).
//!
//! Appendix C.5 is enforced by omission and by refusal. `RestoreMethod` has no merge member, so no
//! method this module can choose is one; and a request naming a method outside the closed list is
//! refused with the sentence that says automatic semantic merging is a KUANG/11 provider's to
//! offer as a distinct plan action with its own verification.

use std::sync::Arc;

use ono_change_core::{
    ChangePlan, MetadataCoverage, RecoveryAsset, RecoveryAssetId, RecoveryGoal,
    RecoveryPlanFragment, RestoreMethod, choose_method, error,
};
use ono_change_protection::{ProviderRefusal, ProviderRegistry};
use ono_value::ErrorValue;

/// One provider's answer about one asset (§12.1, Appendix C.1).
#[derive(Debug, Clone, PartialEq)]
pub struct MethodOffer {
    provider: Arc<str>,
    asset: RecoveryAssetId,
    reference: Arc<str>,
    fragment: RecoveryPlanFragment,
    preserves: Vec<Arc<str>>,
}

impl MethodOffer {
    /// Records that `provider` would recover from `asset` the way `fragment` describes.
    #[must_use]
    pub fn new(
        provider: impl Into<Arc<str>>,
        asset: RecoveryAssetId,
        reference: impl Into<Arc<str>>,
        fragment: RecoveryPlanFragment,
    ) -> Self {
        Self {
            provider: provider.into(),
            asset,
            reference: reference.into(),
            fragment,
            preserves: Vec::new(),
        }
    }

    /// Names an application semantic this method keeps intact (Appendix C.1).
    ///
    /// Appendix C.1 lets a lower item win where an upper one "cannot preserve required metadata or
    /// application semantics". Metadata is measurable and travels on the fragment; a semantic is
    /// the provider's own claim about its domain — that a package downgrade keeps the service's
    /// unit file, that a clone-and-copy keeps an open database consistent — so it is declared
    /// rather than derived.
    #[must_use]
    pub fn preserving(mut self, semantic: impl Into<Arc<str>>) -> Self {
        self.preserves.push(semantic.into());
        self
    }

    /// The provider that made the offer.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// The asset it would restore from.
    #[must_use]
    pub const fn asset(&self) -> &RecoveryAssetId {
        &self.asset
    }

    /// The provider-native reference of that asset — `rpool/ROOT/debian@ono-a82f`.
    #[must_use]
    pub fn reference(&self) -> &str {
        &self.reference
    }

    /// The method it would use.
    #[must_use]
    pub fn method(&self) -> RestoreMethod {
        self.fragment.method()
    }

    /// The provider's fragment of the recovery plan.
    #[must_use]
    pub const fn fragment(&self) -> &RecoveryPlanFragment {
        &self.fragment
    }

    /// Which file metadata the restore actually puts back (Appendix C.7).
    #[must_use]
    pub fn metadata(&self) -> MetadataCoverage {
        self.fragment.metadata()
    }

    /// The application semantics the provider claims this method keeps.
    #[must_use]
    pub fn preserved_semantics(&self) -> &[Arc<str>] {
        &self.preserves
    }
}

/// What the providers offered, and who could not be asked (§12.2, Appendix A.3).
#[derive(Debug, Clone, Default)]
pub struct MethodOffers {
    offers: Vec<MethodOffer>,
    refusals: Vec<ProviderRefusal>,
}

impl MethodOffers {
    /// An empty answer, before any provider was asked.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            offers: Vec::new(),
            refusals: Vec::new(),
        }
    }

    /// Adds an offer, which a caller does directly when it holds the fragment already.
    #[must_use]
    pub fn with(mut self, offer: MethodOffer) -> Self {
        self.offers.push(offer);
        self
    }

    /// Records a provider that could not be asked (§12.2, Appendix G.4).
    #[must_use]
    pub fn refusing(mut self, refusal: ProviderRefusal) -> Self {
        self.refusals.push(refusal);
        self
    }

    /// What was offered.
    #[must_use]
    pub fn offers(&self) -> &[MethodOffer] {
        &self.offers
    }

    /// The providers that could not answer, and why.
    #[must_use]
    pub fn refusals(&self) -> &[ProviderRefusal] {
        &self.refusals
    }

    /// Whether every provider that holds an asset actually answered (§56.3).
    #[must_use]
    pub fn is_conclusive(&self) -> bool {
        self.refusals.is_empty()
    }
}

/// Asks each provider that holds one of `assets` how it would restore (§12.1, §24.1).
///
/// This calls `plan_recovery` and nothing else. `restore`, `create` and `cleanup` are the
/// executor's, and §55.8 case 35 requires `recover @plan` to change nothing at all.
#[must_use]
pub fn offers(
    registry: &ProviderRegistry,
    assets: &[RecoveryAsset],
    source: Option<&ChangePlan>,
    goal: RecoveryGoal,
) -> MethodOffers {
    let mut answer = MethodOffers::empty();
    for asset in assets {
        let Some(provider) = registry.get(asset.provider()) else {
            let reason = format!(
                "no provider with this id is registered, so the asset {} cannot be restored from",
                asset.reference()
            );
            answer = answer.refusing(ProviderRefusal::new(
                asset.provider(),
                reason.clone(),
                error::provider_unavailable(asset.provider(), &reason),
            ));
            continue;
        };
        let availability = provider.availability();
        if !availability.is_available() {
            let reason = availability
                .reason()
                .unwrap_or("the provider did not say why")
                .to_owned();
            answer = answer.refusing(ProviderRefusal::new(
                provider.id(),
                reason.clone(),
                error::provider_unavailable(provider.id(), &reason),
            ));
            continue;
        }
        match provider.plan_recovery(asset, source, goal) {
            Ok(fragment) => {
                answer = answer.with(MethodOffer::new(
                    provider.id(),
                    asset.id().clone(),
                    asset.reference(),
                    fragment,
                ));
            }
            Err(error) => {
                let reason = error.message().to_owned();
                answer = answer.refusing(ProviderRefusal::new(provider.id(), reason, error));
            }
        }
    }
    answer
}

/// Why an offered method was not chosen (Appendix C.1, C.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodRejection {
    /// Appendix C.2: it cannot achieve what this recovery is for.
    GoalUnsatisfied,
    /// Appendix C.7: it cannot put back metadata the goal requires.
    MetadataShortfall,
    /// Appendix C.1: it cannot preserve an application semantic the goal requires.
    SemanticShortfall,
    /// Appendix C.1: something less destructive can do the same job.
    Dominated,
}

impl MethodRejection {
    /// The word a rendering shows for the reason.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            MethodRejection::GoalUnsatisfied => "goal-unsatisfied",
            MethodRejection::MetadataShortfall => "metadata-shortfall",
            MethodRejection::SemanticShortfall => "semantic-shortfall",
            MethodRejection::Dominated => "dominated",
        }
    }
}

/// A method that was offered and not chosen, and what it lost to (Appendix C.1).
///
/// The losers are kept because §20.1 asks a plan to answer "why this and not that", and a method
/// silently passed over for missing SELinux support is exactly the invisible metadata gap
/// Appendix C.7 forbids.
#[derive(Debug, Clone, PartialEq)]
pub struct RejectedMethod {
    provider: Arc<str>,
    method: RestoreMethod,
    reason: MethodRejection,
    unmet: Vec<Arc<str>>,
    detail: Arc<str>,
}

impl RejectedMethod {
    /// The provider that offered it.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// The method.
    #[must_use]
    pub const fn method(&self) -> RestoreMethod {
        self.method
    }

    /// Why it lost.
    #[must_use]
    pub const fn reason(&self) -> MethodRejection {
        self.reason
    }

    /// What it could not do — the metadata or the semantics the goal asked for.
    #[must_use]
    pub fn unmet(&self) -> &[Arc<str>] {
        &self.unmet
    }

    /// The sentence that says so.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// Everything [`select`] needs (Appendix C.1, C.2, C.7).
#[derive(Debug, Clone)]
pub struct MethodRequest<'a> {
    goal: RecoveryGoal,
    offers: &'a MethodOffers,
    required_metadata: MetadataCoverage,
    required_semantics: Vec<Arc<str>>,
    forced: Option<Arc<str>>,
}

impl<'a> MethodRequest<'a> {
    /// A selection for `goal` over `offers`.
    #[must_use]
    pub fn new(goal: RecoveryGoal, offers: &'a MethodOffers) -> Self {
        Self {
            goal,
            offers,
            required_metadata: MetadataCoverage::content_only(),
            required_semantics: Vec::new(),
            forced: None,
        }
    }

    /// States which file metadata the recovery must put back (Appendix C.7).
    #[must_use]
    pub const fn requiring_metadata(mut self, required: MetadataCoverage) -> Self {
        self.required_metadata = required;
        self
    }

    /// Names an application semantic the recovery must preserve (Appendix C.1).
    #[must_use]
    pub fn requiring_semantic(mut self, semantic: impl Into<Arc<str>>) -> Self {
        self.required_semantics.push(semantic.into());
        self
    }

    /// Names the method the operator asked for by name, overriding Appendix C.1's order.
    #[must_use]
    pub fn forcing(mut self, method: impl Into<Arc<str>>) -> Self {
        self.forced = Some(method.into());
        self
    }

    /// The goal.
    #[must_use]
    pub const fn goal(&self) -> RecoveryGoal {
        self.goal
    }

    /// The offers.
    #[must_use]
    pub const fn offers(&self) -> &MethodOffers {
        self.offers
    }

    /// The metadata the recovery must put back.
    #[must_use]
    pub const fn required_metadata(&self) -> MetadataCoverage {
        self.required_metadata
    }

    /// The semantics the recovery must preserve.
    #[must_use]
    pub fn required_semantics(&self) -> &[Arc<str>] {
        &self.required_semantics
    }
}

/// The method that won, the ones that did not, and what the winner still does not restore.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodSelection {
    chosen: MethodOffer,
    rejected: Vec<RejectedMethod>,
}

impl MethodSelection {
    /// The method the recovery will use.
    #[must_use]
    pub const fn chosen(&self) -> &MethodOffer {
        &self.chosen
    }

    /// Every offer that was not chosen, with the reason (Appendix C.1).
    #[must_use]
    pub fn rejected(&self) -> &[RejectedMethod] {
        &self.rejected
    }

    /// The metadata the chosen method does not put back (Appendix C.7).
    ///
    /// Non-empty is the ordinary case: a selective file restore that returns content, mode and
    /// owner still returns no SELinux label, and Appendix C.7 requires the operator to see that
    /// before the recovery runs rather than after it.
    #[must_use]
    pub fn metadata_gaps(&self) -> Vec<&'static str> {
        self.chosen.metadata().gaps()
    }
}

/// Chooses the least-destructive method that can actually meet the goal (Appendix C.1, §56.3).
///
/// # Errors
///
/// - `change.action_not_plannable` when the request names a method outside the closed list of
///   Appendix C.1 — automatic configuration merging among them (Appendix C.5).
/// - `recovery.plan_incomplete` when nothing on offer can meet the goal, the metadata or the
///   semantics it requires. §56.3 makes that a block rather than a fall back to the biggest
///   available hammer.
pub fn select(request: &MethodRequest<'_>) -> Result<MethodSelection, ErrorValue> {
    let forced = match request.forced.as_deref() {
        None => None,
        Some(name) => Some(RestoreMethod::from_name(name).ok_or_else(|| unknown_method(name))?),
    };
    let mut acceptable: Vec<&MethodOffer> = Vec::new();
    let mut rejected: Vec<RejectedMethod> = Vec::new();
    for offer in request.offers().offers() {
        if forced.is_some_and(|method| offer.method() != method) {
            rejected.push(dominated(offer, "the operator named a different method"));
            continue;
        }
        if choose_method(request.goal(), &[offer.method()]).is_none() {
            rejected.push(RejectedMethod {
                provider: Arc::from(offer.provider()),
                method: offer.method(),
                reason: MethodRejection::GoalUnsatisfied,
                unmet: vec![Arc::from(request.goal().as_str())],
                detail: Arc::from(format!(
                    "Appendix C.2: {} cannot achieve {}",
                    offer.method(),
                    request.goal()
                )),
            });
            continue;
        }
        let metadata = unmet_metadata(request.required_metadata(), offer.metadata());
        if !metadata.is_empty() {
            rejected.push(RejectedMethod {
                provider: Arc::from(offer.provider()),
                method: offer.method(),
                reason: MethodRejection::MetadataShortfall,
                unmet: metadata.iter().map(|name| Arc::from(*name)).collect(),
                detail: Arc::from(format!(
                    "Appendix C.7: {} does not restore {}",
                    offer.method(),
                    metadata.join(", ")
                )),
            });
            continue;
        }
        let semantics = unmet_semantics(request.required_semantics(), offer.preserved_semantics());
        if !semantics.is_empty() {
            rejected.push(RejectedMethod {
                provider: Arc::from(offer.provider()),
                method: offer.method(),
                reason: MethodRejection::SemanticShortfall,
                unmet: semantics.clone(),
                detail: Arc::from(format!(
                    "Appendix C.1: {} cannot preserve {}",
                    offer.method(),
                    join(&semantics)
                )),
            });
            continue;
        }
        acceptable.push(offer);
    }
    let methods: Vec<RestoreMethod> = acceptable.iter().map(|offer| offer.method()).collect();
    let winner = choose_method(request.goal(), &methods)
        .ok_or_else(|| nothing_meets_the_goal(request, &rejected))?;
    let mut chosen: Option<MethodOffer> = None;
    for offer in acceptable {
        if chosen.is_none() && offer.method() == winner {
            chosen = Some(offer.clone());
        } else {
            rejected.push(dominated(
                offer,
                &format!("{winner} reaches the same goal and risks less unrelated state"),
            ));
        }
    }
    chosen
        .map(|chosen| MethodSelection { chosen, rejected })
        .ok_or_else(|| nothing_meets_the_goal(request, &[]))
}

/// The metadata pieces `required` asks for and `offered` does not restore (Appendix C.7).
#[must_use]
pub fn unmet_metadata(required: MetadataCoverage, offered: MetadataCoverage) -> Vec<&'static str> {
    [
        (required.content, offered.content, "content"),
        (required.mode, offered.mode, "mode"),
        (required.owner, offered.owner, "owner/group"),
        (required.acl, offered.acl, "ACLs"),
        (required.xattrs, offered.xattrs, "extended attributes"),
        (
            required.capabilities,
            offered.capabilities,
            "file capabilities",
        ),
        (required.selinux, offered.selinux, "SELinux labels"),
        (
            required.hardlinks,
            offered.hardlinks,
            "hard-link relationships",
        ),
    ]
    .into_iter()
    .filter_map(|(wanted, present, name)| (wanted && !present).then_some(name))
    .collect()
}

fn unmet_semantics(required: &[Arc<str>], preserved: &[Arc<str>]) -> Vec<Arc<str>> {
    required
        .iter()
        .filter(|wanted| !preserved.iter().any(|kept| kept == *wanted))
        .cloned()
        .collect()
}

fn dominated(offer: &MethodOffer, detail: &str) -> RejectedMethod {
    RejectedMethod {
        provider: Arc::from(offer.provider()),
        method: offer.method(),
        reason: MethodRejection::Dominated,
        unmet: Vec::new(),
        detail: Arc::from(detail),
    }
}

fn join(names: &[Arc<str>]) -> String {
    names
        .iter()
        .map(|name| name.to_string())
        .collect::<Vec<String>>()
        .join(", ")
}

/// The refusal for a method outside Appendix C.1's closed list (Appendix C.5).
fn unknown_method(name: &str) -> ErrorValue {
    let known = RestoreMethod::ALL
        .iter()
        .map(|method| method.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    error::action_not_plannable(
        &format!("restore method `{name}`"),
        &format!(
            "v0.6 Appendix C.1 closes the list of methods the core recovery engine chooses from: \
             {known}. Appendix C.5 makes automatic semantic merging of configuration files an \
             explicit non-goal here; a KUANG/11 provider may offer merge support, and must \
             present it as a distinct RecoveryPlan action with its own verification"
        ),
    )
}

/// The refusal when Appendix C.1's whole order has been walked without an acceptable method.
fn nothing_meets_the_goal(request: &MethodRequest<'_>, rejected: &[RejectedMethod]) -> ErrorValue {
    let mut detail = format!(
        "{} method(s) were offered for {} and none of them can meet it",
        request.offers().offers().len(),
        request.goal()
    );
    for rejection in rejected {
        detail.push_str(&format!("; {}", rejection.detail()));
    }
    for refusal in request.offers().refusals() {
        detail.push_str(&format!(
            "; {} could not be asked: {}",
            refusal.provider(),
            refusal.reason()
        ));
    }
    error::recovery_plan_incomplete("a restore method that meets the recovery goal", &detail)
}
