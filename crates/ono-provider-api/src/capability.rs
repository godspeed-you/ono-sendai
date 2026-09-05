//! What a provider must be allowed to do, and how much it matters.
//!
//! These are the *provider* capabilities of `docs/contracts/capabilities.yaml` — what a command needs
//! from a provider for it to work at all. They are not the KUANG/11 capabilities of spec §31.16,
//! which are a security boundary granted to an extension. Conflating the two is how someone
//! eventually grants a plugin `process.list` believing it is `process.read` (ADR-0012).

use std::fmt;

/// How much a capability could change or reveal.
///
/// Spec §17.1 computes a risk descriptor from this together with the number of targets, the
/// context, the privilege level and irreversibility. This is the part the provider declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Risk {
    /// A point-in-time query with no side effect.
    Read,
    /// A continuous subscription: it holds resources and keeps running.
    Observe,
    /// Changes state outside the shell, reversibly.
    Mutate,
    /// May cause irreversible loss.
    Destructive,
}

impl Risk {
    /// The name `docs/contracts/capabilities.yaml` uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Risk::Read => "read",
            Risk::Observe => "observe",
            Risk::Mutate => "mutate",
            Risk::Destructive => "destructive",
        }
    }

    /// Whether an operation of this risk changes anything outside the shell.
    #[must_use]
    pub const fn changes_the_world(self) -> bool {
        matches!(self, Risk::Mutate | Risk::Destructive)
    }
}

impl fmt::Display for Risk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One thing a provider can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    id: String,
    risk: Risk,
    elevation: bool,
}

impl Capability {
    /// A capability an ordinary user has.
    #[must_use]
    pub fn new(id: impl Into<String>, risk: Risk) -> Self {
        Self {
            id: id.into(),
            risk,
            elevation: false,
        }
    }

    /// Marks the capability as needing elevated privilege.
    #[must_use]
    pub fn needing_elevation(mut self) -> Self {
        self.elevation = true;
        self
    }

    /// The capability's id, as `docs/contracts/capabilities.yaml` spells it.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// How much it could change or reveal.
    #[must_use]
    pub fn risk(&self) -> Risk {
        self.risk
    }

    /// Whether it needs elevated privilege.
    #[must_use]
    pub fn needs_elevation(&self) -> bool {
        self.elevation
    }
}

/// Whether a provider can answer on this machine, and why not when it cannot.
///
/// Spec §35.3 and ADR-0015: a provider that is not available must say so. Returning an empty
/// result would be indistinguishable from "there are none", which is the conflation between
/// absence and ignorance the whole value model exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    /// The provider can answer.
    Available,
    /// The provider cannot answer here, for this reason.
    Unavailable(String),
}

impl Availability {
    /// A provider that cannot answer, with the reason a user needs.
    #[must_use]
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Availability::Unavailable(reason.into())
    }

    /// Whether the provider can answer.
    #[must_use]
    pub fn is_available(&self) -> bool {
        matches!(self, Availability::Available)
    }

    /// Why it cannot, when it cannot.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Availability::Available => None,
            Availability::Unavailable(reason) => Some(reason),
        }
    }
}
