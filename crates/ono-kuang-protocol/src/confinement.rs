//! The confinement vocabulary both sides of the boundary name things with (v0.4.1 §16, §17.2).
//!
//! v0.4.1 §16.4 asks for one central table in which every confinement control is declared
//! `mandatory` or `best_effort`, and §52.1 asks for that table as a machine-readable registry
//! rather than as constants scattered through the code. The registry is
//! `docs/spec/hardening/kuang_confinement_controls.yaml`; this module is the same table as Rust,
//! and `cargo xtask spec-check` compares the two in both directions, so a control the runtime
//! knows and the registry omits — or the reverse — turns the gate red (§52.3, ADR-0442).
//!
//! It lives in the protocol crate rather than in the supervisor because a control id is
//! *vocabulary*: it appears in the structured errors of §16.3, in the confinement report of
//! §16.5, in the audit trail and in `inspect plugin`, all of which cross the boundary this crate
//! defines. The supervisor owns installing the controls and building the report (§56.5); it does
//! not own their names, in the same way it does not own [`Capability`](crate::Capability) or
//! [`KuangErrorCode`](crate::KuangErrorCode).
//!
//! [`ExecutionTier`] is the named tier of §17.2. It exists so that stronger isolation can arrive
//! later without a boolean having to mean two different things, and so that no code has to infer
//! "is this thing sandboxed" from a flag that cannot say which boundary it means (§17.3,
//! ADR-0448).

use std::fmt;

/// How a tier treats one control (v0.4.1 §16.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Requirement {
    /// The tier claims the control, so §2.3 applies: failing to install it prevents the
    /// operation it protects from starting. For a pre-exec control, the plugin never execs.
    Mandatory,
    /// The tier attempts the control and continues without it. The failure is still observable —
    /// §16.4 requires that much — as a `failed` row in the confinement report.
    BestEffort,
    /// The tier does not install the control at all. Present as an answer rather than as a
    /// silence, so that nobody infers the boundary from the controls that *are* installed
    /// (Appendix D's closing sentence).
    NotProvided,
}

impl Requirement {
    /// The word the registry, the report and the operator see.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Requirement::Mandatory => "mandatory",
            Requirement::BestEffort => "best_effort",
            Requirement::NotProvided => "not_provided",
        }
    }

    /// Whether a spawn may proceed when this control could not be installed.
    #[must_use]
    pub const fn is_mandatory(self) -> bool {
        matches!(self, Requirement::Mandatory)
    }
}

impl fmt::Display for Requirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What happens when a control cannot be installed — Appendix D's `Failure` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FailureBehaviour {
    /// The spawn is abandoned before `exec`, and the caller receives a structured error naming
    /// the control (§16.3).
    SpawnFails,
    /// The instance is already running: it is killed and quarantined (§18.2).
    Quarantine,
    /// The host call the plugin made is refused; the instance keeps running.
    RefuseBrokeredOperation,
    /// Nothing is refused. The failure is recorded in the confinement report and the audit trail.
    Recorded,
    /// The control is not installed, so it has no failure mode.
    None,
}

impl FailureBehaviour {
    /// The word the registry and the report use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            FailureBehaviour::SpawnFails => "spawn_fails",
            FailureBehaviour::Quarantine => "quarantine",
            FailureBehaviour::RefuseBrokeredOperation => "refuse_brokered_operation",
            FailureBehaviour::Recorded => "recorded",
            FailureBehaviour::None => "none",
        }
    }
}

impl fmt::Display for FailureBehaviour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

macro_rules! controls {
    ($( $variant:ident => $id:literal, $summary:literal; )*) => {
        /// One confinement control a tier may install (v0.4.1 §16.1, Appendix D).
        ///
        /// ```
        /// use ono_kuang_protocol::Control;
        /// assert_eq!(Control::NoNewPrivs.id(), "no_new_privs");
        /// assert_eq!(Control::from_id("no_new_privs"), Some(Control::NoNewPrivs));
        /// assert_eq!(Control::from_id("nothing_of_the_sort"), None);
        /// ```
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[non_exhaustive]
        pub enum Control {
            $( #[doc = $summary] $variant, )*
        }

        impl Control {
            /// Every control, in the order the registry declares them.
            pub const ALL: &'static [Control] = &[ $( Control::$variant, )* ];

            /// The id the registry, the report and the `plugin.*_failed` errors carry.
            #[must_use]
            pub const fn id(self) -> &'static str {
                match self { $( Control::$variant => $id, )* }
            }

            /// The one-line description an operator reads beside the id.
            #[must_use]
            pub const fn summary(self) -> &'static str {
                match self { $( Control::$variant => $summary, )* }
            }
        }
    };
}

controls! {
    CapabilityBroker => "capability_broker",
        "Every host call is resolved against the instance's grants before it is served.";
    ProtocolLimits => "protocol_limits",
        "Frame size, depth and credit ceilings on everything the instance sends.";
    FdHygiene => "fd_hygiene",
        "The child inherits no descriptor the host did not hand it.";
    ProtocolStdio => "protocol_stdio",
        "stdin and stdout are the protocol pipes and stderr is closed.";
    SessionSeparation => "session_separation",
        "`setsid`: the instance leads its own session and cannot signal the shell's process group.";
    NoNewPrivs => "no_new_privs",
        "`PR_SET_NO_NEW_PRIVS`: a setuid binary the instance execs gains it nothing.";
    RlimitData => "rlimit_data",
        "`RLIMIT_DATA` at the negotiated memory ceiling.";
    RlimitAddressSpace => "rlimit_address_space",
        "`RLIMIT_AS`. The native tier bounds allocated memory instead (ADR-0283).";
    RlimitCpu => "rlimit_cpu",
        "`RLIMIT_CPU`. The native tier expresses CPU as a scheduling class instead.";
    RlimitOpenFiles => "rlimit_open_files",
        "`RLIMIT_NOFILE`: the instance cannot exhaust the shell's descriptor table.";
    RlimitProcesses => "rlimit_processes",
        "`RLIMIT_NPROC`. The native tier does not configure it: it counts the whole user (ADR-0283).";
    RlimitFileSize => "rlimit_file_size",
        "`RLIMIT_FSIZE`: no single file the instance writes can fill the disk unnoticed.";
    RlimitCore => "rlimit_core",
        "`RLIMIT_CORE` at zero: the address space is never written to a file nobody asked for.";
    SchedulingPriority => "scheduling_priority",
        "`setpriority` at the nice level the declared `cpu_budget` becomes.";
    EnvironmentSanitization => "environment_sanitization",
        "The environment is built from nothing; the shell's variables are not a side channel.";
    WorkingDirectory => "working_directory",
        "The instance starts in its own private directory, never in the user's current one.";
    ProcessLifetime => "process_lifetime",
        "The supervisor owns the child, so no instance outlives the shell that started it.";
    FilesystemIsolation => "filesystem_isolation",
        "Kernel policy over filesystem access. The native tier does not provide it (§15.1).";
    NetworkIsolation => "network_isolation",
        "Kernel policy over network access. The native tier does not provide it (§15.1).";
    SeccompAllowlist => "seccomp_allowlist",
        "A seccomp syscall allowlist. Not required by v0.4.1 (§17.1).";
    LandlockAllowlist => "landlock_allowlist",
        "A Landlock path allowlist. Not required by v0.4.1 (§17.1).";
}

impl Control {
    /// Resolves a control from its registry id, or `None` when nothing carries that name.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|control| control.id() == id)
    }

    /// The error code a mandatory failure of this control is reported as (§16.3).
    ///
    /// §16.3 gives three: `plugin.no_new_privs_failed` for the privilege-transition control,
    /// `plugin.resource_limit_failed` for the resource ceilings, and `plugin.confinement_failed`
    /// for everything else. The distinction is the one an operator acts on — a refused rlimit is
    /// usually a configuration problem, a refused `PR_SET_NO_NEW_PRIVS` is a kernel one.
    #[must_use]
    pub const fn failure_code(self) -> crate::KuangErrorCode {
        use crate::KuangErrorCode as Code;
        match self {
            Control::NoNewPrivs => Code::PluginNoNewPrivsFailed,
            Control::RlimitData
            | Control::RlimitAddressSpace
            | Control::RlimitCpu
            | Control::RlimitOpenFiles
            | Control::RlimitProcesses
            | Control::RlimitFileSize
            | Control::RlimitCore => Code::PluginResourceLimitFailed,
            _ => Code::PluginConfinementFailed,
        }
    }
}

impl fmt::Display for Control {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// The named execution tier a plugin instance runs in (v0.4.1 §17.2).
///
/// A name rather than a boolean, because `sandboxed: true` cannot distinguish a process under
/// rlimits from one behind a Landlock allowlist, and §17.3 forbids calling either "sandboxed"
/// without saying which boundary is meant. The tier is what reaches audit, diagnostics and
/// documentation (ADR-0448).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ExecutionTier {
    /// A process of the Ono user under capability mediation and process confinement, with no
    /// kernel isolation of the filesystem or the network. The tier v0.4.1 ships.
    NativeConfined,
    /// `native-confined` plus kernel policy over the filesystem and the network. Named here so
    /// the model can express it; not implemented, and [`ExecutionTier::is_available`] says so
    /// (§17.1, §17.3).
    NativeIsolated,
    /// The capability-limited component tier of spec §31.10. Not implemented in this build.
    Wasm,
}

impl ExecutionTier {
    /// Every tier the model names.
    pub const ALL: &'static [ExecutionTier] = &[
        ExecutionTier::NativeConfined,
        ExecutionTier::NativeIsolated,
        ExecutionTier::Wasm,
    ];

    /// The id the registry, the audit trail and `inspect plugin` carry.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            ExecutionTier::NativeConfined => "native-confined",
            ExecutionTier::NativeIsolated => "native-isolated",
            ExecutionTier::Wasm => "wasm",
        }
    }

    /// Resolves a tier from its id.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|tier| tier.id() == id)
    }

    /// Whether this build can actually run a plugin in this tier.
    ///
    /// §17.3: a tier that is only a name must not be offered, because offering it is describing
    /// isolation the implementation does not have.
    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, ExecutionTier::NativeConfined)
    }

    /// The sentence §15.2 requires wherever this tier is described.
    #[must_use]
    pub const fn boundary(self) -> &'static str {
        match self {
            ExecutionTier::NativeConfined => {
                "A native KUANG/11 plugin executes as a process of the Ono user. Ono limits its \
                 brokered capabilities and applies process confinement, but native execution in \
                 v0.4.1 is not a complete filesystem or network sandbox. Install native plugins \
                 only from sources you are willing to run as your user account."
            }
            ExecutionTier::NativeIsolated => {
                "Not implemented. §17.1 does not require Landlock, seccomp or namespaces for \
                 v0.4.1, and §17.3 forbids describing an isolation boundary that does not exist."
            }
            ExecutionTier::Wasm => {
                "Not implemented in this build. A package declaring `wasm-component` is refused \
                 at load rather than served by the native path."
            }
        }
    }

    /// How this tier treats `control` (§16.4).
    #[must_use]
    pub const fn requirement(self, control: Control) -> Requirement {
        match self {
            ExecutionTier::NativeConfined => native_confined_requirement(control),
            // A tier that installs nothing claims nothing. §17.3 is the reason this is not a
            // guess at what the tier will one day require.
            ExecutionTier::NativeIsolated | ExecutionTier::Wasm => Requirement::NotProvided,
        }
    }

    /// What happens when `control` cannot be installed in this tier (Appendix D).
    #[must_use]
    pub const fn failure(self, control: Control) -> FailureBehaviour {
        match self.requirement(control) {
            Requirement::Mandatory => match control {
                Control::CapabilityBroker => FailureBehaviour::RefuseBrokeredOperation,
                Control::ProtocolLimits => FailureBehaviour::Quarantine,
                _ => FailureBehaviour::SpawnFails,
            },
            Requirement::BestEffort => FailureBehaviour::Recorded,
            Requirement::NotProvided => FailureBehaviour::None,
        }
    }

    /// Every control this tier claims, mandatory or best-effort, in registry order.
    ///
    /// This is what a confinement report has one row per (§16.5): the controls the tier said it
    /// would install. The `not_provided` rows are deliberately not here — they are a statement
    /// about the tier, not an outcome of a spawn.
    pub fn claimed_controls(self) -> impl Iterator<Item = Control> {
        Control::ALL
            .iter()
            .copied()
            .filter(move |&control| self.requirement(control) != Requirement::NotProvided)
    }

    /// Every control whose failure must prevent the spawn (§2.3, §16.3).
    pub fn mandatory_controls(self) -> impl Iterator<Item = Control> {
        Control::ALL
            .iter()
            .copied()
            .filter(move |&control| self.requirement(control).is_mandatory())
    }
}

/// The `native-confined` column of the registry.
///
/// Kept as one exhaustive `match` so that adding a [`Control`] variant without deciding what this
/// tier does about it does not compile — the compiler is the first referee, `spec-check` the
/// second.
const fn native_confined_requirement(control: Control) -> Requirement {
    match control {
        Control::CapabilityBroker
        | Control::ProtocolLimits
        | Control::FdHygiene
        | Control::ProtocolStdio
        | Control::SessionSeparation
        | Control::NoNewPrivs
        | Control::RlimitData
        | Control::RlimitOpenFiles
        | Control::RlimitFileSize
        | Control::RlimitCore
        | Control::EnvironmentSanitization
        | Control::WorkingDirectory
        | Control::ProcessLifetime => Requirement::Mandatory,
        // §16.4: "nice/setpriority best_effort unless policy explicitly requires it". A refused
        // renice costs responsiveness, not containment.
        Control::SchedulingPriority => Requirement::BestEffort,
        // "mandatory when configured by tier" — this tier configures none of these three, and
        // the registry records why beside each of them.
        Control::RlimitAddressSpace | Control::RlimitCpu | Control::RlimitProcesses => {
            Requirement::NotProvided
        }
        // Appendix D's last four rows. Never inferred from the rows above.
        Control::FilesystemIsolation
        | Control::NetworkIsolation
        | Control::SeccompAllowlist
        | Control::LandlockAllowlist => Requirement::NotProvided,
    }
}

impl fmt::Display for ExecutionTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_never_call_a_tier_that_installs_nothing_available() {
        // §17.3: a name is not an isolation boundary. Offering `native-isolated` would be
        // describing kernel policy this build does not install.
        assert!(ExecutionTier::NativeConfined.is_available());
        assert!(!ExecutionTier::NativeIsolated.is_available());
        assert!(!ExecutionTier::Wasm.is_available());
    }

    #[test]
    fn should_not_provide_filesystem_or_network_isolation_in_the_native_tier() {
        // Appendix D: "The UI/documentation MUST never infer the last four rows from the first
        // rows." The answer is an explicit `not_provided`, not an absence.
        for control in [
            Control::FilesystemIsolation,
            Control::NetworkIsolation,
            Control::SeccompAllowlist,
            Control::LandlockAllowlist,
        ] {
            assert_eq!(
                ExecutionTier::NativeConfined.requirement(control),
                Requirement::NotProvided
            );
        }
    }

    #[test]
    fn should_name_the_specific_error_for_a_privilege_or_resource_control() {
        use crate::KuangErrorCode;

        // §16.3's three-code family: the distinction is what an operator acts on.
        assert_eq!(
            Control::NoNewPrivs.failure_code(),
            KuangErrorCode::PluginNoNewPrivsFailed
        );
        assert_eq!(
            Control::RlimitData.failure_code(),
            KuangErrorCode::PluginResourceLimitFailed
        );
        assert_eq!(
            Control::SessionSeparation.failure_code(),
            KuangErrorCode::PluginConfinementFailed
        );
    }
}
