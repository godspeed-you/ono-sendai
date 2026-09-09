//! What the host checks before a package's claim about the past becomes evidence
//! (v0.5 §30.7, §37.3, §37.4).
//!
//! §37.1 states the boundary: "KUANG/11 may extend history and causality, but Ono core retains
//! authority over identity, evidence classes, causal labels, capability policy and rendering
//! truth." Everything in this module is one side of that sentence.
//!
//! Three rules do the work, and each is a refusal rather than a correction:
//!
//! - **A package cannot forge a source.** §37.3 makes the host validate source identity, and the
//!   host does it by *setting* it: a contributed event's evidence source is
//!   `kuang:<package-id>/<source-id>` ([`EvidenceSource::kuang`]), written by
//!   [`Contribution::attributed`] over whatever the package supplied. This is the same rule
//!   `restamp_provenance` already applies to emitted values.
//! - **A package cannot assert that an object exists outside what it can see.** §37.3's scope
//!   rule, and the one with teeth: an event whose subject is of a schema the package could not
//!   resolve through a permitted provider is refused, because a package able to manufacture an
//!   object's existence could manufacture anything reconstructed from it.
//! - **A package's causal claim is capped at `asserted`.** §37.4, unless the host contract
//!   explicitly trusts the package as authoritative for a domain — which is
//!   [`AuthoritativeDomains`], and empty by default. The cap lowers a claim and never raises
//!   one, because §7.2 forbids any API that raises a strength.
//!
//! The size and rate ceilings of §37.3 live here too, in [`ContributionLimits`]. They are what
//! keeps a contribution call from being a way to fill the ledger.

use std::collections::BTreeSet;

use jiff::Timestamp;
use ono_temporal_core::{EventKind, EvidenceSource, EvidenceStrength};
use serde_json::Value as Json;

/// The ceiling v0.5 §37.4 puts on the strength of a contributed causal claim.
///
/// The value `docs/contracts/temporal/causality.yaml` → `contributed_rules.strength_ceiling`
/// declares; `tests/temporal.rs` holds this constant against that registry rather than against a
/// second copy of the specification.
pub const CONTRIBUTED_STRENGTH_CEILING: EvidenceStrength = EvidenceStrength::Asserted;

/// How far ahead of the host clock a contributed timestamp may be before it is refused.
///
/// Not zero, because a package on a machine whose clock is a few milliseconds ahead is not
/// lying; not generous, because an event stamped in the future is an event that reorders a
/// timeline it was never part of. §24.4's default clock-uncertainty warning is 100 ms, and this
/// is the same order of magnitude for the same reason.
const FUTURE_TOLERANCE: jiff::SignedDuration = jiff::SignedDuration::from_secs(2);

/// The bounds §37.3 requires the host to put on contributed events.
///
/// "Event size/rate limits" is the last item on §37.3's validation list, and it is the one that
/// decides whether a contribution call is a feature or a way to fill the disk. Every ceiling is
/// the host's; a package declares none of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContributionLimits {
    /// How many events one call may carry.
    pub max_events_per_call: usize,
    /// How large one event's encoded form may be, in bytes.
    pub max_event_bytes: usize,
    /// How many events the instance may contribute within [`rate_window_seconds`].
    ///
    /// [`rate_window_seconds`]: Self::rate_window_seconds
    pub max_events_per_window: u32,
    /// The length of the rate window, in seconds.
    pub rate_window_seconds: i64,
}

impl Default for ContributionLimits {
    /// Bounds a well-behaved package never meets and a runaway one meets immediately.
    fn default() -> Self {
        Self {
            max_events_per_call: 256,
            max_event_bytes: 64 * 1024,
            max_events_per_window: 10_000,
            rate_window_seconds: 60,
        }
    }
}

/// Which packages the host contract trusts as authoritative for a domain (§37.4).
///
/// §37.4's exception, and nothing wider: "unless the host contract explicitly trusts that
/// package/source as authoritative for a domain". Empty by default, so the ceiling of
/// [`CONTRIBUTED_STRENGTH_CEILING`] applies to every package until an operator says otherwise
/// about a named package and a named domain. There is deliberately no wildcard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthoritativeDomains(BTreeSet<(String, String)>);

impl AuthoritativeDomains {
    /// Trusts nobody as authoritative, which is where every host starts.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Trusts `package` as authoritative for `domain`.
    #[must_use]
    pub fn trusting(mut self, package: &str, domain: &str) -> Self {
        self.0.insert((package.to_owned(), domain.to_owned()));
        self
    }

    /// Whether `package` is trusted as authoritative for `domain`.
    #[must_use]
    pub fn trusts(&self, package: &str, domain: &str) -> bool {
        self.0.contains(&(package.to_owned(), domain.to_owned()))
    }

    /// The strongest claim `package` may make about `domain` (§37.4, §7.2).
    ///
    /// The ceiling, or [`EvidenceStrength::Authoritative`] where the host contract trusts the
    /// package for that domain. Never anything in between, and never raised at the call site.
    #[must_use]
    pub fn ceiling_for(&self, package: &str, domain: &str) -> EvidenceStrength {
        if self.trusts(package, domain) {
            EvidenceStrength::Authoritative
        } else {
            CONTRIBUTED_STRENGTH_CEILING
        }
    }
}

/// Why a contribution was refused (§37.3, §37.4).
///
/// Each variant is one item of §37.3's validation list, so a refusal names which check it failed
/// rather than saying that something was wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContributionRefusal {
    /// The event is not the shape `ono.temporal-event/1` declares (§37.3: schema).
    Schema {
        /// What was missing or wrong.
        detail: String,
    },
    /// The event names a kind outside the closed list of §6.1 (§37.3: schema).
    UnknownKind {
        /// The kind the package wrote.
        kind: String,
    },
    /// A timestamp is unreadable or ahead of the host clock (§37.3: timestamps).
    Timestamp {
        /// Which field, and why it was refused.
        detail: String,
    },
    /// The event's subject is outside what the package can resolve (§37.3: scope visibility and
    /// referenced spatial identities).
    Invisible {
        /// The schema the package claimed an object of.
        schema: String,
    },
    /// The event is larger than one event may be (§37.3: event size).
    TooLarge {
        /// Its encoded size in bytes.
        bytes: usize,
        /// The ceiling.
        limit: usize,
    },
    /// The call carries more events than one call may (§37.3: event size).
    TooMany {
        /// How many the call carried.
        count: usize,
        /// The ceiling.
        limit: usize,
    },
    /// The instance has contributed more events than its rate allows (§37.3: rate limits).
    TooFast {
        /// The ceiling within the window.
        limit: u32,
        /// The window, in seconds.
        window_seconds: i64,
    },
    /// A contributed causal rule claims the project's own namespace (§31.5, §37.4).
    ReservedNamespace {
        /// The rule id the package wrote.
        rule_id: String,
    },
    /// A contributed link names a rule the package never declared (§37.4).
    UndeclaredRule {
        /// The rule id the link named.
        rule_id: String,
    },
    /// A contributed link names a relation class Ono does not have (§15.1, §37.1).
    UnknownRelation {
        /// The class the package wrote.
        relation: String,
    },
}

impl ContributionRefusal {
    /// The sentence a package and an operator both read.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Schema { detail } => {
                format!(
                    "a contributed temporal event is not the shape a temporal event has: {detail}"
                )
            }
            Self::UnknownKind { kind } => format!(
                "`{kind}` is not one of Ono's event kinds; a package refines a kind through \
                 `subtype` and the top-level kind stays Ono's (v0.5 section 6.1, section 37.1)"
            ),
            Self::Timestamp { detail } => {
                format!("a contributed timestamp was refused: {detail}")
            }
            Self::Invisible { schema } => format!(
                "the event asserts something about an object of `{schema}`, which this package \
                 cannot resolve through any permitted provider; a package may not assert that an \
                 object exists outside what it can see (v0.5 section 37.3)"
            ),
            Self::TooLarge { bytes, limit } => {
                format!("a contributed event is {bytes} bytes and one event may be at most {limit}")
            }
            Self::TooMany { count, limit } => {
                format!("the call carries {count} events and one call may carry at most {limit}")
            }
            Self::TooFast {
                limit,
                window_seconds,
            } => format!(
                "this package has contributed its {limit} events for the last {window_seconds} \
                 seconds; the rate ceiling is the host's and is not negotiable"
            ),
            Self::ReservedNamespace { rule_id } => format!(
                "`{rule_id}` claims the `ono.` namespace, which belongs to the project; a \
                 third-party causal rule is namespaced to its publisher (v0.5 section 37.4)"
            ),
            Self::UndeclaredRule { rule_id } => format!(
                "no rule `{rule_id}` is declared in this package's `causal_rules` contribution, \
                 and a link may only come from a rule a reader can go and read"
            ),
            Self::UnknownRelation { relation } => format!(
                "`{relation}` is not one of Ono's five causal relation classes; core retains \
                 authority over causal labels (v0.5 section 15.1, section 37.1)"
            ),
        }
    }

    /// The structured code the refusal answers with.
    ///
    /// Three codes for three kinds of wrong, all of them already in the taxonomy: a claim
    /// outside what the package may see is a scope violation, in the same words a path outside a
    /// granted `paths` scope is refused with (§31.16); a ceiling met is the shipped resource
    /// family of v0.2 §43; everything else is a payload that is not the shape it claims to be.
    #[must_use]
    pub const fn code(&self) -> ono_core::ErrorCode {
        use ono_core::ErrorCode as Code;
        match self {
            Self::Invisible { .. } => Code::KuangCapabilityScopeViolation,
            Self::TooLarge { .. } => Code::ResourceByteLimit,
            Self::TooMany { .. } | Self::TooFast { .. } => Code::ResourceItemLimit,
            _ => Code::KuangRuntimeSchemaViolation,
        }
    }
}

impl From<&ContributionRefusal> for ono_kuang_protocol::WireError {
    fn from(refusal: &ContributionRefusal) -> Self {
        Self::from_core(refusal.code(), refusal.message())
    }
}

/// What the host may see about a contributed event, once it has been validated (§37.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventClaim {
    /// The canonical kind, resolved against the closed list of §6.1.
    pub kind: EventKind,
    /// The schema of the object the event is about, where it names one.
    pub subject_schema: Option<String>,
    /// The instant the package says the thing happened.
    pub source_time: Option<Timestamp>,
    /// The instant the package says it saw it.
    pub observed_at: Option<Timestamp>,
    /// The evidence source the host attributed the event to.
    pub source: EvidenceSource,
}

/// What a package is allowed to see, as the granted policy describes it (§37.3).
///
/// The scope rule of §37.3 asks a question about *providers*: can this package resolve an object
/// of this schema? The supervisor answers it from the grant on `object.read` and from the
/// schemas the package itself contributes, and hands the answer here as a set of schema ids.
/// `None` is a package holding an unscoped `object.read`, which can resolve anything the host
/// can and therefore may say so.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibleSchemas {
    /// The schema ids the package can reach, or `None` for "everything the host can".
    resolvable: Option<BTreeSet<String>>,
}

impl VisibleSchemas {
    /// A package that can resolve nothing: the honest default for one holding no `object.read`.
    #[must_use]
    pub fn none() -> Self {
        Self {
            resolvable: Some(BTreeSet::new()),
        }
    }

    /// A package that can resolve anything the host can, because its grant is unscoped.
    #[must_use]
    pub fn unscoped() -> Self {
        Self { resolvable: None }
    }

    /// A package that can resolve exactly these schema ids.
    #[must_use]
    pub fn of<I: IntoIterator<Item = String>>(schemas: I) -> Self {
        Self {
            resolvable: Some(schemas.into_iter().collect()),
        }
    }

    /// Adds a schema the package contributes itself, which it can always speak about.
    #[must_use]
    pub fn and_own(mut self, schema: &str) -> Self {
        if let Some(resolvable) = &mut self.resolvable {
            resolvable.insert(schema.to_owned());
        }
        self
    }

    /// Whether the package can resolve an object of `schema` through a permitted provider.
    #[must_use]
    pub fn can_resolve(&self, schema: &str) -> bool {
        match &self.resolvable {
            None => true,
            Some(resolvable) => resolvable.iter().any(|known| known == schema),
        }
    }
}

/// One package's contribution surface: what it may claim, and how fast.
#[derive(Debug, Clone)]
pub struct Contribution {
    package: String,
    limits: ContributionLimits,
    visible: VisibleSchemas,
    declared_rules: BTreeSet<String>,
    authoritative: AuthoritativeDomains,
    window_started: Option<Timestamp>,
    in_window: u32,
}

impl Contribution {
    /// The surface for `package`, with the host's default ceilings and nothing else granted.
    #[must_use]
    pub fn new(package: &str) -> Self {
        Self {
            package: package.to_owned(),
            limits: ContributionLimits::default(),
            visible: VisibleSchemas::none(),
            declared_rules: BTreeSet::new(),
            authoritative: AuthoritativeDomains::none(),
            window_started: None,
            in_window: 0,
        }
    }

    /// Replaces the host's ceilings.
    #[must_use]
    pub const fn with_limits(mut self, limits: ContributionLimits) -> Self {
        self.limits = limits;
        self
    }

    /// States what the package can resolve through permitted providers (§37.3).
    #[must_use]
    pub fn seeing(mut self, visible: VisibleSchemas) -> Self {
        self.visible = visible;
        self
    }

    /// Registers the causal rules the package declared in its manifest contribution (§37.4).
    #[must_use]
    pub fn declaring<I: IntoIterator<Item = String>>(mut self, rules: I) -> Self {
        self.declared_rules = rules.into_iter().collect();
        self
    }

    /// States which domains the host contract trusts this package as authoritative for (§37.4).
    #[must_use]
    pub fn trusted_for(mut self, authoritative: AuthoritativeDomains) -> Self {
        self.authoritative = authoritative;
        self
    }

    /// The package this surface belongs to.
    #[must_use]
    pub fn package(&self) -> &str {
        &self.package
    }

    /// The §7.1 evidence source the host attributes this package's claims to.
    ///
    /// `kuang:<package-id>/<source-id>`, always. A package that supplied a `source` field has it
    /// overwritten rather than argued with: attribution is the host's, so there is nothing to
    /// negotiate and nothing a package can say that changes it (§37.3).
    #[must_use]
    pub fn attributed(&self, source_id: &str) -> EvidenceSource {
        EvidenceSource::kuang(&self.package, source_id).unwrap_or_else(EvidenceSource::session)
    }

    /// Validates a whole call's worth of events against §37.3, in the order §37.3 lists.
    ///
    /// `now` is the host clock; nothing here reads one. `source_id` names which of the package's
    /// contributed sources the events came through, and becomes the second half of the evidence
    /// source the host stamps on them.
    ///
    /// # Errors
    ///
    /// The first [`ContributionRefusal`] any event meets. A call is refused whole: a partially
    /// accepted contribution would leave the package unable to say which half landed, and the
    /// ledger holding half a story.
    pub fn check_events(
        &mut self,
        events: &[Json],
        source_id: &str,
        now: Timestamp,
    ) -> Result<Vec<EventClaim>, ContributionRefusal> {
        if events.len() > self.limits.max_events_per_call {
            return Err(ContributionRefusal::TooMany {
                count: events.len(),
                limit: self.limits.max_events_per_call,
            });
        }
        self.charge_rate(events.len(), now)?;
        events
            .iter()
            .map(|event| self.check_event(event, source_id, now))
            .collect()
    }

    /// Validates one event against §37.3's list.
    fn check_event(
        &self,
        event: &Json,
        source_id: &str,
        now: Timestamp,
    ) -> Result<EventClaim, ContributionRefusal> {
        let encoded = event.to_string();
        if encoded.len() > self.limits.max_event_bytes {
            return Err(ContributionRefusal::TooLarge {
                bytes: encoded.len(),
                limit: self.limits.max_event_bytes,
            });
        }
        let kind_name = event.get("kind").and_then(Json::as_str).ok_or_else(|| {
            ContributionRefusal::Schema {
                detail: "it names no event kind".to_owned(),
            }
        })?;
        let kind =
            EventKind::from_name(kind_name).ok_or_else(|| ContributionRefusal::UnknownKind {
                kind: kind_name.to_owned(),
            })?;

        let source_time = read_instant(event, "source_time", now)?;
        let observed_at = read_instant(event, "observed_at", now)?;
        if source_time.is_none() && observed_at.is_none() {
            return Err(ContributionRefusal::Schema {
                detail: "it carries neither a source time nor an observation time, so there is \
                         no instant it could be placed at"
                    .to_owned(),
            });
        }

        let subject_schema = subject_schema(event);
        if let Some(schema) = &subject_schema
            && !self.visible.can_resolve(schema)
        {
            return Err(ContributionRefusal::Invisible {
                schema: schema.clone(),
            });
        }

        Ok(EventClaim {
            kind,
            subject_schema,
            source_time,
            observed_at,
            source: self.attributed(source_id),
        })
    }

    /// Validates a contributed causal link against §37.4, and lowers its strength to the ceiling.
    ///
    /// The returned strength is what the link actually carries: the weaker of what the package
    /// declared and what the host permits it. §7.2 forbids raising one, so a package that
    /// declares `correlated` keeps `correlated` even where the host trusts it.
    ///
    /// # Errors
    ///
    /// [`ContributionRefusal::ReservedNamespace`] for a rule claiming `ono.*`,
    /// [`ContributionRefusal::UndeclaredRule`] for one the package never declared,
    /// [`ContributionRefusal::UnknownRelation`] for a class Ono does not have, and
    /// [`ContributionRefusal::Schema`] for a link that is not the shape a link has.
    pub fn check_link(
        &self,
        link: &Json,
        domain: &str,
    ) -> Result<(String, EvidenceStrength), ContributionRefusal> {
        let rule_id =
            link.get("rule")
                .and_then(Json::as_str)
                .ok_or_else(|| ContributionRefusal::Schema {
                    detail: "a causal link names the registered rule that emitted it (v0.5 §15.8)"
                        .to_owned(),
                })?;
        if rule_id.starts_with("ono.") {
            return Err(ContributionRefusal::ReservedNamespace {
                rule_id: rule_id.to_owned(),
            });
        }
        if !self.declared_rules.contains(rule_id) {
            return Err(ContributionRefusal::UndeclaredRule {
                rule_id: rule_id.to_owned(),
            });
        }
        let relation = link.get("relation").and_then(Json::as_str).ok_or_else(|| {
            ContributionRefusal::Schema {
                detail: "a causal link names its relation class".to_owned(),
            }
        })?;
        if ono_temporal_core::CausalRelation::from_name(relation).is_none() {
            return Err(ContributionRefusal::UnknownRelation {
                relation: relation.to_owned(),
            });
        }
        let declared = link
            .get("strength")
            .and_then(Json::as_str)
            .and_then(EvidenceStrength::from_name)
            .ok_or_else(|| ContributionRefusal::Schema {
                detail: "a causal link states the evidence strength it carries (v0.5 §7.2)"
                    .to_owned(),
            })?;
        // §37.4's ceiling, applied through the one operation §7.2 permits. `weakest_of` cannot
        // raise anything, which is why it is the operation used rather than a comparison and an
        // assignment: there is no branch here in which a claim gets stronger.
        Ok((
            rule_id.to_owned(),
            declared.weakest_of(self.authoritative.ceiling_for(&self.package, domain)),
        ))
    }

    /// Charges `count` events against the rate window, refusing the call that would exceed it.
    fn charge_rate(&mut self, count: usize, now: Timestamp) -> Result<(), ContributionRefusal> {
        let window = jiff::SignedDuration::from_secs(self.limits.rate_window_seconds);
        let expired = self
            .window_started
            .is_none_or(|started| now.duration_since(started) >= window);
        if expired {
            self.window_started = Some(now);
            self.in_window = 0;
        }
        let charged = self
            .in_window
            .saturating_add(u32::try_from(count).unwrap_or(u32::MAX));
        if charged > self.limits.max_events_per_window {
            return Err(ContributionRefusal::TooFast {
                limit: self.limits.max_events_per_window,
                window_seconds: self.limits.rate_window_seconds,
            });
        }
        self.in_window = charged;
        Ok(())
    }
}

/// The schema of the object an event is about, where it names one.
///
/// `ono.temporal-event/1` carries the subject as a reference with its own schema id; a package
/// that names no subject is talking about a scope rather than an object, which the scope rule
/// has nothing to say about.
fn subject_schema(event: &Json) -> Option<String> {
    let subject = event.get("subject")?;
    subject
        .get("schema")
        .and_then(Json::as_str)
        .or_else(|| subject.get("object_type").and_then(Json::as_str))
        .map(str::to_owned)
}

/// Reads one timestamp field, refusing an unreadable one and one ahead of the host clock.
fn read_instant(
    event: &Json,
    field: &str,
    now: Timestamp,
) -> Result<Option<Timestamp>, ContributionRefusal> {
    let Some(value) = event.get(field).filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let text = value
        .as_str()
        .ok_or_else(|| ContributionRefusal::Timestamp {
            detail: format!("`{field}` is not an RFC 3339 instant"),
        })?;
    let at: Timestamp = text.parse().map_err(|_| ContributionRefusal::Timestamp {
        detail: format!("`{field}` is `{text}`, which is not an RFC 3339 instant"),
    })?;
    if at.duration_since(now) > FUTURE_TOLERANCE {
        return Err(ContributionRefusal::Timestamp {
            detail: format!(
                "`{field}` is {at}, which is ahead of this host's clock; an event stamped in the \
                 future reorders a timeline it was never part of"
            ),
        });
    }
    Ok(Some(at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn instant(text: &str) -> Timestamp {
        text.parse().unwrap_or(Timestamp::UNIX_EPOCH)
    }

    fn now() -> Timestamp {
        instant("2026-08-31T12:00:00Z")
    }

    #[test]
    fn should_attribute_a_contribution_to_the_package_whatever_it_claimed() {
        let surface = Contribution::new("dev.example.packet-eye");
        assert_eq!(
            surface.attributed("flows").as_str(),
            "kuang:dev.example.packet-eye/flows",
            "§37.3: the host owns attribution, so a package cannot forge a source"
        );
    }

    #[test]
    fn should_cap_a_contributed_causal_claim_at_asserted() {
        let surface = Contribution::new("dev.example.packet-eye")
            .declaring(["dev.example.packet-eye.retransmit".to_owned()]);
        let (_, strength) = surface
            .check_link(
                &json!({
                    "rule": "dev.example.packet-eye.retransmit",
                    "relation": "caused_by",
                    "strength": "authoritative",
                }),
                "network",
            )
            .expect("the link is well formed");

        assert_eq!(
            strength,
            EvidenceStrength::Asserted,
            "§37.4: plugin causal strength MUST NOT exceed `asserted` by default"
        );
    }

    #[test]
    fn should_leave_a_weaker_claim_weak_when_the_host_trusts_the_package() {
        let surface = Contribution::new("dev.example.packet-eye")
            .declaring(["dev.example.packet-eye.retransmit".to_owned()])
            .trusted_for(
                AuthoritativeDomains::none().trusting("dev.example.packet-eye", "network"),
            );
        let (_, strength) = surface
            .check_link(
                &json!({
                    "rule": "dev.example.packet-eye.retransmit",
                    "relation": "correlated_with",
                    "strength": "correlated",
                }),
                "network",
            )
            .expect("the link is well formed");

        assert_eq!(
            strength,
            EvidenceStrength::Correlated,
            "§7.2: no path raises a strength, including the trust exception"
        );
    }

    #[test]
    fn should_refuse_an_event_about_an_object_the_package_cannot_resolve() {
        let mut surface = Contribution::new("dev.example.packet-eye").seeing(VisibleSchemas::of([
            "dev.example.packet-eye.flow/1".to_owned(),
        ]));

        let refusal = surface
            .check_events(
                &[json!({
                    "kind": "object.appeared",
                    "observed_at": "2026-08-31T11:59:00Z",
                    "subject": {"schema": "ono.process/1"},
                })],
                "flows",
                now(),
            )
            .expect_err("§37.3: a plugin cannot assert an object outside what it can resolve");

        assert_eq!(
            refusal,
            ContributionRefusal::Invisible {
                schema: "ono.process/1".to_owned()
            }
        );
    }

    #[test]
    fn should_refuse_a_timestamp_ahead_of_the_host_clock() {
        let mut surface = Contribution::new("dev.example.packet-eye");
        let refusal = surface
            .check_events(
                &[json!({"kind": "object.observed", "observed_at": "2026-08-31T13:00:00Z"})],
                "flows",
                now(),
            )
            .expect_err("§37.3: the host validates timestamps");

        assert!(matches!(refusal, ContributionRefusal::Timestamp { .. }));
    }

    #[test]
    fn should_refuse_more_events_than_the_rate_ceiling_allows() {
        let mut surface =
            Contribution::new("dev.example.packet-eye").with_limits(ContributionLimits {
                max_events_per_window: 1,
                ..ContributionLimits::default()
            });
        let event = json!({"kind": "object.observed", "observed_at": "2026-08-31T11:59:00Z"});

        assert!(
            surface
                .check_events(std::slice::from_ref(&event), "flows", now())
                .is_ok()
        );
        let refusal = surface
            .check_events(std::slice::from_ref(&event), "flows", now())
            .expect_err("§37.3: the host enforces a rate limit");

        assert!(matches!(refusal, ContributionRefusal::TooFast { .. }));
    }
}
