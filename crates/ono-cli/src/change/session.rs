//! The state every change command shares: the plan store, the recovery providers, the resolved
//! §53 settings and the protection policy they compose to (spec v0.6 §36, §12.1, §17, §53).
//!
//! §50.1 keeps providers out of the core and §55.7 keeps the engines out of `ono-cli`, so what
//! lives here is the fact that *one shell process has one plan store and one provider registry*.
//! Everything else — building a plan, analysing coverage, executing, recovering — belongs to the
//! `ono-change-*` crates and is reached from the commands.
//!
//! # Why a process-global rather than a field on the session
//!
//! Exactly the reason `crate::temporal::session` and `crate::spatial::session` are: a command is
//! handed an [`ono_command::Invocation`] and not the shell, so state a command must read has to
//! be reachable without one. A called script is another process and therefore another store
//! handle, which is what keeps §42.3's claim honest between two shells.
//!
//! # Degrading rather than panicking
//!
//! A poisoned lock answers the safe direction here as it does for the coordinate: the last plan
//! is forgotten rather than guessed at, and a registry that could not be built is an empty
//! registry whose refusals `discover` reports. §12.2 makes an unavailable provider an *answer*
//! rather than an error — a machine with no ZFS still plans and still protects with the file
//! provider — so registration never fails a command.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

use jiff::Timestamp;
use ono_change_core::{PlanId, error};
use ono_change_plan::{PlanStore, StoreOptions};
use ono_change_protection::{ChangeSettings, MountTable, ProtectionPolicy, ProviderRegistry};
use ono_value::{ErrorValue, Value};
use tokio::sync::{Mutex, MutexGuard};

/// The directory the file recovery provider archives into, beside the plan store (§15.3).
const RECOVERY_DIRECTORY: &str = "recovery";

/// Everything the thirteen commands of §5 read that the library cannot reach.
#[derive(Debug)]
pub struct ChangeState {
    store: PlanStore,
    providers: ProviderRegistry,
    settings: ChangeSettings,
    mounts: MountTable,
    session: Arc<str>,
}

impl ChangeState {
    /// The plan store this shell reads and writes (§36.1).
    #[must_use]
    pub const fn store(&self) -> &PlanStore {
        &self.store
    }

    /// The recovery providers this shell may ask (§12.1).
    #[must_use]
    pub const fn providers(&self) -> &ProviderRegistry {
        &self.providers
    }

    /// The resolved §53 configuration.
    #[must_use]
    pub const fn settings(&self) -> &ChangeSettings {
        &self.settings
    }

    /// The mount table persistence resolution reads (Appendix B).
    #[must_use]
    pub const fn mounts(&self) -> &MountTable {
        &self.mounts
    }

    /// The identity §36.1 attributes a plan to, shared with the temporal ledger (§29.2).
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session
    }

    /// The protection policy a plan runs under, given what `--protection` asked for (§17.3).
    #[must_use]
    pub fn policy(&self, requested: Option<ono_change_core::ProtectionMode>) -> ProtectionPolicy {
        self.settings.policy_for(requested)
    }
}

/// The one change state of this process, built on first use (§36.1).
///
/// Asynchronous because every command that holds it is already inside the runtime, and because a
/// blocking guard taken there is a deadlock waiting for a reason. It is `None` until the first
/// command opens the store successfully: §32.1 budgets a shell that never plans at no filesystem
/// cost, and opening a SQLite database on startup would spend it.
fn cell() -> &'static Mutex<Option<Arc<ChangeState>>> {
    static STATE: OnceLock<Mutex<Option<Arc<ChangeState>>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

/// Borrows this process's change state, opening the store on first use.
///
/// # Errors
///
/// `change.plan_store_unavailable` where the store cannot be created or opened, and
/// `change.plan_store_corrupt` where SQLite reports damage (§36.2).
pub async fn change_session() -> Result<Arc<ChangeState>, ErrorValue> {
    let mut held: MutexGuard<'_, Option<Arc<ChangeState>>> = cell().lock().await;
    if let Some(state) = held.as_ref() {
        return Ok(Arc::clone(state));
    }
    let built = Arc::new(build(Timestamp::now())?);
    *held = Some(Arc::clone(&built));
    Ok(built)
}

/// Opens the store and registers the three first-party recovery providers (§12.1, §15.3).
fn build(now: Timestamp) -> Result<ChangeState, ErrorValue> {
    let settings = configured();
    let directory = store_directory()?;
    let store = PlanStore::open_with(&StoreOptions::at(
        &directory.join(ono_change_plan::DATABASE_NAME),
    ))?;
    let mounts = MountTable::from_proc().unwrap_or_else(|_| MountTable::from_text(""));
    Ok(ChangeState {
        providers: registry(&settings, &directory, now),
        store,
        settings,
        mounts,
        session: Arc::from(session_identity()),
    })
}

/// The directory §36.2 puts the store in, resolved the way every other XDG path in the tree is.
///
/// `XDG_DATA_HOME` is therefore the override a test needs, and the one the rest of the shell
/// already honours: a suite that points it at a scratch directory neither reads nor writes the
/// developer's own plans.
///
/// # Errors
///
/// `change.plan_store_unavailable` when neither `XDG_DATA_HOME` nor `HOME` is set, because then
/// there is nowhere to keep a plan.
pub fn store_directory() -> Result<PathBuf, ErrorValue> {
    ono_change_plan::plan_store_directory(
        |name| std::env::var(name).ok(),
        std::env::var_os("HOME").map(PathBuf::from).as_deref(),
    )
    .ok_or_else(|| {
        error::store_unavailable(
            "no home directory and no XDG data directory, so there is nowhere to keep a plan",
        )
    })
}

/// The registry of §12.1's recovery providers, with every first-party one registered.
///
/// Registering a provider that cannot run here is deliberate and is not an error: §12.2 makes
/// [`ono_change_core::ProviderAvailability`] the answer, and `discover` turns it into a
/// [`ono_change_protection::ProviderRefusal`] the coverage matrix shows. A machine with no ZFS
/// still plans, still protects with the file provider, and still says why the ZFS provider was
/// not asked — which is what §55.6 case 29 requires and what a silent gap would destroy.
///
/// A provider the configuration switches off is a different case and is left unregistered:
/// `recovery.zfs.enabled = false` is an operator saying "do not ask", and a refusal claiming the
/// tool was missing would be untrue.
fn registry(
    settings: &ChangeSettings,
    directory: &std::path::Path,
    now: Timestamp,
) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    if let Ok(store) =
        ono_recovery_files::FileRecoveryStore::open(directory.join(RECOVERY_DIRECTORY))
    {
        let provider = ono_recovery_files::FileRecoveryProvider::new(store, now)
            .with_retention(settings.retention_policy())
            // A session outlives its first plan: each asset is dated, named and expired by when
            // it was made, so two recovery points of one file are two assets (§11.1, §37.1).
            .with_clock(Timestamp::now);
        let _ = registry.register(Arc::new(provider));
    }
    if settings.zfs_enabled() {
        let runner = Arc::new(ono_recovery_zfs::ProcessRunner::default());
        let provider = ono_recovery_zfs::ZfsProvider::new(runner)
            .with_minimum_free(zfs_floor(settings.min_filesystem_free()))
            .with_recovery_policy(
                settings.zfs_prefer_selective_restore(),
                settings.zfs_allow_destructive_rollback(),
            );
        let _ = registry.register(Arc::new(provider));
    }
    if settings.btrfs_enabled() {
        let runner = Arc::new(ono_recovery_btrfs::ProcessRunner::default());
        let provider = ono_recovery_btrfs::BtrfsProvider::new(runner).with_config(
            ono_recovery_btrfs::config::BtrfsConfig::default()
                .preferring_read_only(
                    settings.prefer_read_only_snapshots()
                        && settings.btrfs_prefer_read_only_snapshots(),
                )
                .recovering_root_by(btrfs_root_recovery(settings.btrfs_root_recovery())),
        );
        let _ = registry.register(Arc::new(provider));
    }
    registry
}

/// The identity §36.1 attributes a plan to.
///
/// The temporal session's, so §29.2's two histories agree about who did what: a plan created in
/// this shell and the events the ledger holds for it carry one session word.
fn session_identity() -> String {
    static IDENTITY: OnceLock<String> = OnceLock::new();
    IDENTITY
        .get_or_init(|| {
            format!(
                "s{:x}{:x}",
                std::process::id(),
                Timestamp::now().as_nanosecond().unsigned_abs()
            )
        })
        .clone()
}

/// The resolved §53 settings, published where a command can read them without the shell.
fn published_settings() -> &'static RwLock<ChangeSettings> {
    static SETTINGS: OnceLock<RwLock<ChangeSettings>> = OnceLock::new();
    SETTINGS.get_or_init(|| RwLock::new(ChangeSettings::defaults()))
}

/// The §53 settings this shell resolved, or §53's own defaults where the lock is poisoned.
///
/// Degrading to the defaults is the safe direction: `change.allow_opaque_actions` is `false`
/// there and `change.default_protection` is `prefer`, so a lock nobody can read cannot widen what
/// a plan is permitted to do.
#[must_use]
pub fn configured() -> ChangeSettings {
    published_settings()
        .read()
        .map_or_else(|_| ChangeSettings::defaults(), |held| held.clone())
}

/// Reads the shell's resolved configuration into the change layer (§53).
///
/// Called once, from `crate::eval::native::implementations`, for the same reason
/// `crate::temporal::configure_from` is: that is the only point where the shell's settings and
/// the process-wide state a command reaches without an invocation meet.
///
/// A key the settings catalogue does not declare is read from its mechanical `ONO_*` spelling
/// (ADR-0010) instead, so §53's seventeen keys are settable before the catalogue carries them.
/// The catalogue wins wherever it answers, and a catalogued key still at its built-in default is
/// passed on as unset (ADR-0834).
pub fn configure_from(settings: &crate::settings::Settings) -> Vec<ErrorValue> {
    let lookup = |key: &str| -> Option<Value> {
        // A key at its built-in default was not written, and the change layer reads it as unset:
        // ADR-0834 lets a plan lower the built-in protection mode and never one an operator
        // configured, so "written" has to survive into the reader.
        match settings.effective(key) {
            Some(resolved) if resolved.layer == crate::settings::Layer::Default => return None,
            Some(resolved) => return Some(resolved.value.clone()),
            None => {}
        }
        let variable = format!("ONO_{}", key.to_ascii_uppercase().replace('.', "_"));
        std::env::var(variable)
            .ok()
            .filter(|text| !text.is_empty())
            .map(|text| typed(&text))
    };
    // §53's closing sentence asks for a configuration nobody can read to be reported rather than
    // silently ignored. Each unreadable key keeps its default and comes back as a problem for
    // `get config --problems`; every key that could be read keeps the operator's value.
    let (resolved, problems) = ChangeSettings::read(&lookup);
    if let Ok(mut held) = published_settings().write() {
        *held = resolved;
    }
    problems
}

/// An environment variable as the value the setting's declared type wants.
///
/// The settings catalogue types a key and the environment does not, so `ONO_CHANGE_ALLOW_OPAQUE_
/// ACTIONS=true` has to arrive as a boolean rather than as the word "true" — `ChangeSettings`
/// refuses a value of the wrong shape (§53), and refusing the operator's own configuration for
/// the way an environment stores it would be a rule nobody could satisfy.
fn typed(text: &str) -> Value {
    if let Ok(flag) = text.parse::<bool>() {
        return Value::Bool(flag);
    }
    if let Ok(count) = text.parse::<i128>() {
        return Value::Int(count);
    }
    Value::string(text)
}

/// The plan `@` names: the last one this process produced (§36.4, ADR-0803).
fn published_plan() -> &'static RwLock<Option<PlanId>> {
    static LAST: OnceLock<RwLock<Option<PlanId>>> = OnceLock::new();
    LAST.get_or_init(|| RwLock::new(None))
}

/// Records `plan` as the one `@` now names.
pub fn note_last_plan(plan: &PlanId) {
    if let Ok(mut held) = published_plan().write() {
        *held = Some(plan.clone());
    }
}

/// The plan `@` names, or `None` where this shell has produced none.
#[must_use]
pub fn last_plan() -> Option<PlanId> {
    published_plan().read().ok().and_then(|held| held.clone())
}

/// §53's `recovery.min_filesystem_free`, as the ZFS provider enforces it against a pool
/// (Appendix D.3).
///
/// Every part of the floor carries over: a share and a quantity together stay together, because
/// keeping only one of them would quietly lower the floor the operator configured.
fn zfs_floor(
    floor: ono_change_protection::policy::FreeSpaceFloor,
) -> ono_recovery_zfs::FreeSpaceFloor {
    use ono_change_protection::policy::FreeSpaceFloor as Configured;
    use ono_recovery_zfs::FreeSpaceFloor as Enforced;
    match floor {
        Configured::Share(share) => Enforced::Share(share),
        Configured::Absolute(bytes) => Enforced::Bytes(bytes),
        Configured::Both { share, absolute } => Enforced::Both {
            share,
            bytes: absolute,
        },
    }
}

/// §53's `recovery.btrfs.root_recovery`, as the Btrfs provider plans a root recovery (§14.6).
const fn btrfs_root_recovery(
    configured: ono_change_protection::settings::RootRecovery,
) -> ono_recovery_btrfs::config::RootRecovery {
    use ono_change_protection::settings::RootRecovery as Configured;
    use ono_recovery_btrfs::config::RootRecovery as Planned;
    match configured {
        Configured::OnlineSelectiveRestore => Planned::OnlineSelectiveRestore,
        Configured::OfflineSubvolumeReplacement => Planned::OfflineSubvolumeReplacement,
        Configured::NextBoot => Planned::NextBoot,
    }
}

/// The authority this process actually holds (§43.2, §43.3).
///
/// Read from `/proc/self/status`: the effective uid (`Uid:`'s second field) and the effective
/// capability set (`CapEff:`). A set that cannot be read is left unknown, and
/// [`Authority::for_session`](ono_change_executor::Authority::for_session) then decides by uid —
/// never by assuming the session may do everything.
#[must_use]
pub fn authority() -> ono_change_executor::Authority {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .map(str::trim)
    };
    let uid = field("Uid:")
        .and_then(|values| values.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u32>().ok())
        // A uid nobody could read is not root: the narrower answer is the safe one.
        .unwrap_or(u32::MAX);
    let capabilities = field("CapEff:").and_then(|value| u64::from_str_radix(value, 16).ok());
    ono_change_executor::Authority::for_session(uid, capabilities)
}
