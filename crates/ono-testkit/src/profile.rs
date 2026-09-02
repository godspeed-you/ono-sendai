//! Reference cardinality profiles, and the live populations that realise them.
//!
//! v0.4.1 §32.1 says why this file exists: *"v0.4.1 MUST stop treating one small fixture passing
//! a latency budget as sufficient proof that a spatial operation is performant."* §32.2 and
//! Appendix F then fix the numbers, and §32.2 fixes the one rule that makes a fixture worth
//! anything — *"provider/planner code exercised by the benchmark MUST match production logic"*.
//! A fixture that hands the planner a pre-built answer measures the fixture.
//!
//! So a population here is **real**: real child processes the operating system lists in `/proc`,
//! which the production process provider reads the same way it reads any other. Nothing is
//! injected, and no provider is replaced.

use std::process::{Child, Command, Stdio};

/// A reference cardinality profile (v0.4.1 §32.2, Appendix F).
///
/// The four numbers are one system's size, not four independent knobs: Profile M is *a machine
/// with a thousand processes*, whose graph therefore has five thousand nodes and twenty-five
/// thousand edges. A measurement quotes the profile, so two runs are comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    /// How the profile is named in a measurement record and in a test failure — `S`, `M`, `L`.
    pub name: &'static str,
    /// Processes the system is expected to be running.
    pub processes: usize,
    /// Nodes the spatial graph of such a system holds.
    pub graph_nodes: usize,
    /// Edges between those nodes.
    pub edges: usize,
    /// Sockets the system is expected to have open (§32.2's socket-specific profiles).
    pub sockets: usize,
}

/// The small profile: a quiet machine (v0.4.1 §32.2).
pub const PROFILE_S: Profile = Profile {
    name: "S",
    processes: 100,
    graph_nodes: 500,
    edges: 2_000,
    sockets: 1_000,
};

/// The medium profile: an ordinary busy desktop or server (v0.4.1 §32.2).
///
/// This is the profile §33.2 states the interactive targets against, and the one §33.3's
/// no-blank-hang rule names beside Profile L.
pub const PROFILE_M: Profile = Profile {
    name: "M",
    processes: 1_000,
    graph_nodes: 5_000,
    edges: 25_000,
    sockets: 10_000,
};

/// The large profile: the cardinality at which §0.5.7's failures were first seen (v0.4.1 §32.2).
///
/// Ten thousand processes is more than a `cargo test` should create on a developer machine, so
/// no in-repository test builds this population today; the containerised fixture
/// `docker/acceptance/fixtures/perf/many-processes.pl` already forks it, and phase H7 is where
/// the benchmark command of §37.1 meets it.
pub const PROFILE_L: Profile = Profile {
    name: "L",
    processes: 10_000,
    graph_nodes: 50_000,
    edges: 250_000,
    sockets: 100_000,
};

/// A population of real, idle child processes that puts the host at a profile's cardinality.
///
/// The children are added to whatever the host is already running, so the machine the shell
/// observes holds **at least** `profile.processes` processes. That is the honest reading of a
/// profile: a Profile M measurement is made on a machine of at least Profile M size, and a
/// quiet CI runner and a busy desktop then measure the same thing rather than two different
/// ones.
///
/// Every child is killed and reaped when the population is dropped, including when the test
/// panics, so a failing run leaves nothing behind.
///
/// ```no_run
/// use ono_testkit::{PROFILE_S, ProcessPopulation};
/// let population = ProcessPopulation::of(PROFILE_S);
/// assert_eq!(population.len(), PROFILE_S.processes);
/// ```
#[derive(Debug)]
pub struct ProcessPopulation {
    profile: Profile,
    children: Vec<Child>,
}

impl ProcessPopulation {
    /// Spawns `profile.processes` idle children and waits for the kernel to list them.
    ///
    /// The children are `sleep`, which every POSIX host has, and which costs the scheduler
    /// nothing once it is asleep — the fixture is about how many objects exist, not about load.
    ///
    /// # Panics
    ///
    /// Panics if `sleep` cannot be spawned, which means the host cannot host the fixture at all.
    #[must_use]
    pub fn of(profile: Profile) -> Self {
        let children = (0..profile.processes)
            .map(|_| {
                Command::new("sleep")
                    // Longer than any single test may take, so the population cannot thin out
                    // underneath a measurement and make it mean something else.
                    .arg("900")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("`sleep` must be available to build a process population")
            })
            .collect();
        Self { profile, children }
    }

    /// The profile this population realises.
    #[must_use]
    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// How many processes the fixture itself created.
    #[must_use]
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Whether the fixture created nothing, which only a zero-process profile does.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// The process id of the first child, for a test that needs to name one of them.
    ///
    /// # Panics
    ///
    /// Panics on an empty population, which has no process to name.
    #[must_use]
    pub fn first_pid(&self) -> u32 {
        self.children
            .first()
            .expect("a population has at least one process")
            .id()
    }
}

impl Drop for ProcessPopulation {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
