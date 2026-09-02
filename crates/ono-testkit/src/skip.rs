//! The third test outcome, and the reason that makes it readable.
//!
//! `cargo test` knows two outcomes. A test whose precondition this host cannot meet — no second
//! mount to cross, no `git` on `PATH`, running as root where the assertion is what a normal user
//! is refused — is neither of them: it returns early, the summary counts it as `ok`, and the
//! suite reports coverage it did not have. v0.4.1 §65.10 names that "skip-as-pass" and §38.1
//! requires the three outcomes `PASS`, `FAIL` and `SKIP(reason)` to be distinguishable.
//!
//! There is no third outcome to return, so the honesty is in the record: every skip prints the
//! same marker, naming the test, the §38.4 category and the detail, on the stream a test harness
//! shows.

use std::fmt;
use std::io::Write;

/// Why a test could not exercise its subject on this host — the stable categories of v0.4.1
/// §38.4.
///
/// A category is what the expected-skip registry compares against, so it is a closed set rather
/// than free text. The detail beside it is free text and is for the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SkipReason {
    /// The kernel does not offer the facility the test observes.
    MissingKernelFeature,
    /// The run does not hold — or holds too much of — the privilege the test needs.
    MissingPrivilege,
    /// The processor architecture is not one the test's subject applies to.
    UnsupportedArch,
    /// The distribution does not ship what the test observes.
    UnsupportedDistribution,
    /// A program the test drives is not installed.
    ExternalToolUnavailable,
    /// The host cannot present the situation the test is about.
    FixtureNotApplicable,
}

impl SkipReason {
    /// Every category, in the order v0.4.1 §38.4 lists them.
    pub const ALL: [SkipReason; 6] = [
        SkipReason::MissingKernelFeature,
        SkipReason::MissingPrivilege,
        SkipReason::UnsupportedArch,
        SkipReason::UnsupportedDistribution,
        SkipReason::ExternalToolUnavailable,
        SkipReason::FixtureNotApplicable,
    ];

    /// The category's stable token, as §38.4 spells it and as the registry stores it.
    #[must_use]
    pub const fn category(self) -> &'static str {
        match self {
            SkipReason::MissingKernelFeature => "missing_kernel_feature",
            SkipReason::MissingPrivilege => "missing_privilege",
            SkipReason::UnsupportedArch => "unsupported_arch",
            SkipReason::UnsupportedDistribution => "unsupported_distribution",
            SkipReason::ExternalToolUnavailable => "external_tool_unavailable",
            SkipReason::FixtureNotApplicable => "fixture_not_applicable",
        }
    }

    /// The category a token names, or `None` when the token is not one of the six.
    #[must_use]
    pub fn from_category(token: &str) -> Option<Self> {
        SkipReason::ALL
            .into_iter()
            .find(|reason| reason.category() == token)
    }
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.category())
    }
}

/// Whether the host could present what a test needs — the return value of [`require`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "a prerequisite that is not consulted is a test that skipped nothing"]
pub enum TestPrerequisite {
    /// The host can present it; the test carries on.
    Met,
    /// It could not, and the skip has been announced.
    Unmet,
}

impl TestPrerequisite {
    /// Whether the test may carry on.
    #[must_use]
    pub fn met(self) -> bool {
        self == TestPrerequisite::Met
    }

    /// Whether the test must return, having already announced its skip.
    #[must_use]
    pub fn unmet(self) -> bool {
        self == TestPrerequisite::Unmet
    }
}

/// Announces that a test could not exercise its subject on this host, in which §38.4 category,
/// and why.
///
/// The marker is one line on standard error, and its shape is the contract the expected-skip
/// registry of §38.2 reads:
///
/// ```text
/// SKIPPED should_cross_a_mount_boundary: fixture_not_applicable: this host reports no mount below `/`
/// ```
///
/// The test's name comes from the thread `cargo test` runs it on, so the marker cannot go stale
/// by being copied.
///
/// A skip is a last resort. Prefer arranging the precondition — spawning the child, binding the
/// listener, creating the file — over asking the host for it (ADR-0417).
pub fn skipped(reason: SkipReason, detail: &str) {
    // `cargo test` names each test's thread after the test, which makes the marker self-locating
    // without the caller repeating a name that could go stale.
    let test = std::thread::current()
        .name()
        .unwrap_or("<unnamed>")
        .to_owned();
    // Written to the real standard error rather than through `eprintln!`, because the test
    // harness captures the macros and prints them only for tests that *failed*. A skip belongs to
    // a test that passed, so through the macro the marker would exist and never be seen — and
    // §38.1 asks for the skip to be visible in the harness output, not merely emitted.
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "SKIPPED {test}: {}: {detail}", reason.category());
}

/// The prerequisite helper of v0.4.1 Appendix G: `require(condition, category, detail)`.
///
/// When `condition` holds the test carries on. When it does not, the skip is announced through
/// [`skipped`] before the caller returns, so the early return has already emitted the canonical
/// signal the gate recognises:
///
/// ```no_run
/// use ono_testkit::{SkipReason, require};
/// # fn has_a_second_mount() -> bool { true }
/// if require(
///     has_a_second_mount(),
///     SkipReason::FixtureNotApplicable,
///     "this host reports no mount below `/`",
/// )
/// .unmet()
/// {
///     return;
/// }
/// ```
pub fn require(condition: bool, reason: SkipReason, detail: &str) -> TestPrerequisite {
    if condition {
        TestPrerequisite::Met
    } else {
        skipped(reason, detail);
        TestPrerequisite::Unmet
    }
}

/// What a fixture needed from the host and what the host would give it.
///
/// A machine that cannot supply a hundred thousand file descriptors has not found a defect in the
/// product, and a red result meaning "this runner has a lower rlimit" is the inverse of
/// v0.4.1 §65.10's skip-as-pass: it is fail-as-defect, and §38 forbids both. A shortfall is
/// therefore a prerequisite the test reports with [`skipped`], never a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescriptorShortfall {
    /// What the fixture needed open at once, including its headroom.
    pub needed: u64,
    /// The soft limit after the fixture raised it as far as it may.
    pub soft: u64,
    /// The hard limit, which only a privileged process can raise.
    pub hard: u64,
}

impl fmt::Display for DescriptorShortfall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the fixture needs {} open file descriptors and this host allows {} (hard limit {}, \
             which only a privileged process can raise)",
            self.needed, self.soft, self.hard
        )
    }
}

/// Raises this process's soft descriptor limit as far as the hard limit allows, and reports what
/// it reached against `needed`.
///
/// Raising the soft limit changes nothing about what a fixture measures — the descriptors were
/// always allowed, the process was simply not asking for them — so it is tried first and without
/// ceremony. Only when the *hard* limit is too low has the host genuinely refused.
///
/// # Errors
///
/// Returns the shortfall when the host cannot supply `needed` descriptors.
pub fn require_descriptors(needed: u64) -> Result<(), DescriptorShortfall> {
    use nix::sys::resource::{Resource, getrlimit, setrlimit};

    let (soft, hard) = getrlimit(Resource::RLIMIT_NOFILE).unwrap_or((0, 0));
    let mut reached = soft;
    if soft < needed && hard > soft {
        let want = needed.min(hard);
        if setrlimit(Resource::RLIMIT_NOFILE, want, hard).is_ok() {
            reached = want;
        }
    }
    if reached >= needed {
        Ok(())
    } else {
        Err(DescriptorShortfall {
            needed,
            soft: reached,
            hard,
        })
    }
}
