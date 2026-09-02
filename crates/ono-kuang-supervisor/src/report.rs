//! What a spawn can say about the confinement it actually got (v0.4.1 §16.5, §2.6).
//!
//! §16.5 asks the supervisor to build a report for every spawn, with five columns —
//! `control`, `required`, `attempted`, `result`, `platform_detail` — and one binding invariant:
//!
//! > A successful plugin spawn MUST imply every `required=true` control has `result=applied`.
//!
//! [`ConfinementReport::is_confined`] is that invariant, and `sandbox::spawn` refuses to return a
//! child for which it does not hold. The report is therefore not a diagnostic *about* the
//! decision; it is the decision, written down.
//!
//! # Reading an outcome out of a process that no longer exists
//!
//! Most of the controls are installed between `fork` and `exec`, in a child that either execs the
//! artifact or dies. Whether `setsid` succeeded is a fact only that child ever knew, and the
//! standard library's `pre_exec` failure path carries exactly one integer back to the parent —
//! the `errno` — which cannot say *which* control produced it, and says nothing at all about the
//! controls that succeeded or about a best-effort one that failed without stopping the spawn.
//!
//! So the parent gives the child somewhere to write. [`Outcomes`] is one page of `MAP_SHARED`
//! anonymous memory, mapped before the fork and therefore the same physical page in both
//! processes, holding one `AtomicU64` per [`Control`]. The child stores an outcome as it installs
//! each control — a single relaxed store, which allocates nothing and takes no lock, so it is
//! legal in a `pre_exec` context — and the parent reads the page afterwards whether the spawn
//! succeeded or failed. §2.6 is why this exists rather than an inference: *"If Ono cannot
//! determine whether a plugin control was installed … it MUST report an explicit unknown/refusal
//! state rather than claim success."* A control the child never reached reads back as
//! [`ControlResult::NotAttempted`], not as applied (ADR-0445).

use std::sync::atomic::{AtomicU64, Ordering};

use ono_kuang_protocol::{Control, ExecutionTier, KuangError, Requirement};

/// What became of one control (v0.4.1 §16.5's `result`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ControlResult {
    /// The control is in force.
    Applied,
    /// The control was attempted and the platform refused it.
    Failed,
    /// The tier does not claim this control, so nothing was attempted and nothing is in force.
    Skipped,
    /// The control was never reached, because an earlier mandatory control failed first. Never
    /// reported as applied: §2.6 forbids inventing the certainty (ADR-0445).
    NotAttempted,
}

impl ControlResult {
    /// The word the report and the operator see.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ControlResult::Applied => "applied",
            ControlResult::Failed => "failed",
            ControlResult::Skipped => "skipped",
            ControlResult::NotAttempted => "not_attempted",
        }
    }

    /// Whether the control was tried at all (§16.5's `attempted`).
    #[must_use]
    pub const fn attempted(self) -> bool {
        matches!(self, ControlResult::Applied | ControlResult::Failed)
    }

    fn from_code(code: u8) -> Self {
        match code {
            1 => ControlResult::Applied,
            2 => ControlResult::Failed,
            3 => ControlResult::Skipped,
            _ => ControlResult::NotAttempted,
        }
    }

    const fn code(self) -> u8 {
        match self {
            ControlResult::NotAttempted => 0,
            ControlResult::Applied => 1,
            ControlResult::Failed => 2,
            ControlResult::Skipped => 3,
        }
    }
}

impl std::fmt::Display for ControlResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One row of the report (v0.4.1 §16.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfinementEntry {
    control: Control,
    requirement: Requirement,
    result: ControlResult,
    detail: Option<String>,
}

impl ConfinementEntry {
    /// The control this row is about.
    #[must_use]
    pub const fn control(&self) -> Control {
        self.control
    }

    /// How the tier treats it (§16.4).
    #[must_use]
    pub const fn requirement(&self) -> Requirement {
        self.requirement
    }

    /// Whether a successful spawn implies this control is in force (§16.5's `required`).
    #[must_use]
    pub const fn required(&self) -> bool {
        self.requirement.is_mandatory()
    }

    /// Whether the control was tried (§16.5's `attempted`).
    #[must_use]
    pub const fn attempted(&self) -> bool {
        self.result.attempted()
    }

    /// What became of it (§16.5's `result`).
    #[must_use]
    pub const fn result(&self) -> ControlResult {
        self.result
    }

    /// The platform's own account of a refusal (§16.5's `platform_detail`).
    ///
    /// The operating system's error text and nothing else. §16.5: the report "MUST not expose
    /// secrets", and an `errno` is a fact about the kernel rather than about the user.
    #[must_use]
    pub fn platform_detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

/// The confinement one spawn actually got, control by control (v0.4.1 §16.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfinementReport {
    tier: ExecutionTier,
    entries: Vec<ConfinementEntry>,
}

impl ConfinementReport {
    /// The named tier the spawn ran in (§17.2).
    #[must_use]
    pub const fn tier(&self) -> ExecutionTier {
        self.tier
    }

    /// Every row, in the order the central table declares the controls.
    #[must_use]
    pub fn entries(&self) -> &[ConfinementEntry] {
        &self.entries
    }

    /// The row for one control, if the tier claims it.
    #[must_use]
    pub fn entry(&self, control: Control) -> Option<&ConfinementEntry> {
        self.entries.iter().find(|entry| entry.control == control)
    }

    /// Every control the platform refused, required or not.
    ///
    /// §16.4: "A best-effort failure MUST still be observable in diagnostics but does not prevent
    /// spawn." This is where it is observable.
    pub fn failed(&self) -> impl Iterator<Item = &ConfinementEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.result == ControlResult::Failed)
    }

    /// The required control that is not in force, if there is one.
    ///
    /// A refusal cascades: the control the platform refused stops the sequence, and every
    /// mandatory control after it reads `not_attempted`. The cause is the one that was refused,
    /// so a `failed` row outranks a `not_attempted` one whatever order they are stored in —
    /// otherwise the operator is told about the consequence and left to find the reason (§54.1).
    #[must_use]
    pub fn unmet(&self) -> Option<&ConfinementEntry> {
        self.entries
            .iter()
            .find(|entry| entry.required() && entry.result == ControlResult::Failed)
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|entry| entry.required() && entry.result != ControlResult::Applied)
            })
    }

    /// Whether §16.5's invariant holds: every required control has `result = applied`.
    ///
    /// A spawn that does not satisfy this never returns a child (§2.3).
    #[must_use]
    pub fn is_confined(&self) -> bool {
        self.unmet().is_none()
    }

    /// The structured error a report that is not confined stands for (§16.3).
    ///
    /// `None` when the report is confined. Otherwise the code of §16.3's family that matches the
    /// control, with the control id and the platform's own reason in the metadata, so a script
    /// branches on the code and an operator reads the row.
    #[must_use]
    pub fn refusal(&self, package: &str) -> Option<KuangError> {
        let unmet = self.unmet()?;
        let detail = unmet.platform_detail().unwrap_or(match unmet.result {
            ControlResult::Skipped => "this platform does not install it",
            ControlResult::NotAttempted => "an earlier mandatory control failed first",
            ControlResult::Applied | ControlResult::Failed => "the platform refused it",
        });
        let error = KuangError::new(
            unmet.control.failure_code(),
            format!(
                "{package} was not started because {} could not be installed: {detail}",
                unmet.control.id()
            ),
        )
        .with_help(format!(
            "required control: {}\nexecution tier: {}\n{}",
            unmet.control.id(),
            self.tier.id(),
            unmet.control.summary()
        ))
        .with_metadata(
            "control",
            serde_json::Value::String(unmet.control.id().to_owned()),
        )
        .with_metadata(
            "execution_tier",
            serde_json::Value::String(self.tier.id().to_owned()),
        )
        .with_metadata(
            "result",
            serde_json::Value::String(unmet.result.as_str().to_owned()),
        )
        .with_metadata(
            "platform_detail",
            serde_json::Value::String(detail.to_owned()),
        );
        Some(error)
    }
}

/// One page of memory the forked child and its parent share, holding one outcome per control.
///
/// `MAP_SHARED | MAP_ANONYMOUS` survives the fork as the same physical page, which is what lets a
/// child that is about to `exec` — or about to die — leave a record the parent can read. The
/// child's side is one relaxed atomic store per control: no allocation, no lock, legal between
/// `fork` and `exec`.
pub(crate) struct Outcomes {
    page: *mut AtomicU64,
    slots: usize,
}

// SAFETY: the page is an owned anonymous mapping this type allocates and unmaps, and every access
// to it goes through an `AtomicU64`. Sharing the handle across threads is sharing a pointer to
// atomics, which is exactly what `AtomicU64` is for; the cross-*process* sharing is the same
// physical memory and the same atomic operations.
#[allow(
    unsafe_code,
    reason = "a page shared with a forked child is how a pre-exec outcome reaches the parent \
              (v0.4.1 §16.5, ADR-0445)"
)]
unsafe impl Send for Outcomes {}
#[allow(
    unsafe_code,
    reason = "see the Send impl above: every access goes through an AtomicU64 (ADR-0445)"
)]
unsafe impl Sync for Outcomes {}

impl Outcomes {
    /// Maps a page the child can write and the parent can read.
    #[cfg(unix)]
    #[allow(
        unsafe_code,
        reason = "`mmap` has no safe wrapper, and shared anonymous memory is the only channel \
                  out of a `pre_exec` context that carries more than one errno (ADR-0445)"
    )]
    pub(crate) fn map() -> std::io::Result<Self> {
        let slots = Control::ALL.len();
        let len = std::mem::size_of::<AtomicU64>() * slots;
        // SAFETY: an anonymous mapping with a null hint, so the kernel chooses the address and
        // nothing of the caller's is touched. The result is checked against `MAP_FAILED` before
        // it is used.
        let page = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if page == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self {
            page: page.cast::<AtomicU64>(),
            slots,
        })
    }

    #[allow(
        unsafe_code,
        reason = "indexing the mapping this type owns, bounds-checked against its own length"
    )]
    fn slot(&self, control: Control) -> Option<&AtomicU64> {
        let index = Control::ALL.iter().position(|&known| known == control)?;
        if index >= self.slots {
            return None;
        }
        // SAFETY: `index` is a position within `Control::ALL`, whose length is the number of
        // slots the mapping was made with, and the mapping is live for as long as `self` is.
        Some(unsafe { &*self.page.add(index) })
    }

    /// Records what became of `control`. Called in the child; allocates nothing.
    pub(crate) fn record(&self, control: Control, result: ControlResult, errno: i32) {
        if let Some(slot) = self.slot(control) {
            let packed = u64::from(result.code()) | (u64::from(errno.unsigned_abs()) << 8);
            slot.store(packed, Ordering::Relaxed);
        }
    }

    /// Reads what became of `control`. Called in the parent, after the spawn returned.
    pub(crate) fn read(&self, control: Control) -> (ControlResult, i32) {
        let Some(slot) = self.slot(control) else {
            return (ControlResult::NotAttempted, 0);
        };
        let packed = slot.load(Ordering::Relaxed);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            reason = "the errno was written as its own absolute value in bits 8..40"
        )]
        let errno = ((packed >> 8) & 0xFFFF_FFFF) as i32;
        (ControlResult::from_code((packed & 0xFF) as u8), errno)
    }
}

impl Drop for Outcomes {
    #[cfg(unix)]
    #[allow(
        unsafe_code,
        reason = "unmapping the page this type mapped, exactly once (ADR-0445)"
    )]
    fn drop(&mut self) {
        // SAFETY: the address and length are the ones `map` produced, and `Drop` runs once.
        let unmapped = unsafe {
            libc::munmap(
                self.page.cast::<libc::c_void>(),
                std::mem::size_of::<AtomicU64>() * self.slots,
            )
        };
        // Nothing a caller could do about it at drop time, and nothing to hide either: a refused
        // `munmap` of a mapping this type made itself is a defect in this file. The debug build
        // says so; the release build leaks a page rather than aborting a shell over it.
        debug_assert_eq!(
            unmapped,
            0,
            "the confinement outcome page must unmap: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// The rows for the controls the parent installs, before there is a child to have recorded
/// anything.
///
/// A working directory that could not be made is a mandatory failure that costs no fork at all,
/// and checking it here is what keeps §2.3's "prevent the operation from starting" literal.
pub(crate) fn parent_only(
    tier: ExecutionTier,
    parent: &[(Control, ControlResult, Option<String>)],
) -> ConfinementReport {
    let entries = parent
        .iter()
        .map(|(control, result, detail)| ConfinementEntry {
            control: *control,
            requirement: tier.requirement(*control),
            result: *result,
            detail: detail.clone(),
        })
        .collect();
    ConfinementReport { tier, entries }
}

/// Builds the report for a tier from what the child recorded and what the parent installed.
pub(crate) fn build(
    tier: ExecutionTier,
    outcomes: &Outcomes,
    parent: &[(Control, ControlResult, Option<String>)],
) -> ConfinementReport {
    let mut entries = Vec::new();
    for control in tier.claimed_controls() {
        if let Some((_, result, detail)) =
            parent.iter().find(|(recorded, _, _)| *recorded == control)
        {
            entries.push(ConfinementEntry {
                control,
                requirement: tier.requirement(control),
                result: *result,
                detail: detail.clone(),
            });
            continue;
        }
        let (result, errno) = outcomes.read(control);
        let detail = (result == ControlResult::Failed)
            .then(|| std::io::Error::from_raw_os_error(errno).to_string());
        entries.push(ConfinementEntry {
            control,
            requirement: tier.requirement(control),
            result,
            detail,
        });
    }
    ConfinementReport { tier, entries }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_carry_an_outcome_written_through_the_shared_page() {
        let outcomes = Outcomes::map().expect("one page of anonymous memory");
        outcomes.record(Control::NoNewPrivs, ControlResult::Failed, libc::EPERM);
        assert_eq!(
            outcomes.read(Control::NoNewPrivs),
            (ControlResult::Failed, libc::EPERM)
        );
        // A control nothing wrote is never an applied one (§2.6).
        assert_eq!(
            outcomes.read(Control::SessionSeparation).0,
            ControlResult::NotAttempted
        );
    }

    #[test]
    fn should_refuse_to_call_a_spawn_confined_when_a_required_control_was_not_applied() {
        let outcomes = Outcomes::map().expect("one page of anonymous memory");
        for control in ExecutionTier::NativeConfined.claimed_controls() {
            outcomes.record(control, ControlResult::Applied, 0);
        }
        let report = build(ExecutionTier::NativeConfined, &outcomes, &[]);
        assert!(report.is_confined());
        assert!(report.refusal("example").is_none());

        outcomes.record(Control::NoNewPrivs, ControlResult::Failed, libc::EPERM);
        let report = build(ExecutionTier::NativeConfined, &outcomes, &[]);
        assert!(
            !report.is_confined(),
            "§16.5's invariant is not a suggestion"
        );
        let refusal = report
            .refusal("example")
            .expect("a report that is not confined refuses");
        assert_eq!(
            refusal.code(),
            ono_kuang_protocol::KuangErrorCode::PluginNoNewPrivsFailed
        );
    }

    #[test]
    fn should_not_call_a_best_effort_refusal_a_reason_to_refuse_the_spawn() {
        // §16.4: observable, but not fatal.
        let outcomes = Outcomes::map().expect("one page of anonymous memory");
        for control in ExecutionTier::NativeConfined.claimed_controls() {
            outcomes.record(control, ControlResult::Applied, 0);
        }
        outcomes.record(
            Control::SchedulingPriority,
            ControlResult::Failed,
            libc::EACCES,
        );
        let report = build(ExecutionTier::NativeConfined, &outcomes, &[]);
        assert!(report.is_confined());
        assert_eq!(report.failed().count(), 1);
    }
}
