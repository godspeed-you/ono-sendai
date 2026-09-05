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

impl Profile {
    /// The profile constant carrying `id`, when there is one.
    ///
    /// The registry declares the numbers and these constants carry them; this is how a row of
    /// `docs/contracts/hardening/performance_profiles.yaml` reaches the fixture that realises it.
    #[must_use]
    pub fn named(id: &str) -> Option<Self> {
        [PROFILE_S, PROFILE_M, PROFILE_L]
            .into_iter()
            .find(|profile| profile.name == id)
    }
}

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

// ------------------------------------------------------------------------------------------
// The declaration the fixtures are built from (v0.4.1 §32.2, §52.2, Appendix F).
// ------------------------------------------------------------------------------------------

/// The registry of v0.4.1 Appendix F, embedded so a fixture cannot drift from its declaration.
const DECLARATIONS: &str =
    include_str!("../../../docs/contracts/hardening/performance_profiles.yaml");

/// Where a fixture of a given size can honestly be created.
///
/// A profile is no less required for being expensive; it is measured somewhere that can afford
/// it. This says where, so nobody has to guess why one profile is built by `cargo test` and the
/// next one is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltBy {
    /// `cargo test` builds it on every gate run.
    Gate,
    /// `cargo xtask perf` and `--ignored` proofs build it: too large for every gate run.
    Benchmark,
    /// Only the acceptance image builds it, through the fixture the declaration names.
    Container,
}

impl BuiltBy {
    /// The word the registry uses.
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "gate" => Some(Self::Gate),
            "benchmark" => Some(Self::Benchmark),
            "container" => Some(Self::Container),
            _ => None,
        }
    }
}

/// One row of `docs/contracts/hardening/performance_profiles.yaml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDeclaration {
    /// `S`, `M` or `L`.
    pub id: String,
    /// Appendix F.1's process count.
    pub processes: usize,
    /// Appendix F.1's node count.
    pub graph_nodes: usize,
    /// Appendix F.1's edge count.
    pub edges: usize,
    /// Appendix F.2's socket count.
    pub sockets: usize,
    /// Open file descriptors a host must allow one process before the socket axis can be built,
    /// including the headroom the rest of the test process needs (ADR-0517).
    pub descriptors: u64,
    /// Where a fixture of this size is built.
    pub built_by: BuiltBy,
    /// Where the socket axis alone is built, when that differs from `built_by`.
    pub sockets_built_by: BuiltBy,
    /// The scripts that build it, for a `container` profile.
    fixtures: Vec<String>,
}

impl ProfileDeclaration {
    /// The repository-relative fixture scripts that build this profile, if any.
    #[must_use]
    pub fn fixtures(&self) -> &[String] {
        &self.fixtures
    }

    /// The constant a test builds this profile from.
    ///
    /// # Panics
    ///
    /// Panics when no constant carries the declared id, which means the registry declares a
    /// profile nothing can build.
    #[must_use]
    pub fn profile(&self) -> Profile {
        Profile::named(&self.id)
            .unwrap_or_else(|| panic!("no profile constant is named `{}`", self.id))
    }
}

/// One row of Appendix F.3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadDeclaration {
    /// `small`, `medium` or `large`.
    pub id: String,
    /// How many bytes one value of this profile holds.
    pub bytes: usize,
}

/// Appendix F.1 and F.2 as the registry declares them, in registry order.
///
/// # Panics
///
/// Panics if the registry cannot be parsed, which means no benchmark can state its cardinality.
#[must_use]
pub fn declared_profiles() -> Vec<ProfileDeclaration> {
    let document = registry();
    document
        .get("profiles")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .map(|rows| rows.iter().map(declaration).collect())
        .expect("performance_profiles.yaml declares `profiles`")
}

/// Appendix F.3 as the registry declares it, in registry order.
///
/// # Panics
///
/// Panics if the registry cannot be parsed.
#[must_use]
pub fn declared_payloads() -> Vec<PayloadDeclaration> {
    let document = registry();
    document
        .get("payloads")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .map(|rows| {
            rows.iter()
                .map(|row| PayloadDeclaration {
                    id: string(row, "id"),
                    bytes: count(row, "bytes"),
                })
                .collect()
        })
        .expect("performance_profiles.yaml declares `payloads`")
}

/// A value of exactly `bytes` bytes, the same bytes every time.
///
/// Appendix F.3's payload profiles exist so a materialization benchmark measures the byte budget
/// rather than the generator. Random content would make two runs incomparable and would make a
/// serialized size depend on the seed, so the fill is a fixed cycle.
#[must_use]
pub fn payload(bytes: usize) -> String {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz012345";
    (0..bytes)
        .map(|index| char::from(ALPHABET[index % ALPHABET.len()]))
        .collect()
}

/// The descriptor limit `profile`'s socket axis needs, as the registry declares it.
///
/// # Panics
///
/// Panics when the registry knows no such profile, which means the constants and the contract
/// have drifted apart.
#[must_use]
pub fn descriptors_for(profile: Profile) -> u64 {
    declared_profiles()
        .into_iter()
        .find(|declaration| declaration.id == profile.name)
        .unwrap_or_else(|| {
            panic!(
                "performance_profiles.yaml declares no profile `{}`",
                profile.name
            )
        })
        .descriptors
}

/// The parsed registry.
fn registry() -> serde_yaml_ng::Value {
    serde_yaml_ng::from_str(DECLARATIONS)
        .expect("docs/contracts/hardening/performance_profiles.yaml must be valid YAML")
}

/// One profile row.
fn declaration(row: &serde_yaml_ng::Value) -> ProfileDeclaration {
    let id = string(row, "id");
    let built = string(row, "built_by");
    ProfileDeclaration {
        processes: count(row, "processes"),
        graph_nodes: count(row, "graph_nodes"),
        edges: count(row, "edges"),
        sockets: count(row, "sockets"),
        descriptors: count(row, "descriptors") as u64,
        built_by: BuiltBy::from_name(&built)
            .unwrap_or_else(|| panic!("profile `{id}` declares an unknown `built_by`: {built}")),
        sockets_built_by: row
            .get("sockets_built_by")
            .and_then(serde_yaml_ng::Value::as_str)
            .map_or_else(
                || {
                    BuiltBy::from_name(&built).unwrap_or_else(|| {
                        panic!("profile `{id}` declares an unknown `built_by`: {built}")
                    })
                },
                |name| {
                    BuiltBy::from_name(name).unwrap_or_else(|| {
                        panic!("profile `{id}` declares an unknown `sockets_built_by`: {name}")
                    })
                },
            ),
        fixtures: ["fixture", "socket_fixture"]
            .iter()
            .filter_map(|field| row.get(field).and_then(serde_yaml_ng::Value::as_str))
            .map(str::to_owned)
            .collect(),
        id,
    }
}

/// A required string field.
fn string(row: &serde_yaml_ng::Value, field: &str) -> String {
    row.get(field)
        .and_then(serde_yaml_ng::Value::as_str)
        .unwrap_or_else(|| panic!("a performance profile row must declare `{field}`"))
        .to_owned()
}

/// A required count field.
fn count(row: &serde_yaml_ng::Value, field: &str) -> usize {
    usize::try_from(
        row.get(field)
            .and_then(serde_yaml_ng::Value::as_u64)
            .unwrap_or_else(|| panic!("a performance profile row must declare `{field}`")),
    )
    .expect("a cardinality fits a usize")
}

// ------------------------------------------------------------------------------------------
// The socket half of §32.2 (Appendix F.2).
// ------------------------------------------------------------------------------------------

/// A population of real listening sockets that puts the host at a profile's socket cardinality.
///
/// Unix domain sockets, for the reason `docker/acceptance/fixtures/perf/many-sockets.pl` gives:
/// the acceptance container runs with networking disabled, and a unix listener is the one kind
/// that always exists. They are listening sockets with a queue, so `sock_diag` reports them
/// exactly as it reports any other listener and the production socket provider reads them
/// unchanged — §32.2's rule that the code under measurement must be production logic.
///
/// Every socket is closed and its path removed when the population is dropped, including when
/// the test panics.
///
/// ```no_run
/// use ono_testkit::{PROFILE_S, SocketPopulation};
/// let population = SocketPopulation::of(PROFILE_S);
/// assert_eq!(population.len(), PROFILE_S.sockets);
/// ```
#[derive(Debug)]
pub struct SocketPopulation {
    profile: Profile,
    directory: std::path::PathBuf,
    listeners: Vec<std::os::unix::net::UnixListener>,
}

impl SocketPopulation {
    /// Opens `profile.sockets` listening unix sockets in a directory of its own.
    ///
    /// # Panics
    ///
    /// Panics if the host cannot supply the descriptors, which is a host capability rather than
    /// a product defect: use [`SocketPopulation::try_of`] wherever the caller can report the
    /// shortfall as a skip instead (v0.4.1 §38.1, §38.4).
    #[must_use]
    pub fn of(profile: Profile) -> Self {
        Self::try_of(profile).unwrap_or_else(|shortfall| {
            panic!(
                "Profile {} needs {} listening sockets and {shortfall}",
                profile.name, profile.sockets
            )
        })
    }

    /// Opens `profile.sockets` listening unix sockets, or reports the descriptor limit it could
    /// not reach.
    ///
    /// The soft limit is raised toward the hard one first. That changes nothing about what the
    /// fixture measures — the descriptors were always allowed and the process was simply not
    /// asking for them — so it is tried before anything is reported. Only a *hard* limit below
    /// the profile's cardinality is a host that has genuinely refused, and a host that refuses
    /// has not found a defect in the product (v0.4.1 §38.1).
    ///
    /// # Errors
    ///
    /// Returns the shortfall when this host cannot hold the profile's sockets open at once.
    ///
    /// # Panics
    ///
    /// Panics if the directory or a socket cannot be created for a reason other than the
    /// descriptor limit, which is a broken host rather than a small one.
    pub fn try_of(profile: Profile) -> Result<Self, crate::DescriptorShortfall> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        // The requirement comes from the registry beside the cardinality that fixes it, not from
        // a constant here: the fixture is not the only thing holding descriptors, and how much
        // headroom the rest of the test process needs is a statement about the profile
        // (§52.2, ADR-0517).
        crate::require_descriptors(descriptors_for(profile))?;

        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::var_os("CARGO_TARGET_TMPDIR")
            .map_or_else(std::env::temp_dir, std::path::PathBuf::from);
        let directory = root.join(format!("ono-sockets-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&directory)
            .unwrap_or_else(|error| panic!("cannot create {}: {error}", directory.display()));

        let listeners = (0..profile.sockets)
            .map(|index| {
                let path = directory.join(format!("s{index}"));
                std::os::unix::net::UnixListener::bind(&path).unwrap_or_else(|error| {
                    panic!(
                        "cannot open socket {index} of {} at {}: {error}. Profile {} needs a \
                         descriptor limit above its socket cardinality",
                        profile.sockets,
                        path.display(),
                        profile.name
                    )
                })
            })
            .collect();
        Ok(Self {
            profile,
            directory,
            listeners,
        })
    }

    /// The profile this population realises.
    #[must_use]
    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// How many sockets the fixture itself opened.
    #[must_use]
    pub fn len(&self) -> usize {
        self.listeners.len()
    }

    /// Whether the fixture opened nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.listeners.is_empty()
    }

    /// The directory the sockets live in, for a test that needs to name one of them.
    #[must_use]
    pub fn directory(&self) -> &std::path::Path {
        &self.directory
    }
}

impl Drop for SocketPopulation {
    fn drop(&mut self) {
        // Closing the listeners releases the descriptors; the paths outlive them, so the
        // directory goes too. A failure here must not mask the test's own outcome.
        self.listeners.clear();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
