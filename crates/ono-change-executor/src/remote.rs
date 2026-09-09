//! Per-host fragments and what a dropped link leaves behind (spec v0.6 §29.1, §29.3).
//!
//! §29.1: *"Remote plans are decomposed into host/provider-local action fragments. Ono MUST NOT
//! imply global atomicity."* [`decompose`] is that split, and it is the reason nothing in this
//! module composes a plan-level success out of per-host ones — a [`RemoteRun`] answers per host,
//! and the caller that wants one word has to look at all of them.
//!
//! §29.3 is the rule the module exists for: *"If connectivity fails mid-plan, Ono MUST preserve
//! exact per-host action state and verification state. It MUST NOT mark unknown remote actions as
//! failed or successful without evidence."* So a host whose link dropped keeps the statuses it had
//! reached: the action in flight is [`ActionStatus::Unknown`], the ones after it stay
//! [`ActionStatus::Pending`] — nothing ever tried them — and the ones before it keep what the link
//! actually reported. Appendix F.2 makes the unknown one an uncertainty boundary rather than a
//! failure, and §55.10 case 45 is the same sentence from the operator's side.

use std::sync::Arc;

use ono_change_core::{ActionId, ActionStatus, ChangePlan, PlanAction, error};
use ono_value::ErrorValue;

use crate::execute::ExecutionOutcome;

/// What is known about the link to one host (§29.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// The link carried every action that was attempted.
    Connected,
    /// The link dropped, and what it was carrying at the time is unestablished.
    Lost,
    /// Nothing was attempted on this host, because the run stopped before reaching it.
    NotReached,
}

impl LinkState {
    /// The word an outcome renders.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            LinkState::Connected => "connected",
            LinkState::Lost => "lost",
            LinkState::NotReached => "not-reached",
        }
    }
}

/// One host's share of a plan (§29.1).
#[derive(Debug, Clone, PartialEq)]
pub struct HostFragment {
    host: Arc<str>,
    actions: Vec<PlanAction>,
}

impl HostFragment {
    /// The host this fragment runs on.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The actions, in the plan's own order.
    #[must_use]
    pub fn actions(&self) -> &[PlanAction] {
        &self.actions
    }
}

/// What one host's fragment established (§29.3).
#[derive(Debug, Clone, PartialEq)]
pub struct HostOutcome {
    host: Arc<str>,
    link: LinkState,
    statuses: Vec<(ActionId, ActionStatus)>,
    error: Option<ErrorValue>,
}

impl HostOutcome {
    /// The host.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// What is known about the link (§29.3).
    #[must_use]
    pub const fn link(&self) -> LinkState {
        self.link
    }

    /// The exact per-host action state §29.3 requires to be preserved.
    #[must_use]
    pub fn statuses(&self) -> &[(ActionId, ActionStatus)] {
        &self.statuses
    }

    /// What is known about one action on this host.
    #[must_use]
    pub fn status_of(&self, action: &ActionId) -> Option<ActionStatus> {
        self.statuses
            .iter()
            .find(|(id, _)| id == action)
            .map(|(_, status)| *status)
    }

    /// The actions whose outcome could not be established (Appendix F.2, §55.10 case 45).
    #[must_use]
    pub fn uncertainty_boundary(&self) -> Vec<ActionId> {
        self.statuses
            .iter()
            .filter(|(_, status)| *status == ActionStatus::Unknown)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// The structured refusal, where the link left something unresolved.
    #[must_use]
    pub const fn error(&self) -> Option<&ErrorValue> {
        self.error.as_ref()
    }

    /// Whether every action on this host settled successfully.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.link == LinkState::Connected
            && self
                .statuses
                .iter()
                .all(|(_, status)| *status == ActionStatus::Succeeded)
    }
}

/// Every host's share of one run, with no plan-level word composed over them (§29.1).
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteRun {
    hosts: Vec<HostOutcome>,
}

impl RemoteRun {
    /// The per-host outcomes, in the order the fragments were given.
    #[must_use]
    pub fn hosts(&self) -> &[HostOutcome] {
        &self.hosts
    }

    /// One host's outcome.
    #[must_use]
    pub fn host(&self, host: &str) -> Option<&HostOutcome> {
        self.hosts.iter().find(|outcome| outcome.host() == host)
    }

    /// The hosts whose link dropped (§29.3).
    #[must_use]
    pub fn disconnected(&self) -> Vec<&HostOutcome> {
        self.hosts
            .iter()
            .filter(|outcome| outcome.link == LinkState::Lost)
            .collect()
    }

    /// Whether any host holds an action nobody could resolve (Appendix F.2).
    #[must_use]
    pub fn has_unknown(&self) -> bool {
        self.hosts
            .iter()
            .any(|outcome| !outcome.uncertainty_boundary().is_empty())
    }
}

/// Splits `plan` into one fragment per host (§29.1).
///
/// A target with no host belongs to the local fragment, which is named by the empty host: §7.1
/// makes the host part of a remote target's identity, and an action about a local object is not
/// made remote by sitting in a plan that also reaches other machines.
#[must_use]
pub fn decompose(plan: &ChangePlan) -> Vec<HostFragment> {
    let mut fragments: Vec<HostFragment> = Vec::new();
    for action in plan.actions() {
        let host = action
            .target()
            .and_then(|identity| {
                plan.targets()
                    .iter()
                    .find(|target| target.identity() == identity)
            })
            .and_then(|target| target.host())
            .unwrap_or("");
        match fragments
            .iter_mut()
            .find(|fragment| fragment.host.as_ref() == host)
        {
            Some(fragment) => fragment.actions.push(action.clone()),
            None => fragments.push(HostFragment {
                host: Arc::from(host),
                actions: vec![action.clone()],
            }),
        }
    }
    fragments
}

/// Runs each fragment on its own host, preserving exactly what each link established (§29.3).
///
/// `execute` is the per-host execution, taken as a parameter so a dropped link is a scripted
/// answer rather than an unplugged cable. An [`ExecutionOutcome::Unknown`] ends that host's
/// fragment: the actions after it are never attempted and stay `PENDING`, because §29.3 forbids
/// marking them anything else without evidence, and the one that was in flight stays `UNKNOWN`.
///
/// A host that fails is likewise stopped and no other host is: §29.1 forbids implying global
/// atomicity, and stopping every host because one refused would be implying it in the other
/// direction.
#[must_use]
pub fn run_fragments(
    fragments: &[HostFragment],
    execute: &dyn Fn(&str, &PlanAction) -> ExecutionOutcome,
) -> RemoteRun {
    let mut hosts = Vec::new();
    for fragment in fragments {
        let mut statuses: Vec<(ActionId, ActionStatus)> = fragment
            .actions
            .iter()
            .map(|action| (action.id().clone(), ActionStatus::Pending))
            .collect();
        let mut link = LinkState::Connected;
        let mut failure = None;
        for (index, action) in fragment.actions.iter().enumerate() {
            let outcome = execute(&fragment.host, action);
            if let Some(entry) = statuses.get_mut(index) {
                entry.1 = outcome.status();
            }
            match outcome {
                ExecutionOutcome::Succeeded => {}
                ExecutionOutcome::Failed(refusal) => {
                    failure = Some(refusal);
                    break;
                }
                ExecutionOutcome::Unknown(refusal) => {
                    link = LinkState::Lost;
                    failure = Some(error::remote_state_unknown(
                        &fragment.host,
                        action.summary(),
                    ));
                    let _ = refusal;
                    break;
                }
            }
        }
        if fragment.actions.is_empty() {
            link = LinkState::NotReached;
        }
        hosts.push(HostOutcome {
            host: Arc::clone(&fragment.host),
            link,
            statuses,
            error: failure,
        });
    }
    RemoteRun { hosts }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_call_a_host_incomplete_while_anything_on_it_is_unresolved() {
        let plan = ono_change_core::PlanId::derive(&["p"]);
        let action = PlanAction::new(
            &plan,
            1,
            ono_change_core::ActionRole::Mutate,
            "restart nginx",
            ono_change_core::Execution::Program {
                program: Arc::from("/bin/true"),
                argv: Vec::new(),
            },
        );
        let outcome = HostOutcome {
            host: Arc::from("api-04"),
            link: LinkState::Lost,
            statuses: vec![(action.id().clone(), ActionStatus::Unknown)],
            error: None,
        };
        assert!(
            !outcome.is_complete(),
            "§29.3: an unknown remote action is not a success"
        );
        assert_eq!(outcome.uncertainty_boundary().len(), 1);
    }
}
