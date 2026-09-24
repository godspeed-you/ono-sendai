//! The stripped size of the shipped binaries, recorded and budgeted (issue #125, ADR-0863,
//! ADR-0864).
//!
//! A 15 MB increase in the shipped `ono` passed the gate, the acceptance suite and the package
//! build without a word, because the counts §50.1 names are about the repository and none of them
//! is about the artifact it exists to produce. This module adds the artifact:
//!
//! * **a record.** `cargo xtask metrics --write` measures every current release build of the
//!   [`BINARIES`] under the target directory and writes its size, per binary and target triple,
//!   into [`RECORD`]. The README block and the release input manifest read that file, so both
//!   carry the same figure and neither needs a release build to be checked.
//! * **a budget.** `build_budgets` in `docs/contracts/hardening/limits.yaml`, beside the runtime
//!   limits, holds a ceiling per binary — today `build.ono_stripped_bytes` for the shell.
//!   `cargo xtask binary-size` fails when a release binary exceeds its budget, and `spec-check`
//!   fails when the record does.
//! * **no silent pass.** A binary that is missing, older than its sources, or not the build that
//!   ships is *not measured*, and says so. Where a measurement is required (`--require`: the
//!   acceptance image, which has just built the release binary) its absence fails; on a developer
//!   machine it is announced, because the alternative is a fat-LTO release build inside every gate
//!   run (ADR-0864).
//!
//! The size is the file's size. The release profile strips symbols (`strip = "symbols"`) and
//! `scripts/package.sh` packages with `--no-strip`, so the bytes measured are the bytes a package
//! installs.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::scan::Problem;

/// Where the recorded figures live, per binary and target triple.
pub const RECORD: &str = "docs/baselines/binary-size.yaml";

/// The registry that holds the budgets, beside the runtime limits (v0.4.1 §52.1, §52.2).
pub const LIMITS: &str = "docs/contracts/hardening/limits.yaml";

/// The release binaries whose size is recorded: the shell, and the KUANG/11 compiler that left it
/// (ADR-0870).
pub const BINARIES: &[&str] = &["ono", "kuang-compile"];

/// The triples `ono` is built for: the two a release packages (ADR-0123), and the static musl
/// target of the core build (ADR-0912), which is where `scripts/build-core.sh` puts it.
const RELEASE_TRIPLES: &[&str] = &[
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
];

/// The crate whose sources in an `ono`'s dep-info say it links wasmtime's compiler.
///
/// The shipped shell never does (ADR-0870). One built in the same cargo invocation as
/// `kuang-compile` does, because cargo unifies features across the packages of an invocation, and
/// its size is not the size of anything a release ships.
const COMPILER_CRATE: &str = "cranelift-codegen-";

/// The source directory, in an `ono`'s dep-info, of a crate only the full build links.
///
/// The core build (#127, ADR-0910) leaves out the KUANG/11 tier and with it this workspace crate,
/// so its presence tells the two builds of one binary apart (ADR-0868). The core build is the
/// musl triple's (ADR-0912), the full shell every other triple's.
const FULL_ONLY_CRATE: &str = "/crates/ono-kuang-supervisor/";

/// The triple the core build ships on.
const CORE_TRIPLE: &str = "x86_64-unknown-linux-musl";

/// What a release binary found under the target directory turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// The build that ships, from the sources as they are now: its size is a measurement of this
    /// tree.
    Current {
        /// Which of [`BINARIES`] it is.
        binary: String,
        /// The target triple it was built for.
        triple: String,
        /// Where it is.
        path: PathBuf,
        /// Its size on disk, which is its stripped size.
        bytes: u64,
    },
    /// Older than something it was built from, impossible to date, or not the build that ships:
    /// its size is a measurement of something else.
    Unmeasured {
        /// Which of [`BINARIES`] it is.
        binary: String,
        /// The target triple it was built for.
        triple: String,
        /// Where it is.
        path: PathBuf,
        /// Why it is not measured, for the reader.
        reason: String,
    },
}

/// What a check of the binaries against their budgets found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// One line per binary, and one per budgeted binary nothing was measured for.
    pub lines: Vec<String>,
    /// Whether the check passed.
    pub passed: bool,
}

/// The `build_budgets` rows of [`LIMITS`], or the reason the registry cannot be read.
fn budget_rows(root: &Path) -> Result<Vec<serde_yaml_ng::Value>, String> {
    let text = std::fs::read_to_string(root.join(LIMITS))
        .map_err(|error| format!("{LIMITS} cannot be read: {error}"))?;
    let document = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text)
        .map_err(|error| format!("{LIMITS} is not YAML: {error}"))?;
    Ok(document
        .get("build_budgets")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .cloned()
        .unwrap_or_default())
}

/// One `build_budgets` row: which binary, for which triple (or every triple without a row of its
/// own), how many bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Budget {
    key: String,
    binary: String,
    triple: Option<String>,
    bytes: Option<u64>,
}

/// Every budget row [`LIMITS`] declares.
fn budgets(root: &Path) -> Vec<Budget> {
    let text = |row: &serde_yaml_ng::Value, field: &str| {
        row.get(field)
            .and_then(serde_yaml_ng::Value::as_str)
            .map(str::to_owned)
    };
    budget_rows(root)
        .unwrap_or_default()
        .iter()
        .filter_map(|row| {
            Some(Budget {
                key: text(row, "key").unwrap_or_default(),
                binary: text(row, "binary")?,
                triple: text(row, "triple"),
                bytes: row.get("budget").and_then(serde_yaml_ng::Value::as_u64),
            })
        })
        .collect()
}

/// The row that holds `binary` on `triple`: the one naming the triple, else the one naming none.
fn budget_row(root: &Path, binary: &str, triple: &str) -> Option<Budget> {
    let rows = budgets(root);
    let of_binary = |row: &&Budget| row.binary == binary;
    rows.iter()
        .filter(of_binary)
        .find(|row| row.triple.as_deref() == Some(triple))
        .or_else(|| {
            rows.iter()
                .filter(of_binary)
                .find(|row| row.triple.is_none())
        })
        .cloned()
}

/// The budget [`LIMITS`] declares for `binary` built for `triple`, in bytes: the row naming the
/// triple, or the binary's row that names none.
///
/// # Errors
///
/// Returns the reason when the registry cannot be read or declares no budget for it: a budget
/// nobody declared is not one a binary can meet.
pub fn budget(root: &Path, binary: &str, triple: &str) -> Result<u64, String> {
    budget_rows(root)?;
    budget_row(root, binary, triple)
        .and_then(|row| row.bytes)
        .ok_or_else(|| {
            format!(
                "{LIMITS} declares no budget for `{binary}` on {triple} under `build_budgets`, \
                 so there is nothing to hold it to (issue #125, ADR-0864)"
            )
        })
}

/// The figures [`RECORD`] holds for `binary`, by target triple. Empty when there are none.
#[must_use]
pub fn recorded(root: &Path, binary: &str) -> BTreeMap<String, u64> {
    all_recorded(root).remove(binary).unwrap_or_default()
}

/// Every figure [`RECORD`] holds, by binary and target triple.
#[must_use]
pub fn all_recorded(root: &Path) -> BTreeMap<String, BTreeMap<String, u64>> {
    let Some(document) = std::fs::read_to_string(root.join(RECORD))
        .ok()
        .and_then(|text| serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text).ok())
    else {
        return BTreeMap::new();
    };
    document
        .get("stripped_bytes")
        .and_then(serde_yaml_ng::Value::as_mapping)
        .into_iter()
        .flatten()
        .filter_map(|(binary, figures)| {
            let figures = figures
                .as_mapping()?
                .iter()
                .filter_map(|(triple, bytes)| Some((triple.as_str()?.to_owned(), bytes.as_u64()?)))
                .collect();
            Some((binary.as_str()?.to_owned(), figures))
        })
        .collect()
}

/// Every release build of the [`BINARIES`] under `target_dir`, dated against its sources.
///
/// `target/<triple>/release/<binary>` is what `scripts/package.sh` builds and packages, so it is
/// taken for its triple; `target/release/<binary>` is a plain `cargo build --release` for the
/// `host`, taken when no current explicit build for the host exists beside it. At most one per
/// binary and triple.
#[must_use]
pub fn find(root: &Path, target_dir: &Path, host: &str) -> Vec<Found> {
    let mut triples: Vec<&str> = RELEASE_TRIPLES.to_vec();
    if !triples.contains(&host) {
        triples.push(host);
    }
    let mut found = Vec::new();
    for binary in BINARIES {
        for triple in &triples {
            let mut candidates = vec![target_dir.join(triple).join("release").join(binary)];
            if *triple == host {
                candidates.push(target_dir.join("release").join(binary));
            }
            let dated: Vec<Found> = candidates
                .into_iter()
                .filter(|path| path.is_file())
                .map(|path| date(root, binary, triple, path))
                .collect();
            // The explicit build first, then the plain one; a current one before any other.
            if let Some(chosen) = dated
                .iter()
                .find(|found| matches!(found, Found::Current { .. }))
                .or_else(|| dated.first())
            {
                found.push(chosen.clone());
            }
        }
    }
    found
}

/// Measures one named binary without dating it, for a caller that has just built it.
///
/// The binary is known by its file name, which is one of [`BINARIES`].
///
/// # Errors
///
/// Returns the reason the file cannot be read or is not one of the recorded binaries.
pub fn named(path: &Path, triple: &str) -> Result<Found, String> {
    let binary = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| BINARIES.contains(&name.as_str()))
        .ok_or_else(|| {
            format!(
                "{} is none of the binaries whose size is recorded ({})",
                path.display(),
                BINARIES.join(", ")
            )
        })?;
    let bytes = std::fs::metadata(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .len();
    Ok(Found::Current {
        binary,
        triple: triple.to_owned(),
        path: path.to_path_buf(),
        bytes,
    })
}

/// Whether `path` is the shipping build of `binary`, newer than everything it was built from.
///
/// Cargo writes the dep-info file `<binary>.d` beside every binary it links, naming every source
/// file that went into it — the workspace's and the registry's. The workspace manifest, the
/// lockfile and the toolchain pin are added, because the release profile and the dependency graph
/// live there and a change to either rebuilds the binary without touching a source.
fn date(root: &Path, binary: &str, triple: &str, path: PathBuf) -> Found {
    let unmeasured = |reason: String| Found::Unmeasured {
        binary: binary.to_owned(),
        triple: triple.to_owned(),
        path: path.clone(),
        reason,
    };
    let Ok(metadata) = std::fs::metadata(&path) else {
        return unmeasured("it cannot be read".to_owned());
    };
    let Ok(built) = metadata.modified() else {
        return unmeasured("the filesystem does not say when it was written".to_owned());
    };
    let mut dependency = path.clone().into_os_string();
    dependency.push(".d");
    let Ok(listing) = std::fs::read_to_string(&dependency) else {
        return unmeasured(format!(
            "cargo's dep-info file {} is missing, so nothing says what it was built from",
            PathBuf::from(dependency).display()
        ));
    };
    let mut inputs = dependency_paths(&listing);
    if binary == "ono"
        && inputs
            .iter()
            .any(|input| input.to_string_lossy().contains(COMPILER_CRATE))
    {
        return unmeasured(
            "it links wasmtime's compiler, which the shipped shell does not (ADR-0870): it was \
             built in one cargo invocation with `kuang-compile`, whose features cargo unified \
             into it. `cargo build --release --locked -p ono-cli` builds the shell that ships"
                .to_owned(),
        );
    }
    if binary == "ono" {
        let full = inputs
            .iter()
            .any(|input| input.to_string_lossy().contains(FULL_ONLY_CRATE));
        if triple == CORE_TRIPLE && full {
            return unmeasured(format!(
                "it is the full shell, and {CORE_TRIPLE} is the core build's triple (ADR-0912). \
                 `scripts/build-core.sh` builds the core binary that ships there"
            ));
        }
        if triple != CORE_TRIPLE && !full {
            return unmeasured(
                "it is the core build (it links no KUANG/11 supervisor), and this triple ships \
                 the full shell (ADR-0868): a core build without `--target` lands where the \
                 full one does. `cargo build --release --locked -p ono-cli` builds the full shell"
                    .to_owned(),
            );
        }
    }
    // The manifests are inputs as well: the release profile and the dependency graph live in the
    // root ones, and a member's feature list in its own, and editing either rebuilds the binary
    // without touching a source the dep-info names.
    for file in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"] {
        let input = root.join(file);
        if input.is_file() {
            inputs.push(input);
        }
    }
    for members in ["crates", "xtask", "fuzz"] {
        let directory = root.join(members);
        let manifests: Vec<PathBuf> = if members == "crates" {
            std::fs::read_dir(&directory)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path().join("Cargo.toml"))
                .collect()
        } else {
            vec![directory.join("Cargo.toml")]
        };
        inputs.extend(manifests.into_iter().filter(|manifest| manifest.is_file()));
    }
    for input in inputs {
        let input = under_checkout(root, input);
        match std::fs::metadata(&input).and_then(|metadata| metadata.modified()) {
            Ok(changed) if changed <= built => {}
            Ok(_) => return unmeasured(format!("{} changed after it was built", input.display())),
            Err(_) => {
                return unmeasured(format!(
                    "{} went into it and no longer exists",
                    input.display()
                ));
            }
        }
    }
    Found::Current {
        binary: binary.to_owned(),
        triple: triple.to_owned(),
        path,
        bytes: metadata.len(),
    }
}

/// A dep-info path as it is on this machine.
///
/// `scripts/package.sh` builds in a container with the checkout mounted at `/project` and cargo's
/// home under `target/`, so the dep-info of the binary that ships names `/project/crates/…` and
/// `/project/target/container-cargo/registry/…`. A path that does not exist is re-rooted at the
/// checkout by its longest tail that does; one with no such tail is returned as it is, and dating
/// it then reports it missing.
fn under_checkout(root: &Path, path: PathBuf) -> PathBuf {
    if path.exists() {
        return path;
    }
    let components: Vec<_> = path.components().collect();
    (1..components.len())
        .map(|skip| root.join(components[skip..].iter().collect::<PathBuf>()))
        .find(|candidate| candidate.exists())
        .unwrap_or(path)
}

/// The inputs a Makefile-style dep-info file names after its first target.
///
/// Cargo escapes a space inside a path as `\ `; nothing else in a path is escaped.
fn dependency_paths(listing: &str) -> Vec<PathBuf> {
    let Some(first) = listing.lines().next() else {
        return Vec::new();
    };
    let Some((_, inputs)) = first.split_once(": ") else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    let mut current = String::new();
    let mut characters = inputs.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' if characters.peek() == Some(&' ') => {
                current.push(' ');
                characters.next();
            }
            ' ' | '\t' => {
                if !current.is_empty() {
                    paths.push(PathBuf::from(std::mem::take(&mut current)));
                }
            }
            other => current.push(other),
        }
    }
    if !current.is_empty() {
        paths.push(PathBuf::from(current));
    }
    paths
}

/// Holds every measured binary against its budget.
///
/// A binary over its budget fails. A budgeted binary with no measurement — missing, stale, or
/// not the build that ships — is *not measured*, and that is said in so many words; it fails
/// only when `require` is set, because a measurement is required where the release binary has
/// just been built (ADR-0864). The shell's budget missing from the registry fails; a binary with
/// no budget of its own is reported and held to none.
#[must_use]
pub fn check(root: &Path, found: &[Found], require: bool) -> Verdict {
    if !budgets(root).iter().any(|row| row.binary == "ono") {
        return Verdict {
            lines: vec![format!(
                "{LIMITS} declares no budget for `ono` under `build_budgets`, so there is nothing \
                 to hold the shell to (issue #125, ADR-0864)"
            )],
            passed: false,
        };
    }
    let mut lines = Vec::new();
    let mut passed = true;
    for binary in found {
        match binary {
            Found::Current {
                binary,
                triple,
                path,
                bytes,
            } => match budget(root, binary, triple) {
                Ok(budget) if *bytes > budget => {
                    passed = false;
                    lines.push(format!(
                        "{} ({triple}) is {bytes} bytes, over its budget of {budget} bytes by {} \
                         — `build_budgets` in {LIMITS}. A binary this much larger needs the ADR \
                         that raises the budget (ADR-0863)",
                        path.display(),
                        bytes - budget
                    ));
                }
                Ok(budget) => lines.push(format!(
                    "{} ({triple}) is {bytes} bytes, within its budget of {budget} bytes \
                     ({:.1} %)",
                    path.display(),
                    percent(*bytes, budget)
                )),
                Err(_) => lines.push(format!(
                    "{} ({triple}) is {bytes} bytes; it has no budget of its own",
                    path.display()
                )),
            },
            Found::Unmeasured {
                triple,
                path,
                reason,
                ..
            } => lines.push(format!(
                "{} ({triple}) is not measured: {reason}",
                path.display()
            )),
        }
    }
    // Which budget each measured binary answered, so the budgets nobody answered can be named.
    let answered: Vec<Budget> = found
        .iter()
        .filter_map(|found| match found {
            Found::Current { binary, triple, .. } => budget_row(root, binary, triple),
            Found::Unmeasured { .. } => None,
        })
        .collect();
    for row in budgets(root) {
        if answered.contains(&row) {
            continue;
        }
        let which = row
            .triple
            .as_deref()
            .map_or_else(String::new, |triple| format!(" on {triple}"));
        lines.push(format!(
            "`{}`{which} is not measured (`{}`): no current release build of it is here. `cargo \
             build --release --locked -p ono-cli` builds the shell, `scripts/build-core.sh` the \
             core build; the acceptance image measures the shell on every CI run (ADR-0864)",
            row.binary, row.key
        ));
    }
    if require && answered.is_empty() {
        passed = false;
        lines.push(
            "a measurement is required here, and nothing budgeted was measured: a missing \
             measurement is a failure rather than a pass"
                .to_owned(),
        );
    }
    Verdict { lines, passed }
}

/// Writes the size of every current binary into [`RECORD`], keeping the figures of the binaries
/// and triples that were not built here, and answers what it did.
///
/// # Errors
///
/// Returns the reason the record cannot be written.
pub fn record(root: &Path, found: &[Found]) -> Result<Vec<String>, String> {
    let mut figures = all_recorded(root);
    let mut said = Vec::new();
    for binary in found {
        match binary {
            Found::Current {
                binary,
                triple,
                path,
                bytes,
            } => {
                figures
                    .entry(binary.clone())
                    .or_default()
                    .insert(triple.clone(), *bytes);
                said.push(format!(
                    "recorded {bytes} bytes for {binary} on {triple} from {}",
                    path.display()
                ));
            }
            Found::Unmeasured {
                binary,
                triple,
                path,
                reason,
            } => said.push(format!(
                "kept the recorded figure for {binary} on {triple}: {} is not measured — {reason}",
                path.display()
            )),
        }
    }
    if found.is_empty() {
        said.push(
            "no release binary is built, so the recorded sizes are kept. `cargo build --release \
             --locked -p ono-cli` builds the shell"
                .to_owned(),
        );
    }
    if figures.is_empty() {
        return Ok(said);
    }
    let text = render_record(&figures);
    let path = root.join(RECORD);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        std::fs::write(&path, text).map_err(|error| format!("{RECORD}: {error}"))?;
    }
    Ok(said)
}

/// The record file, header and figures.
fn render_record(figures: &BTreeMap<String, BTreeMap<String, u64>>) -> String {
    let mut text = String::from(
        "# The stripped size of the shipped binaries, in bytes, per target triple (issue #125).\n\
         #\n\
         # Written by `cargo xtask metrics --write` from release builds of the tree it sits in:\n\
         # `target/<triple>/release/<binary>` from scripts/package.sh, or \
         `target/release/<binary>` for\n\
         # the host. A binary or triple nobody built on the recording machine keeps its earlier \
         figure.\n\
         # The README block and the release input manifest read this file; the budgets are\n\
         # `build_budgets` in docs/contracts/hardening/limits.yaml (ADR-0863, ADR-0864). Never\n\
         # hand-edited.\n\
         \n\
         schema: ono.binary-size.v1\n\
         stripped_bytes:\n",
    );
    for (binary, triples) in figures {
        let _ = writeln!(text, "  {binary}:");
        for (triple, bytes) in triples {
            let _ = writeln!(text, "    {triple}: {bytes}");
        }
    }
    text
}

/// Holds the record against the budgets, for `spec-check`.
///
/// A budget with no recorded figure it holds is a measurement nobody made, and a recorded figure
/// over its budget is a tree that was already over it when it was measured.
#[must_use]
pub fn check_record(root: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    let figures = all_recorded(root);
    for row in budgets(root) {
        let Some(budget) = row.bytes else {
            problems.push(Problem::new(
                LIMITS,
                format!(
                    "`{}` budgets `{}` without a whole number of bytes",
                    row.key, row.binary
                ),
            ));
            continue;
        };
        let held: Vec<(&String, &u64)> = figures
            .get(&row.binary)
            .into_iter()
            .flatten()
            .filter(|(triple, _)| budget_row(root, &row.binary, triple).as_ref() == Some(&row))
            .collect();
        if held.is_empty() {
            problems.push(Problem::new(
                RECORD,
                format!(
                    "records no stripped size that `{}` holds ({budget} bytes for `{}`{}). Build \
                     it (`cargo build --release --locked -p ono-cli` for the shell, \
                     `scripts/build-core.sh` for the core build) and run `cargo xtask metrics \
                     --write` (issue #125)",
                    row.key,
                    row.binary,
                    row.triple
                        .as_deref()
                        .map_or_else(String::new, |triple| format!(" on {triple}")),
                ),
            ));
        }
        for (triple, bytes) in held.into_iter().filter(|(_, bytes)| **bytes > budget) {
            problems.push(Problem::new(
                RECORD,
                format!(
                    "records {bytes} bytes for `{}` on {triple}, over its budget of {budget} \
                     bytes (`{}` in {LIMITS}). The commit that made it larger needs the ADR that \
                     raises the budget (ADR-0863)",
                    row.binary, row.key
                ),
            ));
        }
    }
    problems
}

/// The part of the release input manifest this module answers (Appendix H, issue #125).
///
/// Per binary, the recorded figures and the budget — the numbers the gate and the README read —
/// or `null` for either that is not there (spec §35.3: unknown is null).
#[must_use]
pub fn manifest_entry(root: &Path) -> serde_json::Value {
    let figures = all_recorded(root);
    let entries = BINARIES
        .iter()
        .map(|binary| {
            let stripped = figures
                .get(*binary)
                .filter(|triples| !triples.is_empty())
                .map_or(serde_json::Value::Null, |triples| {
                    serde_json::Value::Object(
                        triples
                            .iter()
                            .map(|(triple, bytes)| {
                                (triple.clone(), serde_json::Value::from(*bytes))
                            })
                            .collect(),
                    )
                });
            (
                (*binary).to_owned(),
                serde_json::json!({
                    "record": RECORD,
                    "stripped_bytes": stripped,
                    "budget_bytes": budgets_of(root, binary, figures.get(*binary)),
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    serde_json::Value::Object(entries)
}

/// The budget of every recorded triple of `binary`, or `null` when none is recorded or budgeted.
fn budgets_of(
    root: &Path,
    binary: &str,
    triples: Option<&BTreeMap<String, u64>>,
) -> serde_json::Value {
    let held: serde_json::Map<String, serde_json::Value> = triples
        .into_iter()
        .flatten()
        .filter_map(|(triple, _)| {
            budget(root, binary, triple)
                .ok()
                .map(|bytes| (triple.clone(), serde_json::Value::from(bytes)))
        })
        .collect();
    if held.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::Object(held)
    }
}

/// How much of `budget` `bytes` spends.
#[allow(
    clippy::cast_precision_loss,
    reason = "a percentage for a person to read; a binary is nowhere near 2^52 bytes"
)]
fn percent(bytes: u64, budget: u64) -> f64 {
    if budget == 0 {
        return 100.0;
    }
    bytes as f64 * 100.0 / budget as f64
}

/// The triple `rustc` builds for on this machine.
///
/// # Errors
///
/// Returns the reason `rustc` could not answer.
pub fn host_triple() -> Result<String, String> {
    let output = std::process::Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|error| format!("cannot run rustc: {error}"))?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or_else(|| "rustc -vV names no host triple".to_owned())
}
