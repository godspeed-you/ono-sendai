//! The notices the shipped binary owes, generated from the lockfile (ADR-0608).
//!
//! `deny.toml` decides which licences this project may ship; it does not discharge what those
//! licences ask for once the binary is shipped. MIT asks that its permission notice travel with
//! the software, Apache-2.0 §4 that its text and any `NOTICE` travel with a derivative work, and
//! `CDLA-Permissive-2.0` §2.1 that its own text travel with the data — `webpki-roots` embeds the
//! Mozilla root store in the binary (ADR-0607). One file answers all of them: every crate the
//! `ono` binary links, and every licence text those crates ship, reproduced once.
//!
//! It is generated rather than maintained, because a file somebody edits by hand falls behind
//! the first dependency bump and then says something untrue about what is being distributed.
//! `spec-check` regenerates it and compares, so the gate is red while it is stale.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::scan::Problem;

/// Where the generated notices live, relative to the repository root.
pub const NOTICES_FILE: &str = "THIRD-PARTY-LICENSES";

/// The package whose dependency graph is shipped: the `ono` binary.
const SHIPPED: &str = "ono-cli";

/// One crate the binary links, and what it says about its licence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Dependency {
    /// The crate's name.
    pub name: String,
    /// The version resolved by `Cargo.lock`.
    pub version: String,
    /// The SPDX expression the crate declares, where it declares one.
    pub license: Option<String>,
    /// The licence documents it ships, as `(file name, text)`, sorted by name.
    pub texts: Vec<(String, String)>,
}

/// What went wrong while reading the graph. Every one of these means the notices cannot be
/// generated, which is a red gate rather than an empty file.
#[derive(Debug)]
pub enum NoticesError {
    /// `cargo metadata` could not be run or did not answer.
    Metadata(String),
    /// The answer was not the shape this reads.
    Shape(String),
    /// The file could not be written.
    Write(String),
}

impl std::fmt::Display for NoticesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NoticesError::Metadata(detail) => write!(f, "cargo metadata: {detail}"),
            NoticesError::Shape(detail) => write!(f, "the dependency graph: {detail}"),
            NoticesError::Write(detail) => write!(f, "writing {NOTICES_FILE}: {detail}"),
        }
    }
}

impl std::error::Error for NoticesError {}

/// Every crate the shipped binary links, sorted by name and version.
///
/// The graph is walked from `ono-cli` through normal and build dependencies and never through
/// dev-dependencies: a test harness is not distributed. It is *not* filtered by platform, so the
/// answer is the union over every target the lockfile resolves — a superset of what any one build
/// links, which is the safe direction for a notice and the only one that gives the same file on
/// every architecture we release for.
///
/// # Errors
///
/// When `cargo metadata` cannot be run, or answers something this cannot read.
pub fn dependencies(root: &Path) -> Result<Vec<Dependency>, NoticesError> {
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()))
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(root)
        .output()
        .map_err(|error| NoticesError::Metadata(error.to_string()))?;
    if !output.status.success() {
        return Err(NoticesError::Metadata(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| NoticesError::Shape(error.to_string()))?;
    collect(&metadata)
}

/// The crates of `metadata`'s graph, read without touching the network or a registry index.
fn collect(metadata: &serde_json::Value) -> Result<Vec<Dependency>, NoticesError> {
    let shape = |detail: &str| NoticesError::Shape(detail.to_owned());
    let packages = metadata
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| shape("no `packages`"))?;
    let members: Vec<&str> = metadata
        .get("workspace_members")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| shape("no `workspace_members`"))?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    let nodes = metadata
        .get("resolve")
        .and_then(|resolve| resolve.get("nodes"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| shape("no `resolve.nodes`"))?;

    let by_id: BTreeMap<&str, &serde_json::Value> = packages
        .iter()
        .filter_map(|package| Some((package.get("id")?.as_str()?, package)))
        .collect();
    let edges: BTreeMap<&str, &serde_json::Value> = nodes
        .iter()
        .filter_map(|node| Some((node.get("id")?.as_str()?, node)))
        .collect();

    let root = members
        .iter()
        .find(|id| {
            by_id
                .get(*id)
                .and_then(|package| package.get("name"))
                .and_then(serde_json::Value::as_str)
                == Some(SHIPPED)
        })
        .ok_or_else(|| shape(&format!("`{SHIPPED}` is not a workspace member")))?;

    // Reachable through what is linked: a normal dependency, or a build dependency whose code
    // reaches the binary through what it generates. A dev-dependency is not distributed.
    let mut reached: Vec<&str> = Vec::new();
    let mut stack = vec![*root];
    while let Some(id) = stack.pop() {
        if reached.contains(&id) {
            continue;
        }
        reached.push(id);
        let Some(node) = edges.get(id) else {
            continue;
        };
        let Some(deps) = node.get("deps").and_then(serde_json::Value::as_array) else {
            continue;
        };
        for dep in deps {
            let linked = dep
                .get("dep_kinds")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|kinds| {
                    kinds.iter().any(|kind| {
                        !matches!(
                            kind.get("kind").and_then(serde_json::Value::as_str),
                            Some("dev")
                        )
                    })
                });
            if let Some(next) = dep.get("pkg").and_then(serde_json::Value::as_str)
                && linked
            {
                stack.push(next);
            }
        }
    }

    let mut dependencies: Vec<Dependency> = reached
        .into_iter()
        .filter(|id| !members.contains(id))
        .filter_map(|id| {
            let package = by_id.get(id)?;
            let manifest = package.get("manifest_path")?.as_str()?;
            Some(Dependency {
                name: package.get("name")?.as_str()?.to_owned(),
                version: package.get("version")?.as_str()?.to_owned(),
                license: package
                    .get("license")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                texts: licence_texts(Path::new(manifest).parent().unwrap_or(Path::new("."))),
            })
        })
        .collect();
    dependencies.sort();
    Ok(dependencies)
}

/// Every licence document a crate ships, by file name.
///
/// A crate keeps them beside its manifest — `LICENSE`, `LICENSE-MIT`, `COPYING`, `NOTICE` — and
/// a few keep them in a `LICENSES/` directory, so that is read one level deep. Nothing else about
/// the crate's source is read: this is about what it says of itself.
fn licence_texts(directory: &Path) -> Vec<(String, String)> {
    fn names(entry: &std::fs::DirEntry) -> Option<String> {
        let name = entry.file_name().to_string_lossy().into_owned();
        let upper = name.to_uppercase();
        ["LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE"]
            .iter()
            .any(|prefix| upper.starts_with(prefix))
            .then_some(name)
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = names(&entry) else {
            continue;
        };
        let path = entry.path();
        if path.is_dir() {
            if let Ok(inner) = std::fs::read_dir(&path) {
                for file in inner.flatten() {
                    if let Ok(text) = std::fs::read_to_string(file.path()) {
                        found.push((
                            format!("{name}/{}", file.file_name().to_string_lossy()),
                            text,
                        ));
                    }
                }
            }
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            found.push((name, text));
        }
    }
    found.sort();
    found
}

/// The notices file, from the crates the binary links.
#[must_use]
pub fn render(dependencies: &[Dependency]) -> String {
    // One text may be shipped by two hundred crates; it is reproduced once, under the names the
    // crates give it, and the groups are ordered by the first crate that carries each — so the
    // file is stable as long as the lockfile is.
    let mut groups: Vec<(Vec<String>, Vec<String>, String)> = Vec::new();
    let mut index: BTreeMap<&str, usize> = BTreeMap::new();
    for dependency in dependencies {
        for (name, text) in &dependency.texts {
            let at = match index.get(text.as_str()) {
                Some(at) => *at,
                None => {
                    groups.push((Vec::new(), Vec::new(), text.clone()));
                    index.insert(text.as_str(), groups.len() - 1);
                    groups.len() - 1
                }
            };
            let (names, carriers, _) = &mut groups[at];
            if !names.contains(name) {
                names.push(name.clone());
            }
            carriers.push(format!("{} {}", dependency.name, dependency.version));
        }
    }
    let silent: Vec<&Dependency> = dependencies
        .iter()
        .filter(|dependency| dependency.texts.is_empty())
        .collect();

    let mut out = String::new();
    out.push_str("Ono-Sendai — third-party licences\n");
    out.push_str("=================================\n\n");
    out.push_str(
        "The `ono` binary links the crates listed below. This file reproduces what each of them\n\
         ships about its own licence, because MIT asks that its permission notice travel with the\n\
         software, Apache-2.0 §4 that its text travel with a derivative work, and\n\
         CDLA-Permissive-2.0 §2.1 that its text travel with the data. Ono-Sendai's own crates are\n\
         not here: they are under `LICENSE` beside this file.\n\n",
    );
    out.push_str(
        "It is generated from `Cargo.lock` — `cargo run -p xtask -- licenses --write` — and\n\
         `spec-check` regenerates it and compares, so it cannot fall behind a dependency change\n\
         (ADR-0608). What it covers is every crate reachable from `ono-cli` as a normal or a\n\
         build dependency, over every platform the lockfile resolves: a superset of what any one\n\
         build links, which is the safe direction for a notice. Test-only dependencies are absent,\n\
         because they are not distributed.\n\n",
    );
    out.push_str(&format!(
        "{} crates, carrying {} distinct licence documents.\n",
        dependencies.len(),
        groups.len()
    ));

    out.push_str("\n\n1. The crates\n-------------\n\n");
    for dependency in dependencies {
        out.push_str(&format!(
            "{} {}  —  {}\n",
            dependency.name,
            dependency.version,
            dependency
                .license
                .as_deref()
                .unwrap_or("no SPDX expression declared"),
        ));
    }

    if !silent.is_empty() {
        out.push_str("\n\n2. The crates that ship no licence document\n");
        out.push_str("------------------------------------------\n\n");
        out.push_str(
            "Upstream ships no licence file with these. What stands for them is the expression\n\
             they declare in their own manifest, reproduced here rather than passed over.\n\n",
        );
        for dependency in silent {
            out.push_str(&format!(
                "{} {}  —  {}\n",
                dependency.name,
                dependency.version,
                dependency
                    .license
                    .as_deref()
                    .unwrap_or("no SPDX expression declared"),
            ));
        }
    }

    out.push_str("\n\n3. The licence documents\n------------------------\n\n");
    out.push_str("Each is reproduced once, under the crates that ship it.\n");
    for (names, carriers, text) in &groups {
        out.push_str("\n\n");
        out.push_str(&"-".repeat(78));
        out.push('\n');
        out.push_str(&format!("{}\n", names.join(", ")));
        out.push_str("carried by: ");
        let mut carried: Vec<&String> = carriers.iter().collect();
        carried.sort();
        carried.dedup();
        out.push_str(
            &carried
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push('\n');
        out.push_str(&"-".repeat(78));
        out.push_str("\n\n");
        out.push_str(text.trim_end());
        out.push('\n');
    }
    out
}

/// Writes the notices, and says whether the file changed.
///
/// # Errors
///
/// When the graph cannot be read, or the file cannot be written.
pub fn write(root: &Path) -> Result<bool, NoticesError> {
    let rendered = render(&dependencies(root)?);
    let path = notices_path(root);
    if std::fs::read_to_string(&path).is_ok_and(|committed| committed == rendered) {
        return Ok(false);
    }
    std::fs::write(&path, rendered).map_err(|error| NoticesError::Write(error.to_string()))?;
    Ok(true)
}

/// Where the notices are.
#[must_use]
pub fn notices_path(root: &Path) -> PathBuf {
    root.join(NOTICES_FILE)
}

/// The committed notices against the lockfile: absent, stale, or in agreement.
///
/// A failure to read the graph is itself a problem rather than a pass. The notices are what the
/// project distributes with the binary, and "we could not tell" is not an answer to give about
/// that.
#[must_use]
pub fn check_committed(root: &Path) -> Vec<Problem> {
    let dependencies = match dependencies(root) {
        Ok(dependencies) => dependencies,
        Err(error) => {
            return vec![Problem::new(
                NOTICES_FILE,
                format!(
                    "cannot be held against the lockfile: {error}. The notices ship with the \
                     binary, so a graph nobody could read is a red gate rather than a pass \
                     (ADR-0608)"
                ),
            )];
        }
    };
    let rendered = render(&dependencies);
    match std::fs::read_to_string(notices_path(root)) {
        Ok(committed) if committed == rendered => Vec::new(),
        Ok(_) => vec![Problem::new(
            NOTICES_FILE,
            "does not match the crates `Cargo.lock` resolves; run \
             `cargo run -p xtask -- licenses --write`. A notice file that has fallen behind the \
             graph says something untrue about what is being distributed (ADR-0608)"
                .to_owned(),
        )],
        Err(_) => vec![Problem::new(
            NOTICES_FILE,
            "does not exist, and the binary ships licences that ask for their text to travel \
             with it; run `cargo run -p xtask -- licenses --write` (ADR-0608)"
                .to_owned(),
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dependency(name: &str, license: Option<&str>, texts: &[(&str, &str)]) -> Dependency {
        Dependency {
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            license: license.map(str::to_owned),
            texts: texts
                .iter()
                .map(|(file, text)| ((*file).to_owned(), (*text).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn should_reproduce_one_shared_text_once_under_every_crate_that_ships_it() {
        // Two hundred crates ship the same Apache text; a file that repeated it two hundred
        // times would be unreadable and no more complete.
        let rendered = render(&[
            dependency(
                "alpha",
                Some("Apache-2.0"),
                &[("LICENSE", "the same words")],
            ),
            dependency("beta", Some("Apache-2.0"), &[("LICENSE", "the same words")]),
        ]);
        assert_eq!(
            rendered.matches("the same words").count(),
            1,
            "the text appears once: {rendered}"
        );
        assert!(
            rendered.contains("carried by: alpha 1.0.0, beta 1.0.0"),
            "and names both crates: {rendered}"
        );
        assert!(rendered.contains("carrying 1 distinct licence documents"));
    }

    #[test]
    fn should_say_when_a_crate_ships_no_text_rather_than_pass_over_it() {
        // The difference between "we left it out" and "upstream ships none" is the whole value
        // of the file for a reader checking an obligation.
        let rendered = render(&[
            dependency("quiet", Some("MIT"), &[]),
            dependency("loud", Some("MIT"), &[("LICENSE", "words")]),
        ]);
        assert!(
            rendered.contains("2. The crates that ship no licence document"),
            "{rendered}"
        );
        let section = rendered
            .split("2. The crates that ship no licence document")
            .nth(1)
            .expect("the section");
        assert!(section.contains("quiet 1.0.0  —  MIT"), "{section}");
        assert!(
            !section
                .split("3. The licence documents")
                .next()
                .unwrap_or_default()
                .contains("loud"),
            "a crate that ships a text is not listed as silent: {section}"
        );
    }

    #[test]
    fn should_render_a_crate_that_declares_no_expression_as_declaring_none() {
        let rendered = render(&[dependency("bare", None, &[])]);
        assert!(
            rendered.contains("bare 1.0.0  —  no SPDX expression declared"),
            "nothing is invented for a crate that declares nothing: {rendered}"
        );
    }

    #[test]
    fn should_render_the_same_bytes_for_the_same_graph() {
        // The file is compared against a regeneration on every gate run, so two renderings of
        // one graph must not differ by so much as a byte.
        let graph = [
            dependency("beta", Some("MIT"), &[("LICENSE", "b")]),
            dependency("alpha", Some("MIT"), &[("LICENSE-MIT", "a")]),
        ];
        assert_eq!(render(&graph), render(&graph));
    }

    #[test]
    fn should_walk_past_a_dev_dependency_and_through_a_build_one() {
        // What is distributed is what the binary links: a test harness is not, and a build
        // dependency's generated code is.
        let metadata = serde_json::json!({
            "packages": [
                {"id": "ono-cli 0.0.0", "name": "ono-cli", "version": "0.0.0", "manifest_path": "/nowhere/Cargo.toml"},
                {"id": "linked 1.0.0", "name": "linked", "version": "1.0.0", "license": "MIT", "manifest_path": "/nowhere/linked/Cargo.toml"},
                {"id": "built 1.0.0", "name": "built", "version": "1.0.0", "license": "MIT", "manifest_path": "/nowhere/built/Cargo.toml"},
                {"id": "tested 1.0.0", "name": "tested", "version": "1.0.0", "license": "MIT", "manifest_path": "/nowhere/tested/Cargo.toml"}
            ],
            "workspace_members": ["ono-cli 0.0.0"],
            "resolve": {"nodes": [
                {"id": "ono-cli 0.0.0", "deps": [
                    {"pkg": "linked 1.0.0", "dep_kinds": [{"kind": null}]},
                    {"pkg": "built 1.0.0", "dep_kinds": [{"kind": "build"}]},
                    {"pkg": "tested 1.0.0", "dep_kinds": [{"kind": "dev"}]}
                ]},
                {"id": "linked 1.0.0", "deps": []},
                {"id": "built 1.0.0", "deps": []},
                {"id": "tested 1.0.0", "deps": []}
            ]}
        });
        let names: Vec<String> = collect(&metadata)
            .expect("the graph reads")
            .into_iter()
            .map(|dependency| dependency.name)
            .collect();
        assert_eq!(names, vec!["built".to_owned(), "linked".to_owned()]);
    }
}
