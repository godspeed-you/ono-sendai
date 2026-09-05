//! The package lifecycle of spec §31.8, with legal transitions enforced in the type.
//!
//! The point of the model, `docs/contracts/kuang/lifecycle.v1.yaml` verbatim: "KUANG/11 MUST
//! distinguish package presence from code execution." A package that is installed has run
//! nothing; only `load` instantiates a runtime, and only an invocation makes it active.
//!
//! The six states are one flat enum, because `get plugin` shows one flat STATE column
//! (spec §31.8's example table, ADR-0022 §5). Internally [`Lifecycle`] additionally tracks the
//! degradation flag and the invocation count, so that a degraded package that finishes its last
//! invocation returns to `degraded` rather than forgetting what was denied (ADR-0041).

use serde::{Deserialize, Serialize};

/// One of the six lifecycle states of spec §31.8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginState {
    /// The artifact exists locally and its metadata and signature have been validated. Nothing
    /// of it has executed.
    Installed,
    /// Eligible for loading under current policy. Still nothing has executed.
    Enabled,
    /// A runtime instance exists and contributions are registered.
    Loaded,
    /// One or more commands, streams, views or assistant turns are executing.
    Active,
    /// Loaded, with one or more optional dependencies or capabilities unavailable.
    Degraded,
    /// Installed, and prevented from loading by trust, integrity or policy failure. Inert.
    Quarantined,
}

impl PluginState {
    /// Every state, in the order `docs/contracts/kuang/lifecycle.v1.yaml` declares them.
    pub const ALL: &'static [PluginState] = &[
        PluginState::Installed,
        PluginState::Enabled,
        PluginState::Loaded,
        PluginState::Active,
        PluginState::Degraded,
        PluginState::Quarantined,
    ];

    /// The state as `get plugin` renders it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PluginState::Installed => "installed",
            PluginState::Enabled => "enabled",
            PluginState::Loaded => "loaded",
            PluginState::Active => "active",
            PluginState::Degraded => "degraded",
            PluginState::Quarantined => "quarantined",
        }
    }

    /// Whether any package code has run to reach this state (spec §31.8).
    #[must_use]
    pub const fn code_has_run(self) -> bool {
        matches!(
            self,
            PluginState::Loaded | PluginState::Active | PluginState::Degraded
        )
    }
}

/// A transition that is not legal from the current state.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{event}` is not a legal lifecycle transition from `{from}`")]
pub struct TransitionError {
    /// The state the machine was in.
    pub from: &'static str,
    /// The transition that was attempted.
    pub event: &'static str,
}

/// The lifecycle state machine of `docs/contracts/kuang/lifecycle.v1.yaml`.
///
/// Every method is a transition from that contract's table; an illegal one is refused with
/// [`TransitionError`] rather than absorbed, because a supervisor that silently repairs its own
/// state machine is a supervisor whose states mean nothing.
///
/// ```
/// use ono_kuang_protocol::{Lifecycle, PluginState};
/// let mut lifecycle = Lifecycle::installed();
/// lifecycle.enable()?;
/// lifecycle.load(false)?;
/// assert_eq!(lifecycle.state(), PluginState::Loaded);
/// assert!(lifecycle.load(false).is_err(), "load is only legal from enabled");
/// # Ok::<(), ono_kuang_protocol::TransitionError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lifecycle {
    base: Base,
    degraded: bool,
    invocations: u32,
    quarantine_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Base {
    Installed,
    Enabled,
    Loaded,
    Quarantined,
}

impl Lifecycle {
    /// A freshly installed package: metadata validated, nothing executed.
    #[must_use]
    pub const fn installed() -> Self {
        Self {
            base: Base::Installed,
            degraded: false,
            invocations: 0,
            quarantine_reason: None,
        }
    }

    /// The flat state `get plugin` shows (spec §31.8).
    #[must_use]
    pub const fn state(&self) -> PluginState {
        match self.base {
            Base::Installed => PluginState::Installed,
            Base::Enabled => PluginState::Enabled,
            Base::Quarantined => PluginState::Quarantined,
            Base::Loaded => {
                if self.invocations > 0 {
                    PluginState::Active
                } else if self.degraded {
                    PluginState::Degraded
                } else {
                    PluginState::Loaded
                }
            }
        }
    }

    /// Whether the instance runs with denied optional capabilities, whatever else it is doing.
    #[must_use]
    pub const fn is_degraded(&self) -> bool {
        matches!(self.base, Base::Loaded) && self.degraded
    }

    /// Why the package was quarantined, when it was.
    #[must_use]
    pub fn quarantine_reason(&self) -> Option<&str> {
        self.quarantine_reason.as_deref()
    }

    fn refuse(&self, event: &'static str) -> TransitionError {
        TransitionError {
            from: self.state().as_str(),
            event,
        }
    }

    /// `installed -> enabled`. Runs no package code.
    pub fn enable(&mut self) -> Result<(), TransitionError> {
        match self.base {
            Base::Installed => {
                self.base = Base::Enabled;
                Ok(())
            }
            _ => Err(self.refuse("enable")),
        }
    }

    /// `enabled -> installed`. A loaded instance must be unloaded first.
    pub fn disable(&mut self) -> Result<(), TransitionError> {
        match self.base {
            Base::Enabled => {
                self.base = Base::Installed;
                Ok(())
            }
            _ => Err(self.refuse("disable")),
        }
    }

    /// `enabled -> loaded` (or `degraded`, when an optional capability was denied at load).
    ///
    /// The only transition on the main path that runs package code for the first time.
    pub fn load(&mut self, degraded: bool) -> Result<(), TransitionError> {
        match self.base {
            Base::Enabled => {
                self.base = Base::Loaded;
                self.degraded = degraded;
                Ok(())
            }
            _ => Err(self.refuse("load")),
        }
    }

    /// `loaded | degraded -> enabled`. Active invocations must have been drained by the caller.
    pub fn unload(&mut self) -> Result<(), TransitionError> {
        match self.base {
            Base::Loaded if self.invocations == 0 => {
                self.base = Base::Enabled;
                self.degraded = false;
                Ok(())
            }
            _ => Err(self.refuse("unload")),
        }
    }

    /// `loaded | degraded | active -> active`: one more invocation is executing.
    pub fn begin_invocation(&mut self) -> Result<(), TransitionError> {
        match self.base {
            Base::Loaded => {
                self.invocations += 1;
                Ok(())
            }
            _ => Err(self.refuse("begin-invocation")),
        }
    }

    /// `active -> loaded | degraded` when the last invocation finishes.
    pub fn end_invocation(&mut self) -> Result<(), TransitionError> {
        match self.base {
            Base::Loaded if self.invocations > 0 => {
                self.invocations -= 1;
                Ok(())
            }
            _ => Err(self.refuse("end-invocation")),
        }
    }

    /// `loaded -> degraded`: an optional capability or dependency became unavailable.
    pub fn degrade(&mut self) -> Result<(), TransitionError> {
        match self.base {
            Base::Loaded => {
                self.degraded = true;
                Ok(())
            }
            _ => Err(self.refuse("degrade")),
        }
    }

    /// `degraded -> loaded`: the denied optional capability became available again.
    pub fn restore(&mut self) -> Result<(), TransitionError> {
        match self.base {
            Base::Loaded if self.degraded => {
                self.degraded = false;
                Ok(())
            }
            _ => Err(self.refuse("restore")),
        }
    }

    /// `any -> quarantined`: trust, integrity or policy failed — including a protocol violation
    /// at runtime, which is a policy failure in the supervisor's eyes (ADR-0041). A loaded
    /// instance is terminated by the caller first; the artifact is retained, inert.
    pub fn quarantine(&mut self, reason: impl Into<String>) {
        self.base = Base::Quarantined;
        self.degraded = false;
        self.invocations = 0;
        self.quarantine_reason = Some(reason.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_walk_the_main_path_of_spec_31_8_when_each_step_is_legal() {
        let mut lifecycle = Lifecycle::installed();
        assert_eq!(lifecycle.state(), PluginState::Installed);
        lifecycle.enable().expect("install -> enable");
        assert_eq!(lifecycle.state(), PluginState::Enabled);
        lifecycle.load(false).expect("enable -> load");
        assert_eq!(lifecycle.state(), PluginState::Loaded);
        lifecycle.begin_invocation().expect("load -> active");
        assert_eq!(lifecycle.state(), PluginState::Active);
        lifecycle.end_invocation().expect("active -> loaded");
        assert_eq!(lifecycle.state(), PluginState::Loaded);
        lifecycle.unload().expect("loaded -> enabled");
        assert_eq!(lifecycle.state(), PluginState::Enabled);
    }

    #[test]
    fn should_refuse_to_load_when_the_package_is_only_installed() {
        // Presence is not execution eligibility: install grants nothing (spec §31.8, §31.9).
        let mut lifecycle = Lifecycle::installed();
        assert!(lifecycle.load(false).is_err());
        assert!(!lifecycle.state().code_has_run());
    }

    #[test]
    fn should_stay_degraded_when_the_last_invocation_of_a_degraded_package_ends() {
        let mut lifecycle = Lifecycle::installed();
        lifecycle.enable().expect("enable");
        lifecycle.load(true).expect("load degraded");
        assert_eq!(lifecycle.state(), PluginState::Degraded);
        lifecycle.begin_invocation().expect("invoke");
        assert_eq!(lifecycle.state(), PluginState::Active);
        lifecycle.end_invocation().expect("finish");
        assert_eq!(
            lifecycle.state(),
            PluginState::Degraded,
            "a denial does not disappear because an invocation finished (spec §31.63)"
        );
    }

    #[test]
    fn should_refuse_to_unload_while_an_invocation_is_active() {
        let mut lifecycle = Lifecycle::installed();
        lifecycle.enable().expect("enable");
        lifecycle.load(false).expect("load");
        lifecycle.begin_invocation().expect("invoke");
        assert!(
            lifecycle.unload().is_err(),
            "active invocations are drained before unload (lifecycle.v1.yaml)"
        );
    }

    #[test]
    fn should_be_inert_after_quarantine_whatever_is_tried() {
        let mut lifecycle = Lifecycle::installed();
        lifecycle.quarantine("signature failed re-verification");
        assert_eq!(lifecycle.state(), PluginState::Quarantined);
        assert!(lifecycle.enable().is_err());
        assert!(lifecycle.load(false).is_err());
        assert!(lifecycle.begin_invocation().is_err());
        assert_eq!(
            lifecycle.quarantine_reason(),
            Some("signature failed re-verification")
        );
    }

    #[test]
    fn should_restore_a_degraded_package_when_the_capability_returns() {
        let mut lifecycle = Lifecycle::installed();
        lifecycle.enable().expect("enable");
        lifecycle.load(true).expect("load degraded");
        lifecycle.restore().expect("restore");
        assert_eq!(lifecycle.state(), PluginState::Loaded);
        assert!(lifecycle.restore().is_err(), "restore needs a degradation");
    }
}
