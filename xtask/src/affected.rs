//! Which packages the tests of an increment have to cover (ADR-0853).
//!
//! `cargo test` runs its test binaries one after another, and the whole workspace is 568 of them,
//! so a local gate spent eight of its ten minutes on tests whatever the increment touched. This
//! module answers the question the gate asks before its test step: given the files that changed,
//! which packages can a test failure come from?
//!
//! Three rules, and a fourth that is the absence of the others:
//!
//! - **A file inside a package** selects that package and every package that depends on it,
//!   directly or not, by any kind of dependency.
//! - **A file of [`HARNESS`]** selects [`HARNESS_PACKAGE`] — the decision records, the state
//!   board, the scripts, the acceptance cases. Nothing outside xtask reads them, and
//!   `xtask/tests/affected.rs` checks that claim against every source outside xtask. The
//!   workflows are not among them: the fuzz crate's tests read `fuzz.yml`.
//! - **The harness is in every selection.** Its tests read the sources of the whole repository —
//!   the metrics, the scans, the evidence checks — so there is no change they cannot see.
//! - **Anything else selects every package**: the lockfile, the toolchain, the contracts the build
//!   scripts compile, a file no rule knows. A guess here would be a package whose tests an
//!   increment skipped without anyone deciding so.
//!
//! No changed file at all selects every package too, because a gate run on a clean tree is a
//! re-run after the commit and is asked about the tree rather than about an increment.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The paths only `xtask` reads: a directory ends with `/`, anything else is one file.
pub const HARNESS: &[&str] = &[
    "docs/STATE.md",
    "docs/ACCEPTANCE.md",
    "docs/adr/",
    "docs/architecture/",
    "docs/baselines/",
    "docs/reference/",
    "docs/releases/",
    "docs/specs/",
    "docs/strategy/",
    "scripts/",
    "docker/acceptance/",
    "AGENTS.md",
    "CLAUDE.md",
    "README.md",
    "LICENSE",
    "deny.toml",
];

/// The package that reads the paths of [`HARNESS`].
pub const HARNESS_PACKAGE: &str = "xtask";

/// A workspace member, as far as the selection needs to know it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The package's name, as `cargo test --package` takes it.
    pub name: String,
    /// Its directory, relative to the workspace root.
    pub directory: String,
    /// The workspace members it depends on, of any kind.
    pub dependencies: Vec<String>,
}

/// The members of a workspace and how they depend on each other.
#[derive(Debug, Clone, Default)]
pub struct Workspace {
    packages: Vec<Package>,
}

impl Workspace {
    /// A workspace of the given members.
    #[must_use]
    pub fn new(packages: Vec<Package>) -> Self {
        Self { packages }
    }

    /// The workspace rooted at `root`, as `cargo metadata` describes it.
    ///
    /// # Errors
    ///
    /// When `cargo metadata` cannot run or says something this cannot read.
    pub fn read(root: &Path) -> Result<Self, String> {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let output = Command::new(cargo)
            .args(["metadata", "--no-deps", "--format-version", "1"])
            .current_dir(root)
            .output()
            .map_err(|error| format!("cargo metadata cannot run: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "cargo metadata failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("cargo metadata said something unreadable: {error}"))?;
        let workspace_root = metadata["workspace_root"]
            .as_str()
            .map(PathBuf::from)
            .ok_or("cargo metadata names no workspace root")?;
        let members = metadata["packages"]
            .as_array()
            .ok_or("cargo metadata lists no packages")?;

        let mut by_directory = BTreeMap::new();
        for member in members {
            let (Some(name), Some(manifest)) =
                (member["name"].as_str(), member["manifest_path"].as_str())
            else {
                return Err("cargo metadata lists a package without a name or manifest".into());
            };
            let directory = Path::new(manifest)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            by_directory.insert(directory, name.to_owned());
        }

        let mut packages = Vec::new();
        for member in members {
            let name = member["name"].as_str().unwrap_or_default().to_owned();
            let manifest = Path::new(member["manifest_path"].as_str().unwrap_or_default());
            let directory = manifest
                .parent()
                .and_then(|directory| directory.strip_prefix(&workspace_root).ok())
                .map(|directory| directory.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            // A path dependency names its member by directory, which survives a `package = …`
            // rename that the dependency's own name would not.
            let dependencies = member["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|dependency| dependency["path"].as_str())
                .filter_map(|path| by_directory.get(Path::new(path)).cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            packages.push(Package {
                name,
                directory,
                dependencies,
            });
        }
        Ok(Self::new(packages))
    }

    /// The member whose directory holds `path`, the innermost one when directories nest.
    fn owner(&self, path: &str) -> Option<&Package> {
        self.packages
            .iter()
            .filter(|package| !package.directory.is_empty())
            .filter(|package| {
                path == package.directory
                    || path
                        .strip_prefix(package.directory.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .max_by_key(|package| package.directory.len())
    }

    /// `changed` and every member that depends on one of them, transitively.
    fn with_dependents(&self, changed: BTreeSet<String>) -> BTreeSet<String> {
        let mut selected = changed;
        let mut frontier: Vec<String> = selected.iter().cloned().collect();
        while let Some(name) = frontier.pop() {
            for package in &self.packages {
                if package.dependencies.contains(&name) && selected.insert(package.name.clone()) {
                    frontier.push(package.name.clone());
                }
            }
        }
        selected
    }
}

/// The packages an increment's tests have to cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Every package, and why.
    Everything {
        /// What made the selection cover the whole workspace.
        reason: String,
    },
    /// These packages and no other.
    Packages(BTreeSet<String>),
}

impl Selection {
    /// The package arguments `cargo test` takes for this selection.
    #[must_use]
    pub fn cargo_arguments(&self) -> Vec<String> {
        match self {
            Self::Everything { .. } => vec!["--workspace".to_owned()],
            Self::Packages(names) => names
                .iter()
                .flat_map(|name| ["--package".to_owned(), name.clone()])
                .collect(),
        }
    }
}

/// The packages whose tests the change to `changed` can fail, paths relative to the root.
#[must_use]
pub fn select(workspace: &Workspace, changed: &[String]) -> Selection {
    if changed.is_empty() {
        return Selection::Everything {
            reason: "nothing differs from the commit, so the question is the whole tree".into(),
        };
    }
    let mut touched = BTreeSet::from([HARNESS_PACKAGE.to_owned()]);
    for path in changed {
        if let Some(package) = workspace.owner(path) {
            touched.insert(package.name.clone());
        } else if !is_harness(path) {
            return Selection::Everything {
                reason: format!(
                    "{path} belongs to no package and is not a file only {HARNESS_PACKAGE} reads"
                ),
            };
        }
    }
    Selection::Packages(workspace.with_dependents(touched))
}

fn is_harness(path: &str) -> bool {
    HARNESS.iter().any(|harness| {
        if harness.ends_with('/') {
            path.starts_with(harness)
        } else {
            path == *harness
        }
    })
}
