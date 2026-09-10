//! What the coverage answer is allowed to mean (spec §17, §38.3, Appendix H).
//!
//! §17.2's four modes are four different questions asked of the same analysis. `off` creates
//! nothing and still shows what was available. `prefer` takes the bounded-cost protection a
//! provider offers. `require` refuses to apply when a required domain cannot reach the plan's
//! protection class. `maximize` adds every non-conflicting mechanism that improves coverage
//! *within the configured cost limits* — §17.2 and §62.7 both say in as many words that it does
//! not mean "snapshot everything on the host", and the limits in [`CostLimits`] are §38.3's list
//! of the ways a configuration says how far it may go.
//!
//! Appendix H's profiles are presets, and H.5 is the rule that makes them safe to have:
//!
//! > A plan may impose stricter requirements than a profile. A profile MUST NOT weaken
//! > provider-declared safety constraints.
//!
//! [`ProtectionPolicy::tighten_with`] is that sentence as an operation. Applying a profile can
//! raise the mode, lengthen retention, raise the free-space floor and narrow a cost cap; there is
//! no path through it that lowers any of them, so a `cautious` operator who runs with a `fleet`
//! profile keeps `require`.

use std::sync::Arc;
use std::time::Duration;

use ono_change_core::error;
use ono_change_core::{
    EffectDomain, PlanId, ProtectionLevel, ProtectionMode, RecoveryCost, RetentionPolicy, RiskClass,
};
use ono_value::{ByteSize, ErrorValue, Percent};

use crate::coverage::CoverageAnalysis;

/// How much room a filesystem must keep free for automatic protection to run (§53, Appendix D.3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FreeSpaceFloor {
    /// A share of the filesystem, as §53's `min_filesystem_free = "10%"` writes it.
    Share(Percent),
    /// An absolute quantity, for a filesystem whose size makes a share meaningless.
    Absolute(ByteSize),
    /// A share and a quantity together, both in force: what [`FreeSpaceFloor::stricter_of`]
    /// answers for one of each when no filesystem size is known (Appendix H.5).
    Both {
        /// The share of the filesystem that must stay free.
        share: Percent,
        /// The quantity that must stay free.
        absolute: ByteSize,
    },
}

impl FreeSpaceFloor {
    /// §53's default floor: ten percent.
    #[must_use]
    pub fn default_floor() -> Self {
        FreeSpaceFloor::Share(Percent::new(10.0))
    }

    /// The stricter of two floors, for filesystems of any size (Appendix H.5).
    ///
    /// Two shares and two quantities compare directly. A share and a quantity do not: ten percent
    /// is stricter than 50 GiB on 2 TiB and looser on 100 GiB, so without a size neither may be
    /// dropped — H.5 forbids a profile from weakening a floor, and dropping either would weaken
    /// it on some filesystem. The answer is [`FreeSpaceFloor::Both`], which
    /// [`FreeSpaceFloor::is_cleared_by`] clears only where both parts clear, and which is
    /// therefore the stricter one at every size. [`FreeSpaceFloor::stricter_at`] picks one of
    /// the two where the size is known.
    #[must_use]
    pub fn stricter_of(self, other: Self) -> Self {
        let (left_share, left_absolute) = self.parts();
        let (right_share, right_absolute) = other.parts();
        let share = match (left_share, right_share) {
            (Some(left), Some(right)) => Some(if left.value() >= right.value() {
                left
            } else {
                right
            }),
            (only, None) | (None, only) => only,
        };
        let absolute = match (left_absolute, right_absolute) {
            (Some(left), Some(right)) => Some(if left.bytes() >= right.bytes() {
                left
            } else {
                right
            }),
            (only, None) | (None, only) => only,
        };
        match (share, absolute) {
            (Some(share), Some(absolute)) => FreeSpaceFloor::Both { share, absolute },
            (Some(share), None) => FreeSpaceFloor::Share(share),
            (None, Some(absolute)) => FreeSpaceFloor::Absolute(absolute),
            (None, None) => self,
        }
    }

    /// The stricter of two floors on a filesystem of `capacity` (Appendix H.5).
    ///
    /// Both are evaluated against the size, and the one that demands more free room wins; a tie
    /// keeps the share, the form §53 states the default in.
    #[must_use]
    pub fn stricter_at(self, other: Self, capacity: ByteSize) -> Self {
        match self.stricter_of(other) {
            FreeSpaceFloor::Both { share, absolute } => {
                if FreeSpaceFloor::Share(share).required_at(capacity).bytes() >= absolute.bytes() {
                    FreeSpaceFloor::Share(share)
                } else {
                    FreeSpaceFloor::Absolute(absolute)
                }
            }
            single => single,
        }
    }

    /// How much room the floor demands on a filesystem of `capacity`.
    #[must_use]
    pub fn required_at(self, capacity: ByteSize) -> ByteSize {
        match self {
            FreeSpaceFloor::Absolute(minimum) => minimum,
            FreeSpaceFloor::Share(share) => {
                #[allow(
                    clippy::cast_precision_loss,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a share of a filesystem is a proportion, rounded up to a whole byte; \
                              no filesystem's size makes the sixteenth digit of it matter"
                )]
                let bytes = (capacity.bytes() as f64 * share.as_fraction()).ceil() as u128;
                ByteSize::from_bytes(bytes)
            }
            FreeSpaceFloor::Both { share, absolute } => {
                let by_share = FreeSpaceFloor::Share(share).required_at(capacity);
                if by_share.bytes() >= absolute.bytes() {
                    by_share
                } else {
                    absolute
                }
            }
        }
    }

    /// The share and the quantity this floor holds, either of which may be absent.
    const fn parts(self) -> (Option<Percent>, Option<ByteSize>) {
        match self {
            FreeSpaceFloor::Share(share) => (Some(share), None),
            FreeSpaceFloor::Absolute(absolute) => (None, Some(absolute)),
            FreeSpaceFloor::Both { share, absolute } => (Some(share), Some(absolute)),
        }
    }

    /// Whether `free` out of `capacity` clears the floor (Appendix D.3).
    #[must_use]
    pub fn is_cleared_by(self, free: ByteSize, capacity: ByteSize) -> bool {
        match self {
            FreeSpaceFloor::Absolute(minimum) => free.bytes() >= minimum.bytes(),
            FreeSpaceFloor::Both { share, absolute } => {
                FreeSpaceFloor::Share(share).is_cleared_by(free, capacity)
                    && FreeSpaceFloor::Absolute(absolute).is_cleared_by(free, capacity)
            }
            FreeSpaceFloor::Share(share) => {
                if capacity.bytes() == 0 {
                    return false;
                }
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a free-space ratio is a proportion, and no filesystem's size makes \
                              the sixteenth digit of it matter"
                )]
                let ratio = free.bytes() as f64 / capacity.bytes() as f64;
                ratio >= share.as_fraction()
            }
        }
    }

    /// The text a refusal shows for this floor (Appendix D.3).
    #[must_use]
    pub fn describe(self) -> String {
        match self {
            FreeSpaceFloor::Share(share) => format!("{share}"),
            FreeSpaceFloor::Absolute(size) => format!("{size}"),
            FreeSpaceFloor::Both { share, absolute } => {
                format!("{share} and {absolute}, whichever leaves more room")
            }
        }
    }
}

impl Default for FreeSpaceFloor {
    fn default() -> Self {
        Self::default_floor()
    }
}

/// A §38.3 limit a candidate exceeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitBreach {
    /// The asset would be larger than the configured estimate allows.
    EstimatedSize {
        /// What it would take.
        estimated: ByteSize,
        /// What the configuration permits.
        limit: ByteSize,
    },
    /// The candidate's recovery scope reaches further than the configuration allows.
    TargetScope {
        /// How many objects the scope covers.
        objects: usize,
        /// How many the configuration permits.
        limit: usize,
    },
    /// The application would be quiesced for longer than the configuration allows (§18.4).
    QuiesceDuration {
        /// How long the provider needs.
        needed: Duration,
        /// How long the configuration permits.
        limit: Duration,
    },
    /// The plan already proposes as many automatic snapshots as the configuration allows.
    SnapshotCount {
        /// How many the configuration permits.
        limit: usize,
    },
}

impl LimitBreach {
    /// The sentence shown beside the candidate that lost (§38.3).
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            LimitBreach::EstimatedSize { estimated, limit } => format!(
                "the estimated size {estimated} is above the configured limit of {limit} (§38.3)"
            ),
            LimitBreach::TargetScope { objects, limit } => format!(
                "the candidate's recovery scope covers {objects} objects and the configured limit \
                 is {limit} (§38.3)"
            ),
            LimitBreach::QuiesceDuration { needed, limit } => format!(
                "the quiesce window of {needed:?} is longer than the configured bound of \
                 {limit:?} (§18.4, §38.3)"
            ),
            LimitBreach::SnapshotCount { limit } => format!(
                "the plan already proposes the configured maximum of {limit} automatic snapshots \
                 (§38.3, §53's `recovery.max_auto_snapshot_count`)"
            ),
        }
    }
}

/// The bounds configuration puts on automatic protection (§38.3, §53).
#[derive(Debug, Clone, PartialEq)]
pub struct CostLimits {
    max_estimated_size: Option<ByteSize>,
    max_scope_objects: Option<usize>,
    max_quiesce: Option<Duration>,
    max_snapshot_count: Option<usize>,
    min_filesystem_free: FreeSpaceFloor,
}

impl Default for CostLimits {
    /// §53's defaults: thirty-two automatic snapshots and a ten percent free-space floor.
    fn default() -> Self {
        Self {
            max_estimated_size: None,
            max_scope_objects: None,
            max_quiesce: None,
            max_snapshot_count: Some(32),
            min_filesystem_free: FreeSpaceFloor::default_floor(),
        }
    }
}

impl CostLimits {
    /// §38.3's five bounds, in the order the specification lists them.
    pub const KEYS: &'static [&'static str] = &[
        "estimated-size",
        "target-scope",
        "quiesce-duration",
        "snapshot-count",
        "filesystem-free-space-floor",
    ];

    /// Whether one of [`CostLimits::KEYS`] is bound here, or `None` for a name that is not one.
    ///
    /// The free-space floor is always bound — Appendix D.3 makes falling below it a refusal rather
    /// than a preference, so [`FreeSpaceFloor`] has no absent form, only a zero one.
    #[must_use]
    pub fn bounds(&self, key: &str) -> Option<bool> {
        match key {
            "estimated-size" => Some(self.max_estimated_size.is_some()),
            "target-scope" => Some(self.max_scope_objects.is_some()),
            "quiesce-duration" => Some(self.max_quiesce.is_some()),
            "snapshot-count" => Some(self.max_snapshot_count.is_some()),
            "filesystem-free-space-floor" => Some(true),
            _ => None,
        }
    }

    /// Limits that bound nothing, for a caller that states every bound itself.
    #[must_use]
    pub const fn unbounded() -> Self {
        Self {
            max_estimated_size: None,
            max_scope_objects: None,
            max_quiesce: None,
            max_snapshot_count: None,
            min_filesystem_free: FreeSpaceFloor::Share(Percent::ZERO),
        }
    }

    /// Bounds the estimated size of one automatic asset (§38.3).
    #[must_use]
    pub const fn sized(mut self, limit: ByteSize) -> Self {
        self.max_estimated_size = Some(limit);
        self
    }

    /// Bounds how many objects one candidate's recovery scope may cover (§38.3).
    #[must_use]
    pub const fn scoped(mut self, objects: usize) -> Self {
        self.max_scope_objects = Some(objects);
        self
    }

    /// Bounds the quiesce window (§18.4, §38.3).
    #[must_use]
    pub const fn quiesced(mut self, limit: Duration) -> Self {
        self.max_quiesce = Some(limit);
        self
    }

    /// Bounds how many automatic snapshots one plan may propose (§38.3, §53).
    #[must_use]
    pub const fn counted(mut self, limit: usize) -> Self {
        self.max_snapshot_count = Some(limit);
        self
    }

    /// Sets the free-space floor (§53, Appendix D.3).
    #[must_use]
    pub const fn above_floor(mut self, floor: FreeSpaceFloor) -> Self {
        self.min_filesystem_free = floor;
        self
    }

    /// The size bound.
    #[must_use]
    pub const fn max_estimated_size(&self) -> Option<ByteSize> {
        self.max_estimated_size
    }

    /// The scope bound.
    #[must_use]
    pub const fn max_scope_objects(&self) -> Option<usize> {
        self.max_scope_objects
    }

    /// The quiesce bound.
    #[must_use]
    pub const fn max_quiesce(&self) -> Option<Duration> {
        self.max_quiesce
    }

    /// The snapshot-count bound.
    #[must_use]
    pub const fn max_snapshot_count(&self) -> Option<usize> {
        self.max_snapshot_count
    }

    /// The free-space floor.
    #[must_use]
    pub const fn min_filesystem_free(&self) -> FreeSpaceFloor {
        self.min_filesystem_free
    }

    /// Every §38.3 bound a candidate of this cost and scope would exceed.
    ///
    /// `planned` is how many automatic assets the plan already proposes, because the snapshot
    /// count is a property of the plan rather than of one candidate.
    #[must_use]
    pub fn breaches(
        &self,
        cost: &RecoveryCost,
        scope_objects: usize,
        planned: usize,
    ) -> Vec<LimitBreach> {
        let mut breaches = Vec::new();
        if let (Some(limit), Some(estimated)) = (self.max_estimated_size, cost.initial_bytes())
            && estimated.bytes() > limit.bytes()
        {
            breaches.push(LimitBreach::EstimatedSize { estimated, limit });
        }
        if let Some(limit) = self.max_scope_objects
            && scope_objects > limit
        {
            breaches.push(LimitBreach::TargetScope {
                objects: scope_objects,
                limit,
            });
        }
        if let (Some(limit), Some(needed)) = (self.max_quiesce, cost.quiesce())
            && needed > limit
        {
            breaches.push(LimitBreach::QuiesceDuration { needed, limit });
        }
        if let Some(limit) = self.max_snapshot_count
            && planned >= limit
        {
            breaches.push(LimitBreach::SnapshotCount { limit });
        }
        breaches
    }

    /// The stricter of two sets of limits, bound by bound (Appendix H.5).
    ///
    /// A bound that is absent permits everything, so a stated bound always wins over an absent
    /// one and the smaller of two stated bounds wins over the larger. A profile can therefore
    /// narrow what automatic protection may do and can never widen it.
    #[must_use]
    pub fn tightened_with(&self, other: &Self) -> Self {
        Self {
            max_estimated_size: stricter(
                self.max_estimated_size,
                other.max_estimated_size,
                |left, right| left.bytes() <= right.bytes(),
            ),
            max_scope_objects: stricter(
                self.max_scope_objects,
                other.max_scope_objects,
                |left, right| left <= right,
            ),
            max_quiesce: stricter(self.max_quiesce, other.max_quiesce, |left, right| {
                left <= right
            }),
            max_snapshot_count: stricter(
                self.max_snapshot_count,
                other.max_snapshot_count,
                |left, right| left <= right,
            ),
            min_filesystem_free: self
                .min_filesystem_free
                .stricter_of(other.min_filesystem_free),
        }
    }
}

/// The smaller of two optional bounds, where an absent bound permits everything.
fn stricter<T: Copy>(
    left: Option<T>,
    right: Option<T>,
    smaller: impl Fn(T, T) -> bool,
) -> Option<T> {
    match (left, right) {
        (Some(left), Some(right)) => Some(if smaller(left, right) { left } else { right }),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    }
}

/// One of Appendix H's four presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Appendix H.1: the desktop and operator default.
    Interactive,
    /// Appendix H.2: `require`, seventy-two hours, a fifteen percent free-space floor.
    Cautious,
    /// Appendix H.3: fleet operation, where an unknown remote stops new batches.
    Fleet,
    /// Appendix H.4: no prompts, every acknowledgement supplied through policy.
    Scripted,
}

impl Profile {
    /// Every profile Appendix H defines.
    pub const ALL: &'static [Profile] = &[
        Profile::Interactive,
        Profile::Cautious,
        Profile::Fleet,
        Profile::Scripted,
    ];

    /// The name the profile is selected by.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Profile::Interactive => "interactive",
            Profile::Cautious => "cautious",
            Profile::Fleet => "fleet",
            Profile::Scripted => "scripted",
        }
    }

    /// Reads a profile back from its name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|profile| profile.as_str() == name)
    }

    /// The protection mode the profile asks for (Appendix H.1–H.4).
    #[must_use]
    pub const fn mode(self) -> ProtectionMode {
        match self {
            Profile::Interactive | Profile::Fleet | Profile::Scripted => ProtectionMode::Prefer,
            Profile::Cautious => ProtectionMode::Require,
        }
    }

    /// How long the profile retains recovery assets (Appendix H.1–H.3, §37.1).
    #[must_use]
    pub const fn retention(self) -> Duration {
        match self {
            Profile::Cautious => Duration::from_secs(72 * 60 * 60),
            _ => ono_change_core::DEFAULT_RETENTION,
        }
    }

    /// The cost limits the profile carries (Appendix H.2's minimum free space, §38.3).
    #[must_use]
    pub fn limits(self) -> CostLimits {
        match self {
            Profile::Cautious => {
                CostLimits::default().above_floor(FreeSpaceFloor::Share(Percent::new(15.0)))
            }
            _ => CostLimits::default(),
        }
    }

    /// Whether a command running under this profile may stop and ask (Appendix H.4, §17.4).
    #[must_use]
    pub const fn prompts(self) -> bool {
        !matches!(self, Profile::Scripted)
    }

    /// The lowest risk class the profile requires an acknowledgement for (Appendix H.1–H.4, §19.4).
    ///
    /// Appendix H writes it `moderate+` and `high+`: this class and every class above it. §19.2's
    /// `unknown` sits between `moderate` and `high`, so `moderate+` gates a plan whose risk nobody
    /// could classify as well, which is §2.4's rule that unknown is never read as ordinary.
    #[must_use]
    pub const fn risk_gate(self) -> RiskClass {
        match self {
            Profile::Cautious => RiskClass::Moderate,
            Profile::Interactive | Profile::Fleet | Profile::Scripted => RiskClass::High,
        }
    }

    /// The execution strategy the profile names, in §28.4's spelling (Appendix H.1–H.3).
    #[must_use]
    pub const fn strategy(self) -> &'static str {
        match self {
            Profile::Fleet => "canary 1 then batch 10%",
            Profile::Interactive | Profile::Cautious | Profile::Scripted => "sequential",
        }
    }

    /// The profile expanded into the settings it stands for (Appendix H).
    ///
    /// Appendix H requires a profile to *"expand to inspectable settings"* and forbids it hiding
    /// semantics, so the whole expansion is a value — including the keys other crates own, such
    /// as the risk gate and the execution strategy, which are reported here rather than applied
    /// here.
    #[must_use]
    pub fn settings(self) -> Vec<(&'static str, String)> {
        let mut settings = vec![
            ("protection", self.mode().as_str().to_owned()),
            ("retention", format!("{:?}", self.retention())),
            (
                "minimum free space",
                self.limits().min_filesystem_free().describe(),
            ),
            ("opaque actions", "disabled".to_owned()),
            (
                "prompts",
                if self.prompts() { "yes" } else { "no" }.to_owned(),
            ),
        ];
        settings.push(("risk gate", format!("{}+", self.risk_gate().as_str())));
        settings.push(("strategy", self.strategy().to_owned()));
        if self == Profile::Fleet {
            settings.push(("remote unknown", "stop new batches".to_owned()));
        }
        settings
    }
}

/// The protection policy in force for one plan (§17, §38.3).
#[derive(Debug, Clone, PartialEq)]
pub struct ProtectionPolicy {
    mode: ProtectionMode,
    requested: Option<ProtectionMode>,
    required_level: ProtectionLevel,
    retention: RetentionPolicy,
    limits: CostLimits,
    irrelevant_domains: Vec<EffectDomain>,
    excluded_hosts: Vec<Arc<str>>,
    authorises_early_removal: bool,
    profile: Option<Profile>,
}

impl Default for ProtectionPolicy {
    /// §17.1's default: `prefer`, with §37.1's retention and §53's limits.
    fn default() -> Self {
        Self {
            mode: ProtectionMode::Prefer,
            requested: None,
            required_level: ProtectionLevel::Protected,
            retention: RetentionPolicy::default(),
            limits: CostLimits::default(),
            irrelevant_domains: Vec::new(),
            excluded_hosts: Vec::new(),
            authorises_early_removal: false,
            profile: None,
        }
    }
}

impl ProtectionPolicy {
    /// A policy in `mode`, with every other setting at its §17.1 and §53 default.
    #[must_use]
    pub fn of(mode: ProtectionMode) -> Self {
        Self {
            mode,
            ..Self::default()
        }
    }

    /// Records what the plan itself asked for, so a narrowing is never silent (§17.3, §53).
    ///
    /// §17.2's four modes are ordered by [`mode_rank`], and that order is total while the
    /// underlying properties are not: `require` refuses on shortfall and `maximize` attempts every
    /// mechanism that fits, and neither implies the other. So a plan sealed with
    /// `--protection maximize` under a `require` configuration runs at `require` — the failing-
    /// closed answer — and loses the breadth it asked for. [`ProtectionPolicy::narrowed`] is what
    /// keeps that visible instead of leaving an operator to notice it from the coverage matrix.
    #[must_use]
    pub const fn asked_for(mut self, requested: ProtectionMode) -> Self {
        self.requested = Some(requested);
        self
    }

    /// The protection class `require` holds the plan to (§17.2).
    #[must_use]
    pub const fn requiring(mut self, level: ProtectionLevel) -> Self {
        self.required_level = level;
        self
    }

    /// Sets how long the assets are retained (§37.1).
    #[must_use]
    pub const fn retaining(mut self, retention: RetentionPolicy) -> Self {
        self.retention = retention;
        self
    }

    /// Sets the §38.3 cost limits.
    #[must_use]
    pub fn limited_by(mut self, limits: CostLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Declares `domain` irrelevant to the requested recovery objective (Appendix A.7).
    ///
    /// This is the only thing that stops an unknown domain capping a plan at partially protected,
    /// and it is an operator decision recorded in the plan rather than a default.
    #[must_use]
    pub fn declaring_irrelevant(mut self, domain: EffectDomain) -> Self {
        if !self.irrelevant_domains.contains(&domain) {
            self.irrelevant_domains.push(domain);
        }
        self
    }

    /// Excludes `host` from a remote plan's protection claim (§29.2).
    ///
    /// §29.2 calls a fleet with unprotected hosts partially protected "unless policy excludes the
    /// unprotected targets". The exclusion is an operator decision recorded in the plan, and the
    /// excluded host stays named among the plan's exclusions ([`crate::hosts::compose`]).
    #[must_use]
    pub fn excluding_host(mut self, host: impl Into<Arc<str>>) -> Self {
        let host = host.into();
        if !self.excluded_hosts.contains(&host) {
            self.excluded_hosts.push(host);
        }
        self
    }

    /// Whether the policy excludes `host` from the plan's protection claim (§29.2).
    #[must_use]
    pub fn excludes_host(&self, host: &str) -> bool {
        self.excluded_hosts
            .iter()
            .any(|excluded| &**excluded == host)
    }

    /// Authorises removing recovery assets before their retention ends (§37.4).
    #[must_use]
    pub const fn authorising_early_removal(mut self) -> Self {
        self.authorises_early_removal = true;
        self
    }

    /// The mode (§17.2).
    #[must_use]
    pub const fn mode(&self) -> ProtectionMode {
        self.mode
    }

    /// What the plan asked for, where the mode in force does not deliver it (§17.3, §53, ADR-0815).
    ///
    /// `None` when the plan asked for nothing, got what it asked for, or got a mode that contains
    /// it: a stricter mode than the plan asked for is a raise ([`ProtectionPolicy::raised`]), and
    /// raising is what §53 requires. The one pair that is a narrowing is `maximize` requested
    /// under `require`: `require` refuses on a shortfall and does not attempt every mechanism, so
    /// the plan lost the breadth it asked for. It is reported rather than refused.
    #[must_use]
    pub fn narrowed(&self) -> Option<ProtectionMode> {
        self.requested
            .filter(|requested| !delivers(self.mode, *requested))
    }

    /// What the plan asked for, where configuration raised it to a stricter mode (§53, ADR-0834).
    ///
    /// `Some(requested)` when the plan asked for less than a mode the operator configured — `off`
    /// under a configured `prefer` — and runs under the configured one. Nothing was lost, and the
    /// operator is still told, because a plan that said `off` and created an asset would otherwise
    /// look like a plan that ignored its own words.
    #[must_use]
    pub fn raised(&self) -> Option<ProtectionMode> {
        self.requested
            .filter(|requested| *requested != self.mode && delivers(self.mode, *requested))
    }

    /// The sentence a raise is shown as (§17.3, §53).
    #[must_use]
    pub fn raising_note(&self) -> Option<String> {
        self.raised().map(|requested| {
            format!(
                "the plan asked for `{requested}` protection and runs under `{}`, the mode the \
                 configuration requires; a plan may ask for more than configuration and never for \
                 less (§17.3, §53, ADR-0834)",
                self.mode
            )
        })
    }

    /// The sentence a narrowing is shown as (§17.3).
    #[must_use]
    pub fn narrowing_note(&self) -> Option<String> {
        self.narrowed().map(|requested| {
            format!(
                "the plan asked for `{requested}` protection and the configuration answered                  `{}`; the two are not ordered, and the mode that refuses on a shortfall was                  chosen (§17.2, §17.3)",
                self.mode
            )
        })
    }

    /// The protection class a `require` plan must reach (§17.2).
    #[must_use]
    pub const fn required_level(&self) -> ProtectionLevel {
        self.required_level
    }

    /// The retention (§37.1).
    #[must_use]
    pub const fn retention(&self) -> RetentionPolicy {
        self.retention
    }

    /// The cost limits (§38.3).
    #[must_use]
    pub const fn limits(&self) -> &CostLimits {
        &self.limits
    }

    /// The domains policy has declared irrelevant (Appendix A.7).
    #[must_use]
    pub fn irrelevant_domains(&self) -> &[EffectDomain] {
        &self.irrelevant_domains
    }

    /// Whether `domain` has been declared irrelevant (Appendix A.7).
    #[must_use]
    pub fn is_irrelevant(&self, domain: EffectDomain) -> bool {
        self.irrelevant_domains.contains(&domain)
    }

    /// Whether policy authorises early removal of a retained asset (§37.4).
    #[must_use]
    pub const fn authorises_early_removal(&self) -> bool {
        self.authorises_early_removal
    }

    /// The profile this policy was tightened with, where one was applied.
    #[must_use]
    pub const fn profile(&self) -> Option<Profile> {
        self.profile
    }

    /// Applies `profile`, and only where it makes the policy stricter (Appendix H.5).
    ///
    /// Appendix H.5 says a plan may impose stricter requirements than a profile and a profile
    /// MUST NOT weaken a safety constraint, so every field is combined by taking the stricter of
    /// the two: the higher mode, the longer retention, the higher free-space floor, the narrower
    /// cost cap. A `require` plan under a `fleet` profile is still `require`.
    #[must_use]
    pub fn tighten_with(mut self, profile: Profile) -> Self {
        if mode_rank(profile.mode()) > mode_rank(self.mode) {
            self.mode = profile.mode();
        }
        let profile_retention = RetentionPolicy::of(profile.retention());
        let window = self.retention.window().max(profile_retention.window());
        let mut retention = RetentionPolicy::of(window);
        if self.retention.is_held() {
            retention = retention.holding();
        }
        self.retention = retention;
        self.limits = self.limits.tightened_with(&profile.limits());
        self.profile = Some(profile);
        self
    }

    /// Whether `analysis` satisfies this policy (§17.2).
    #[must_use]
    pub fn is_satisfied_by(&self, analysis: &CoverageAnalysis) -> bool {
        !self.mode.refuses_shortfall()
            || level_rank(analysis.level()) >= level_rank(self.required_level)
    }

    /// Refuses the plan when `require` cannot be met (§17.2).
    ///
    /// # Errors
    ///
    /// `recovery.coverage_insufficient`, naming the domains that fell short. Nothing has been
    /// changed when this is raised: §17.2 refuses before PREPARE, not partway through it.
    pub fn enforce(&self, plan: &PlanId, analysis: &CoverageAnalysis) -> Result<(), ErrorValue> {
        if self.is_satisfied_by(analysis) {
            return Ok(());
        }
        let shortfall: Vec<String> = analysis
            .summary()
            .shortfall()
            .iter()
            .map(|row| row.domain().as_str().to_owned())
            .collect();
        Err(error::coverage_insufficient(
            plan,
            analysis.level(),
            self.required_level,
            &shortfall,
        ))
    }

    /// The sentence `off` shows about protection it did not create (§17.2).
    #[must_use]
    pub fn availability_note(&self, analysis: &CoverageAnalysis) -> Arc<str> {
        if self.mode.creates_assets() {
            return Arc::from("");
        }
        Arc::from(format!(
            "protection is off, and {} protection opportunit{} available (§17.2)",
            analysis.available().len(),
            if analysis.available().len() == 1 {
                "y was"
            } else {
                "ies were"
            }
        ))
    }
}

/// How strict a mode is, for Appendix H.5's "never weaker".
///
/// `off` creates nothing, `prefer` creates what is cheap, `maximize` creates everything that fits
/// the limits, and `require` refuses to proceed without coverage. Refusing is the strictest thing
/// a policy can do, so it sits at the top.
#[must_use]
pub const fn mode_rank(mode: ProtectionMode) -> u8 {
    match mode {
        ProtectionMode::Off => 0,
        ProtectionMode::Prefer => 1,
        ProtectionMode::Maximize => 2,
        ProtectionMode::Require => 3,
    }
}

/// Whether a plan running in `got` receives everything a request for `asked` would do (ADR-0815).
///
/// §17.2's modes are two properties — how much protection is attempted, and whether a shortfall
/// refuses — so containment is not the rank. `off` asks for nothing and every mode delivers it;
/// `prefer` is contained in `maximize` and `require`, which both attempt what it attempts;
/// `maximize` and `require` are each contained only in themselves.
const fn delivers(got: ProtectionMode, asked: ProtectionMode) -> bool {
    match asked {
        ProtectionMode::Off => true,
        ProtectionMode::Prefer => !matches!(got, ProtectionMode::Off),
        ProtectionMode::Maximize => matches!(got, ProtectionMode::Maximize),
        ProtectionMode::Require => matches!(got, ProtectionMode::Require),
    }
}

/// How much coverage a protection level stands for, for comparing against a requirement (§10.2).
///
/// `UNKNOWN` sits at the bottom with `UNPROTECTED`: §2.4 forbids reading an unestablished
/// recovery property as coverage, so a plan whose protection is unknown never satisfies a
/// requirement for a protected one.
#[must_use]
pub const fn level_rank(level: ProtectionLevel) -> u8 {
    match level {
        ProtectionLevel::Unknown => 0,
        ProtectionLevel::Unprotected => 1,
        ProtectionLevel::Compensatable => 2,
        ProtectionLevel::PartiallyProtected => 3,
        ProtectionLevel::Protected => 4,
        ProtectionLevel::Transactional => 5,
    }
}

/// The stricter of a configured mode and a mode the plan asked for (§53's last line, §17.3).
///
/// §53 ends with *"Configuration MUST NOT silently weaken explicit plan requirements"*, and §17.3
/// lets `plan ... --protection require` override configuration. Both hold at once when the
/// explicit requirement wins wherever it is stricter, which is what this returns.
#[must_use]
pub fn effective_mode(
    configured: ProtectionMode,
    requested: Option<ProtectionMode>,
) -> ProtectionMode {
    match requested {
        Some(requested) if mode_rank(requested) >= mode_rank(configured) => requested,
        Some(_) => configured,
        None => configured,
    }
}
