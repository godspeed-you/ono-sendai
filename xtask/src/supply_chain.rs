//! Supply-chain pins for everything CI pulls from outside this repository.
//!
//! Spec §43 and §44 come down to one property: a release must be buildable from inputs that
//! cannot change after the source is tagged. A `uses: actions/checkout@v4` and a
//! `FROM fedora:latest` both resolve to *whatever that name means today*, so the same commit
//! built twice is two different builds, and a third party who moves a tag has run code inside a
//! job holding this repository's token (spec §5.2, attacker classes 9 and 10).
//!
//! The pins themselves are one-line edits. What makes them stick is that the gate refuses the
//! unpinned form, so a floating reference cannot be reintroduced by a hurried edit and noticed
//! by nobody — which is exactly how the current ones arrived.
//!
//! Three rules live here, and they are deliberately separate functions: each reports what it
//! alone can prove, so a red gate names the file, the line and the reference rather than "the
//! supply chain is wrong".

use std::path::{Path, PathBuf};

use serde_yaml_ng::Value;

use crate::scan::Problem;

fn problem(location: impl Into<String>, detail: impl Into<String>) -> Problem {
    Problem {
        location: location.into(),
        detail: detail.into(),
    }
}

/// One reference to something outside this repository, and where it is written.
///
/// The pin scanners and the release input manifest read the same references from the same files
/// (ADR-0451): a manifest assembled from a second reading could disagree with the gate that
/// approved it, and then neither is evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// The file it is written in, relative to the repository root.
    pub file: String,
    /// The line it is written on, counted from one.
    pub line: usize,
    /// The reference itself.
    pub value: String,
    /// The trailing comment, which is where a pinned reference keeps its human-readable version.
    pub comment: Option<String>,
}

// --- third-party actions (spec §43.1, §62.1) ----------------------------------------------------

/// Checks that every third-party action is referenced by a full commit SHA (spec §43.1).
///
/// A tag and a branch are both names their owner can repoint. `actions/checkout@v4` is a moving
/// target by design, and the same is true of `dtolnay/rust-toolchain@stable`; pinning the commit
/// is what turns "we run the action we reviewed" from a hope into a fact.
///
/// The conventional form keeps the human-readable version in a trailing comment, and the check
/// accepts it — the comment is the only thing that makes a deliberate bump reviewable:
///
/// ```yaml
/// uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0
/// ```
///
/// Actions that live in this repository (`./.github/actions/…`) are exempt, as spec §62.1 says:
/// they are already pinned by being the commit under test.
#[must_use]
pub fn check_action_pins(root: &Path) -> Vec<Problem> {
    let mut problems: Vec<Problem> = action_references(root)
        .into_iter()
        .filter(|reference| {
            !reference
                .value
                .rsplit_once('@')
                .is_some_and(|(_, git_ref)| is_commit_sha(git_ref))
        })
        .map(|reference| {
            problem(
                format!("{}:{}", reference.file, reference.line),
                format!(
                    "uses `{}`, which is a name its owner can repoint at any commit. \
                     Pin the full commit SHA and keep the version in a trailing comment — \
                     `uses: owner/action@<40-hex> # v1.2.3` — resolved with \
                     `gh api repos/owner/action/commits/<tag> --jq .sha` (spec §43.1, §62.1)",
                    reference.value
                ),
            )
        })
        .collect();
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// Every third-party action this repository runs, in file order.
///
/// Actions that live in this repository (`./.github/actions/…`) are not references to anything
/// outside it and are left out, as spec §62.1 says: they are pinned by being the commit under
/// test.
#[must_use]
pub fn action_references(root: &Path) -> Vec<Reference> {
    let mut references = Vec::new();
    for file in action_definitions(root) {
        let location = relative(root, &file);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let Some((value, comment)) = uses_reference(line) else {
                continue;
            };
            if is_repository_local_action(&value) {
                continue;
            }
            references.push(Reference {
                file: location.clone(),
                line: index + 1,
                value,
                comment,
            });
        }
    }
    references
}

/// The `uses:` value of one line, with any trailing comment and quoting removed.
///
/// Comments are skipped rather than parsed: a line that only *mentions* a reference — a note
/// about the version that was replaced — is prose, and reporting it would teach people to stop
/// writing the note.
fn uses_reference(line: &str) -> Option<(String, Option<String>)> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    let after_dash = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim_start();
    let rest = after_dash.strip_prefix("uses:")?;
    let (value, comment) = match rest.split_once(" #") {
        Some((value, comment)) => (value, Some(comment.trim().to_owned())),
        None => (rest, None),
    };
    let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
    (!value.is_empty()).then(|| (value.to_owned(), comment))
}

/// Whether a reference points at an action stored in this repository.
fn is_repository_local_action(reference: &str) -> bool {
    reference.starts_with("./") || reference.starts_with(".\\")
}

/// Whether a git ref is a full commit object id rather than a tag or a branch.
fn is_commit_sha(git_ref: &str) -> bool {
    matches!(git_ref.len(), 40 | 64) && git_ref.chars().all(|c| c.is_ascii_hexdigit())
}

/// Every file that can carry a `uses:` — the workflows and any composite action beside them.
fn action_definitions(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let github = root.join(".github");
    collect(&github.join("workflows"), &is_yaml, &mut files);
    collect(&github.join("actions"), &is_yaml, &mut files);
    files.sort();
    files
}

// --- container images (spec §44.1, §62.2) -------------------------------------------------------

/// Images this repository builds itself, and therefore never pulls from a registry.
const LOCAL_IMAGE_PREFIX: &str = "ono-sendai:";

/// Checks that every container image the repository pulls is pinned by digest (spec §44.1).
///
/// A tag is a pointer the publisher owns. `fedora:latest` is a different operating system every
/// few weeks, and `rust:1.94-slim-bookworm` is rebuilt whenever its base moves — so a package
/// built today and the "same" package rebuilt next month share a version number and nothing
/// else. The digest is the only reference that names one image.
///
/// The tag stays readable beside it, because `name:tag@sha256:…` is a legal pull reference and
/// the tag is what tells a reviewer what the digest is supposed to be:
///
/// ```text
/// FROM rust:1.94-slim-bookworm@sha256:cf9dd0…
/// ```
///
/// Three references are not registry pulls and are skipped: a later Dockerfile stage naming an
/// earlier one, an image this repository builds itself (`ono-sendai:…`), and a reference that is
/// only a shell expansion, which is pinned wherever the variable is set.
///
/// There is no allowlist. Spec §62.2 permits one for test-only convenience images, and this
/// repository has none that it pulls — the demo images are its own build output — so an
/// allowlist would only be a hole nobody was using.
#[must_use]
pub fn check_image_digests(root: &Path) -> Vec<Problem> {
    let mut problems: Vec<Problem> = image_references(root)
        .into_iter()
        .filter(|reference| !has_digest(&reference.value))
        .map(|reference| {
            let image = &reference.value;
            problem(
                format!("{}:{}", reference.file, reference.line),
                format!(
                    "pulls `{image}`, a tag its publisher can repoint, so the same commit \
                     builds a different artifact tomorrow. Append the digest and keep the tag \
                     readable — `{image}@sha256:<64-hex>` — resolved with \
                     `docker manifest inspect {image}` (spec §44.1, §62.2)"
                ),
            )
        })
        .collect();
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// Every container image this repository pulls from a registry, in file order.
///
/// Three references are not registry pulls and are left out: a later Dockerfile stage naming an
/// earlier one, an image this repository builds itself (`ono-sendai:…`), and a reference that is
/// only a shell expansion, which is pinned wherever the variable is set.
#[must_use]
pub fn image_references(root: &Path) -> Vec<Reference> {
    let mut references = Vec::new();
    for file in image_sources(root) {
        let location = relative(root, &file);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let found = if is_dockerfile(&file) {
            dockerfile_images(&text)
        } else if is_yaml(&file) {
            workflow_images(&text)
        } else {
            shell_images(&text)
        };
        for (line, value) in found {
            if value.contains('$') || value.starts_with(LOCAL_IMAGE_PREFIX) {
                continue;
            }
            references.push(Reference {
                file: location.clone(),
                line,
                value,
                comment: None,
            });
        }
    }
    references
}

/// Whether a reference names one immutable image.
fn has_digest(reference: &str) -> bool {
    reference.split_once("@sha256:").is_some_and(|(_, digest)| {
        digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit())
    })
}

/// The base images of a Dockerfile, excluding stages that build on an earlier stage.
fn dockerfile_images(text: &str) -> Vec<(usize, String)> {
    let mut stages: Vec<String> = Vec::new();
    let mut images = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.to_ascii_lowercase().starts_with("from ") {
            continue;
        }
        let mut words = trimmed.split_whitespace().skip(1).peekable();
        while words.peek().is_some_and(|word| word.starts_with("--")) {
            words.next();
        }
        let Some(image) = words.next() else { continue };
        let mut rest = words;
        if rest
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("as"))
            && let Some(name) = rest.next()
        {
            stages.push(name.to_owned());
        }
        if stages.iter().any(|stage| stage == image) {
            continue;
        }
        images.push((index + 1, image.to_owned()));
    }
    images
}

/// The images a shell script names, taken from the variables it keeps them in.
///
/// The scripts of this repository all follow the same shape — `FEDORA_IMAGE="${OVERRIDE:-ref}"` —
/// and that shape is what the scan reads. A script that instead inlined the reference into a
/// `docker run` would slip past, which is a limit worth naming: the fix is to keep the
/// convention, not to make the scanner guess at every word of every command line.
fn shell_images(text: &str) -> Vec<(usize, String)> {
    let mut images = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        let assignment = ["export ", "readonly ", "declare ", "local "]
            .iter()
            .fold(trimmed, |line, keyword| {
                line.strip_prefix(keyword).unwrap_or(line).trim_start()
            });
        let Some((name, value)) = assignment.split_once('=') else {
            continue;
        };
        if !name.contains("IMAGE")
            || !name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        {
            continue;
        }
        let value = shell_value(value);
        // `KEEP_IMAGE=0` names an image in the way `KEEP_TMPDIR=0` names a directory: the
        // variable is a flag about one, and its value is a number rather than a reference.
        if value.parse::<i64>().is_ok() {
            continue;
        }
        images.push((index + 1, value));
    }
    images
}

/// The literal an assignment carries, unwrapping `"${OVERRIDE:-literal}"`.
fn shell_value(value: &str) -> String {
    let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
    let Some(inner) = value.strip_prefix("${").and_then(|v| v.strip_suffix('}')) else {
        return value.to_owned();
    };
    inner
        .split_once(":-")
        .map_or_else(|| value.to_owned(), |(_, default)| default.to_owned())
}

/// The container images a workflow runs a job or a service in.
fn workflow_images(text: &str) -> Vec<(usize, String)> {
    let mut images = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        for key in ["image:", "container:"] {
            let Some(value) = trimmed.strip_prefix(key) else {
                continue;
            };
            let value = value.split(" #").next().unwrap_or(value).trim();
            let value = value.trim_matches(|c| c == '"' || c == '\'');
            if !value.is_empty() {
                images.push((index + 1, value.to_owned()));
            }
        }
    }
    images
}

/// Every file that can name a container image: the Dockerfiles, the scripts, the workflows.
fn image_sources(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect(&root.join("docker"), &is_dockerfile, &mut files);
    // A Dockerfile at the top level too, without walking the whole repository to find one: the
    // trees below it hold source, not build recipes.
    if let Ok(entries) = std::fs::read_dir(root) {
        files.extend(
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file() && is_dockerfile(path)),
        );
    }
    collect(&root.join("scripts"), &is_shell, &mut files);
    collect(
        &root.join(".github").join("workflows"),
        &is_yaml,
        &mut files,
    );
    files.sort();
    files.dedup();
    files
}

// --- permissions and untrusted pull requests (spec §43.3, §43.4, §43.5) -------------------------

/// Checks least privilege and untrusted-pull-request isolation in every workflow.
///
/// "Untrusted" here means one thing: a run whose code came from outside the set of people who
/// can push to this repository — in practice a `pull_request` from a fork. GitHub already gives
/// such a run a read-only token and no secrets, and the rules below are what keep that true
/// after the next edit rather than by luck:
///
/// * every workflow declares `permissions:`, so no job silently inherits the repository default;
/// * the workflow-level block grants no write scope — `contents: write` belongs on the
///   publishing job and nowhere else (spec §43.3);
/// * `pull_request_target` is not used at all. Its entire purpose is to run against an untrusted
///   head with the base repository's token, which is the shape spec §43.4 forbids, and this
///   repository has no need it would serve;
/// * a workflow a pull request can start references no secret but the automatic `GITHUB_TOKEN`
///   (spec §43.4);
/// * a workflow with a write-granting job is not reachable from a pull request at all, so the
///   release path cannot be entered by proposing a change to it;
/// * a workflow that publishes declares `concurrency:`, so two runs cannot race to attach
///   conflicting artifacts to one tag (spec §43.5).
#[must_use]
pub fn check_workflow_permissions(root: &Path) -> Vec<Problem> {
    let mut files = Vec::new();
    collect(
        &root.join(".github").join("workflows"),
        &is_yaml,
        &mut files,
    );
    files.sort();

    let mut problems = Vec::new();
    for file in files {
        let location = relative(root, &file);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let workflow: Value = match serde_yaml_ng::from_str(&text) {
            Ok(value) => value,
            Err(error) => {
                problems.push(problem(
                    location.clone(),
                    format!("is not valid YAML, so nothing can check what it grants: {error}"),
                ));
                continue;
            }
        };
        problems.extend(check_one_workflow(&location, &text, &workflow));
    }
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

fn check_one_workflow(location: &str, text: &str, workflow: &Value) -> Vec<Problem> {
    let mut problems = Vec::new();
    let triggers = triggers(workflow);
    let untrusted = triggers
        .iter()
        .any(|event| event.starts_with("pull_request"));

    if triggers.iter().any(|event| event == "pull_request_target") {
        problems.push(problem(
            location,
            "is triggered by `pull_request_target`, which runs with the base repository's token \
             while the pull request supplies the code. Trigger on `pull_request` instead and \
             accept the read-only token that comes with it (spec §43.4)",
        ));
    }

    let top_level = workflow.get("permissions");
    match top_level {
        None => problems.push(problem(
            location,
            "declares no `permissions:`, so every job runs with whatever the repository hands \
             out by default. Declare `permissions: contents: read` at the top and widen it on \
             the one job that needs more (spec §43.3)",
        )),
        Some(permissions) => {
            for scope in write_scopes(permissions) {
                problems.push(problem(
                    location,
                    format!(
                        "grants `{scope}` to every job in the workflow. A write scope belongs on \
                         the publishing job alone; the workflow default is `contents: read` \
                         (spec §43.3)"
                    ),
                ));
            }
        }
    }

    if untrusted {
        for (line, secret) in foreign_secrets(text) {
            problems.push(problem(
                format!("{location}:{line}"),
                format!(
                    "reads `secrets.{secret}` in a workflow a pull request can start. A run \
                     proposed from outside must see no privileged secret — move the step to a \
                     workflow only a push or a tag can reach (spec §43.4)"
                ),
            ));
        }
    }

    for (job, permissions) in write_granting_jobs(workflow) {
        if untrusted {
            problems.push(problem(
                location,
                format!(
                    "job `{job}` holds `{permissions}` in a workflow a pull request can start, so \
                     the release path is reachable by proposing a change to it. Keep publishing \
                     in a workflow triggered only by a tag push (spec §43.4)"
                ),
            ));
        }
        if workflow.get("concurrency").is_none() && job_concurrency(workflow, &job).is_none() {
            problems.push(problem(
                location,
                format!(
                    "job `{job}` publishes with `{permissions}` and no `concurrency:` guard, so \
                     two runs can attach conflicting artifacts to the same tag (spec §43.5)"
                ),
            ));
        }
    }

    problems
}

/// The events a workflow reacts to.
///
/// `on` is looked up twice because it is the one key whose name a YAML parser may resolve for
/// itself: under YAML 1.1 the bare word is the boolean `true`, and a workflow whose trigger list
/// silently read as empty would pass every rule below.
fn triggers(workflow: &Value) -> Vec<String> {
    let events = workflow
        .get("on")
        .or_else(|| workflow.get(Value::Bool(true)));
    match events {
        Some(Value::String(event)) => vec![event.clone()],
        Some(Value::Sequence(events)) => events
            .iter()
            .filter_map(|event| event.as_str().map(str::to_owned))
            .collect(),
        Some(Value::Mapping(events)) => events
            .keys()
            .filter_map(|event| event.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// The scopes a `permissions:` block grants at the `write` level.
fn write_scopes(permissions: &Value) -> Vec<String> {
    match permissions {
        Value::String(all) if all == "write-all" => vec!["write-all".to_owned()],
        Value::Mapping(scopes) => scopes
            .iter()
            .filter(|(_, level)| level.as_str() == Some("write"))
            .filter_map(|(scope, _)| scope.as_str().map(|scope| format!("{scope}: write")))
            .collect(),
        _ => Vec::new(),
    }
}

/// The jobs that declare a write scope of their own, with the widest one they grant.
fn write_granting_jobs(workflow: &Value) -> Vec<(String, String)> {
    let Some(Value::Mapping(jobs)) = workflow.get("jobs") else {
        return Vec::new();
    };
    jobs.iter()
        .filter_map(|(name, job)| {
            let name = name.as_str()?;
            let scopes = write_scopes(job.get("permissions")?);
            scopes.first().map(|scope| (name.to_owned(), scope.clone()))
        })
        .collect()
}

/// The `concurrency:` block of one job, when it has one of its own.
fn job_concurrency<'a>(workflow: &'a Value, job: &str) -> Option<&'a Value> {
    workflow.get("jobs")?.get(job)?.get("concurrency")
}

/// Every secret a workflow reads apart from the token GitHub issues to the run itself.
///
/// `GITHUB_TOKEN` is exempt because it is not a stored credential: it is minted per run and
/// scoped by the `permissions:` block the rules above already constrain.
fn foreign_secrets(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for (at, _) in line.match_indices("secrets.") {
            let name: String = line[at + "secrets.".len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() || name == "GITHUB_TOKEN" {
                continue;
            }
            found.push((index + 1, name));
        }
    }
    found
}

// --- shared file walking ------------------------------------------------------------------------

fn is_yaml(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext == "yml" || ext == "yaml")
}

fn is_shell(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "sh")
}

fn is_dockerfile(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|name| name.to_string_lossy()) else {
        return false;
    };
    name == "Dockerfile" || name.starts_with("Dockerfile.") || name.ends_with(".Dockerfile")
}

/// Every file under `dir` that `wanted` accepts, excluding build output.
fn collect(dir: &Path, wanted: &dyn Fn(&Path) -> bool, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "target" || name == "dist" || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect(&path, wanted, files);
        } else if wanted(&path) {
            files.push(path);
        }
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

// --- dependency policy (spec §45.1, §45.2, §62.3) -----------------------------------------------

/// The policy file `cargo deny` reads.
const POLICY_FILE: &str = "deny.toml";

/// The four questions spec §45.2 asks a policy tool to answer.
const REQUIRED_CHECKS: &[&str] = &["advisories", "licenses", "bans", "sources"];

/// Checks that a dependency policy exists, answers all four questions, and is run by the gate.
///
/// Spec §45.1 wants the advisory check *in the gate*, and §45.2 wants licences, bans, advisories
/// and sources validated by a policy tool. `cargo deny` is that tool here, and the rules below
/// are about the things the tool cannot check about itself: whether it is configured for all
/// four checks, whether an ignored advisory has a way of ever being un-ignored, and whether
/// anything actually runs it. A policy file nobody invokes is a document, not a control.
///
/// The rule on `[advisories] ignore` is the one worth stating twice: spec §45.1 allows a known
/// vulnerability to be waived only when a note says why the vulnerable path is unreachable *and*
/// defines a removal deadline, so a bare advisory id is refused and the tabular form with
/// `reason` and `expiration` is required. cargo-deny then expires the waiver by itself, which
/// turns "we will look at that later" into a date the gate enforces.
#[must_use]
pub fn check_dependency_policy(root: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    let Ok(text) = std::fs::read_to_string(root.join(POLICY_FILE)) else {
        return vec![problem(
            POLICY_FILE,
            "is missing, so nothing states which licences, sources and advisories this \
             repository accepts. Write it and run `cargo deny check` from the gate (spec §45.2)",
        )];
    };
    let policy: toml::Value = match toml::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            return vec![problem(
                POLICY_FILE,
                format!("is not valid TOML, so `cargo deny` cannot read it: {error}"),
            )];
        }
    };

    for check in REQUIRED_CHECKS {
        if policy.get(check).is_none() {
            problems.push(problem(
                POLICY_FILE,
                format!(
                    "configures no `[{check}]`, so `cargo deny check` decides that question by \
                     its own defaults rather than by a rule this repository wrote down \
                     (spec §45.2)"
                ),
            ));
        }
    }

    if let Some(licenses) = policy.get("licenses") {
        let allowed = licenses
            .get("allow")
            .and_then(toml::Value::as_array)
            .is_some_and(|allow| !allow.is_empty());
        if !allowed {
            problems.push(problem(
                POLICY_FILE,
                "allows no licence by name. `[licenses] allow` is the whole policy: an empty \
                 list either refuses everything or, worse, is read as no opinion at all \
                 (spec §45.2)",
            ));
        }
    }

    if let Some(sources) = policy.get("sources") {
        for key in ["unknown-registry", "unknown-git"] {
            if sources.get(key).and_then(toml::Value::as_str) != Some("deny") {
                problems.push(problem(
                    POLICY_FILE,
                    format!(
                        "does not set `[sources] {key} = \"deny\"`, so a dependency could arrive \
                         from somewhere this repository never approved (spec §45.2, §45.3)"
                    ),
                ));
            }
        }
    }

    problems.extend(check_ignored_advisories(&policy));

    let gate = std::fs::read_to_string(root.join("scripts").join("gate.sh")).unwrap_or_default();
    if !gate.contains("cargo deny") {
        problems.push(problem(
            "scripts/gate.sh",
            "runs no `cargo deny check`, so the dependency policy is a file nobody consults. \
             Spec §45.1 puts the advisory check in the gate, not in a review (spec §45.1, §62.3)",
        ));
    }

    problems
}

/// Every waived advisory must say why it is safe and when the waiver dies (spec §45.1).
///
/// cargo-deny's own waiver carries an id and a reason and nothing else, so the deadline lives
/// inside the reason in a form this scan reads — `…, expires 2027-03-01` — and the scan refuses
/// a waiver that has no deadline and a waiver whose deadline has passed. That makes the gate
/// turn red on a date nobody has to remember, which is the only kind of deadline a repository
/// keeps.
fn check_ignored_advisories(policy: &toml::Value) -> Vec<Problem> {
    let Some(ignored) = policy
        .get("advisories")
        .and_then(|advisories| advisories.get("ignore"))
        .and_then(toml::Value::as_array)
    else {
        return Vec::new();
    };
    let today = today_as_days();
    ignored
        .iter()
        .filter_map(|entry| {
            let (id, reason) = match entry {
                toml::Value::String(id) => (id.as_str(), ""),
                toml::Value::Table(fields) => (
                    fields
                        .get("id")
                        .and_then(toml::Value::as_str)
                        .unwrap_or("with no id"),
                    fields
                        .get("reason")
                        .and_then(toml::Value::as_str)
                        .unwrap_or_default(),
                ),
                other => {
                    return Some(problem(
                        POLICY_FILE,
                        format!("waives `{other}`, which is neither an advisory id nor a waiver"),
                    ));
                }
            };
            match deadline(reason) {
                None => Some(problem(
                    POLICY_FILE,
                    format!(
                        "waives advisory {id} without saying why the vulnerable path is \
                         unreachable and when the waiver dies. Spec §45.1 accepts a known \
                         vulnerability only against a note and a removal deadline — \
                         `{{ id = \"{id}\", reason = \"… , expires YYYY-MM-DD\" }}`"
                    ),
                )),
                Some((date, days)) if days < today => Some(problem(
                    POLICY_FILE,
                    format!(
                        "waives advisory {id} until {date}, which has passed. Remove the \
                         dependency, or record a new decision and a new date — a deadline that \
                         renews itself silently is not one (spec §45.1)"
                    ),
                )),
                Some(_) => None,
            }
        })
        .collect()
}

/// The `expires YYYY-MM-DD` a waiver carries, as written and as a day number.
fn deadline(reason: &str) -> Option<(String, i64)> {
    let after = reason.split("expires ").nth(1)?;
    let date: String = after
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((date, days_from_civil(year, month, day)))
}

/// Today, as days since the Unix epoch.
fn today_as_days() -> i64 {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    i64::try_from(seconds / 86_400).unwrap_or(0)
}

/// Days from the Unix epoch to a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

// --- recorded justifications (spec §45.3, §45.4) ------------------------------------------------

/// Where the justifications live: `cargo` ignores `workspace.metadata`, and this scan does not.
const REGISTER: &str = "[workspace.metadata.supply-chain]";

/// Crate names that are, on their own, a cryptographic dependency (spec §45.4).
const CRYPTOGRAPHIC_NAMES: &[&str] = &[
    "aes",
    "age",
    "blake2",
    "blake3",
    "der",
    "digest",
    "md5",
    "orion",
    "p256",
    "p384",
    "pem",
    "rand",
    "rand_core",
    "ring",
    "rsa",
    "sec1",
    "sha1",
    "sha2",
    "sha3",
    "spki",
    "subtle",
    "zeroize",
];

/// Fragments of a crate name that mean the crate handles TLS, signing, hashing or keys.
///
/// Deliberately over-inclusive. A false positive costs one line in the register; a false negative
/// is a cryptographic dependency that arrived without anyone deciding to trust it.
const CRYPTOGRAPHIC_MARKERS: &[&str] = &[
    "aead",
    "argon2",
    "certificate",
    "chacha20",
    "cipher",
    "crypto",
    "curve25519",
    "dalek",
    "ecdsa",
    "ed25519",
    "hkdf",
    "hmac",
    "jsonwebtoken",
    "keychain",
    "openssl",
    "pbkdf2",
    "pkcs",
    "rcgen",
    "secrecy",
    "signature",
    "sodium",
    "tls",
    "webpki",
    "x509",
];

/// Whether spec §45.4 applies to a dependency of this name.
fn is_cryptographic(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    CRYPTOGRAPHIC_NAMES.contains(&lower.as_str())
        || CRYPTOGRAPHIC_MARKERS
            .iter()
            .any(|marker| lower.contains(marker))
}

/// One declared dependency, and where it was declared.
struct Declared {
    name: String,
    manifest: String,
    git: Option<toml::Value>,
}

/// Checks that every git and every cryptographic dependency carries a recorded justification.
///
/// Spec §45.3 forbids a production dependency that follows a branch or a tag, and §45.4 requires
/// an ADR or security review before a TLS, signing, hashing or key-handling crate is introduced.
/// Both are rules about a decision somebody made once, and both are invisible in the manifest
/// afterwards — `rustls = "0.23"` looks exactly like `regex = "1"`.
///
/// So the decision is written where the dependency is, in `Cargo.toml`, under the one table
/// cargo hands to other tools:
///
/// ```toml
/// [[workspace.metadata.supply-chain.cryptographic]]
/// crate = "rustls"
/// role = "the authenticated transport of spec §21.5"
/// adr = "ADR-0353"
/// reviewed = "2026-08-29"
/// ```
///
/// The register is checked in both directions. A dependency without an entry fails, and an entry
/// naming a dependency the workspace no longer has fails too — a register that keeps its dead
/// entries stops being a statement about what is here now.
///
/// A crate that writes `rustls.workspace = true` inherits a decision already recorded at the
/// workspace root, so it needs nothing of its own; a crate that names its own version or its own
/// git source is making the decision itself and is held to it.
#[must_use]
pub fn check_dependency_justifications(root: &Path) -> Vec<Problem> {
    let manifest_text = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    let workspace: toml::Value = match toml::from_str(&manifest_text) {
        Ok(value) => value,
        Err(error) => {
            return vec![problem("Cargo.toml", format!("is not valid TOML: {error}"))];
        }
    };
    let register = |kind: &str| -> Vec<toml::Value> {
        workspace
            .get("workspace")
            .and_then(|workspace| workspace.get("metadata"))
            .and_then(|metadata| metadata.get("supply-chain"))
            .and_then(|register| register.get(kind))
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let git_register = register("git");
    let crypto_register = register("cryptographic");

    let mut problems = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for declared in declared_dependencies(root) {
        if seen.contains(&declared.name) {
            continue;
        }
        seen.push(declared.name.clone());
        if let Some(source) = &declared.git {
            problems.extend(check_git_dependency(&declared, source, &git_register));
        } else if is_cryptographic(&declared.name) {
            problems.extend(check_cryptographic_dependency(&declared, &crypto_register));
        }
    }

    for (kind, entries) in [("git", &git_register), ("cryptographic", &crypto_register)] {
        for entry in entries {
            let named = entry.get("crate").and_then(toml::Value::as_str);
            match named {
                Some(name) if seen.iter().any(|declared| declared == name) => {}
                Some(name) => problems.push(problem(
                    "Cargo.toml",
                    format!(
                        "records a {kind} justification for `{name}`, which no manifest in this \
                         workspace depends on any more. Delete the entry — a register that keeps \
                         its dead entries stops describing what is here (spec §45.3, §45.4)"
                    ),
                )),
                None => problems.push(problem(
                    "Cargo.toml",
                    format!(
                        "records a {kind} justification with no `crate` field, so nothing says \
                         which dependency it justifies (spec §45.3, §45.4)"
                    ),
                )),
            }
        }
    }

    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// Spec §45.3: a git dependency names one commit and says why it is here.
fn check_git_dependency(
    declared: &Declared,
    source: &toml::Value,
    register: &[toml::Value],
) -> Vec<Problem> {
    let name = &declared.name;
    let mut problems = Vec::new();
    let revision = source.get("rev").and_then(toml::Value::as_str);
    if !revision.is_some_and(is_commit_sha) {
        let following = ["branch", "tag"]
            .iter()
            .find(|key| source.get(*key).is_some())
            .map_or("no revision at all", |key| *key);
        problems.push(problem(
            &declared.manifest,
            format!(
                "takes `{name}` from git and follows {following}, so the code it builds changes \
                 whenever its author pushes. Pin `rev = \"<40-hex>\"` (spec §45.3)"
            ),
        ));
    }
    problems.extend(missing_justification(
        declared,
        register,
        &["reason", "adr"],
        "git",
        "[[workspace.metadata.supply-chain.git]]\ncrate = \"…\"\nreason = \"…\"\nadr = \"ADR-…\"",
    ));
    problems
}

/// Spec §45.4: a crate that touches TLS, signing, hashing or keys arrives by decision.
fn check_cryptographic_dependency(declared: &Declared, register: &[toml::Value]) -> Vec<Problem> {
    missing_justification(
        declared,
        register,
        &["role", "adr", "reviewed"],
        "cryptographic",
        "[[workspace.metadata.supply-chain.cryptographic]]\ncrate = \"…\"\nrole = \"…\"\n\
         adr = \"ADR-…\"\nreviewed = \"YYYY-MM-DD\"",
    )
}

/// The register entry for one dependency, or what is missing from it.
fn missing_justification(
    declared: &Declared,
    register: &[toml::Value],
    required: &[&str],
    kind: &str,
    form: &str,
) -> Vec<Problem> {
    let name = &declared.name;
    let Some(entry) = register
        .iter()
        .find(|entry| entry.get("crate").and_then(toml::Value::as_str) == Some(name.as_str()))
    else {
        return vec![problem(
            &declared.manifest,
            format!(
                "depends on `{name}`, a {kind} dependency nobody recorded a justification for. \
                 Add it to {REGISTER} in the workspace manifest:\n{form}\n(spec §45.3, §45.4)"
            ),
        )];
    };
    required
        .iter()
        .filter(|field| {
            entry
                .get(**field)
                .and_then(toml::Value::as_str)
                .is_none_or(|value| value.trim().is_empty())
        })
        .map(|field| {
            problem(
                "Cargo.toml",
                format!(
                    "justifies the {kind} dependency `{name}` without a `{field}`, so the record \
                     does not say what a later reader needs to know (spec §45.3, §45.4)"
                ),
            )
        })
        .collect()
}

/// Every dependency this workspace declares for itself, with the manifest that declares it.
///
/// A `workspace = true` entry inherits a decision recorded at the root, and a `path` entry is a
/// crate of this repository; neither is a dependency on anything outside it.
fn declared_dependencies(root: &Path) -> Vec<Declared> {
    let mut declared = Vec::new();
    for manifest in manifests(root) {
        let location = relative(root, &manifest);
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        let Ok(parsed) = toml::from_str::<toml::Value>(&text) else {
            continue;
        };
        let mut tables = Vec::new();
        if let Some(workspace) = parsed.get("workspace") {
            tables.push(workspace.get("dependencies"));
        }
        for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
            tables.push(parsed.get(kind));
        }
        for entries in tables.into_iter().flatten() {
            let Some(entries) = entries.as_table() else {
                continue;
            };
            for (name, source) in entries {
                if source.get("workspace").and_then(toml::Value::as_bool) == Some(true)
                    || (source.get("path").is_some() && source.get("git").is_none())
                {
                    continue;
                }
                declared.push(Declared {
                    name: name.clone(),
                    manifest: location.clone(),
                    git: source.get("git").map(|_| source.clone()),
                });
            }
        }
    }
    declared
}

/// Every cargo manifest of this repository, the workspace root first.
fn manifests(root: &Path) -> Vec<PathBuf> {
    let mut files = vec![root.join("Cargo.toml")];
    let mut found = Vec::new();
    collect(root, &is_manifest, &mut found);
    found.sort();
    files.extend(
        found
            .into_iter()
            .filter(|path| path != &root.join("Cargo.toml")),
    );
    files
}

fn is_manifest(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "Cargo.toml")
}

// --- exact tool versions (spec §44.2, §44.3) ----------------------------------------------------

/// Where the version of every tool the release path installs is written down.
const TOOL_REGISTER: &str = "[workspace.metadata.release-tools]";

/// Checks that every tool a release depends on is installed at one exact version (spec §44.2).
///
/// A packaging tool decides what an artifact *is*: `cargo-deb` 3.5 and `cargo-deb` 3.7 lay out a
/// `.deb` differently, so "the same commit" packaged by two runners is two different packages.
/// `taiki-e/install-action` installs the newest release of whatever it is handed, which makes the
/// version an input nobody chose.
///
/// The versions live in the workspace manifest, once:
///
/// ```toml
/// [workspace.metadata.release-tools]
/// cargo-deb = "3.7.0"
/// ```
///
/// and every other mention of a registered tool must be `name@version` with that version — in a
/// workflow's `tool:` list, in a script's install hint, anywhere. The register is also checked
/// backwards: a version nothing installs is a version that has stopped describing the release.
///
/// The Rust toolchain is the same rule in the file that already owns it. `rust-toolchain.toml`
/// must name an exact version rather than a channel, and a workflow that asks
/// `dtolnay/rust-toolchain` for a toolchain must ask for that one (spec §44.3).
#[must_use]
pub fn check_tool_versions(root: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    let register = tool_versions(root);
    let toolchain = pinned_toolchain(root);

    match &toolchain {
        None => problems.push(problem(
            "rust-toolchain.toml",
            "names no toolchain channel, so a release is built by whatever rustup felt like \
             installing (spec §44.3)",
        )),
        Some(channel) if !is_exact_version(channel) => problems.push(problem(
            "rust-toolchain.toml",
            format!(
                "pins the toolchain to `{channel}`, which is a channel and moves every six \
                 weeks. Name the exact version — `channel = \"1.94\"` (spec §44.3)"
            ),
        )),
        Some(_) => {}
    }

    let mut installed: Vec<String> = Vec::new();
    for file in release_path_files(root) {
        let location = relative(root, &file);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let at = format!("{location}:{}", index + 1);
            if is_yaml(&file) {
                problems.extend(unversioned_tools(&at, line, &register, &mut installed));
                if let (Some(asked), Some(pinned)) = (requested_toolchain(line), toolchain.as_ref())
                    && &asked != pinned
                    && !excused_toolchain(&text, line)
                {
                    problems.push(problem(
                        &at,
                        format!(
                            "asks for Rust `{asked}` while `rust-toolchain.toml` pins `{pinned}`. \
                             A release built by a toolchain the repository does not name is not \
                             the release the repository describes (spec §44.3). A verification \
                             job whose tool needs another toolchain — Miri, libFuzzer, the \
                             sanitizers — says which section requires it in a comment on the \
                             line, and its workflow builds no artifact"
                        ),
                    ));
                }
            }
            for (tool, version) in &register {
                // Two spellings, because a tool is installed two ways in a release path: by
                // cargo, as `<tool>@<version>`, and by an installer action, as
                // `<tool>-release: "v<version>"`. §44.2 is about the version being exact, not
                // about who typed it.
                let by_cargo = format!("{tool}@");
                let by_action = format!("{tool}-release:");
                let mentions: Vec<usize> = line
                    .match_indices(by_cargo.as_str())
                    .map(|(at, _)| at + by_cargo.len())
                    .chain(
                        line.match_indices(by_action.as_str())
                            .map(|(at, _)| at + by_action.len()),
                    )
                    .collect();
                for mention in mentions {
                    let found: String = line[mention..]
                        .trim_start_matches([' ', '"', '\'', 'v'])
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.')
                        .collect();
                    installed.push(tool.clone());
                    if &found != version {
                        problems.push(problem(
                            &at,
                            format!(
                                "installs `{tool}` at `{found}` while {TOOL_REGISTER} names \
                                 `{version}`. One of the two is what a release is built with, \
                                 and nothing says which (spec §44.2)"
                            ),
                        ));
                    }
                }
            }
        }
    }

    for (tool, version) in &register {
        if !installed.contains(tool) {
            problems.push(problem(
                "Cargo.toml",
                format!(
                    "pins `{tool}` at `{version}` and nothing in the release path installs it. \
                     A register entry nobody reads has stopped describing the release \
                     (spec §44.2)"
                ),
            ));
        }
    }

    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// The tools and versions of `[workspace.metadata.release-tools]`.
///
/// Public because the release input manifest records the same versions the gate enforces, and
/// two readings of one register are two chances to disagree (ADR-0451).
#[must_use]
pub fn tool_versions(root: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(root.join("Cargo.toml")) else {
        return Vec::new();
    };
    let Ok(manifest) = toml::from_str::<toml::Value>(&text) else {
        return Vec::new();
    };
    manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("metadata"))
        .and_then(|metadata| metadata.get("release-tools"))
        .and_then(toml::Value::as_table)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|(tool, version)| Some((tool.clone(), version.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// The channel `rust-toolchain.toml` pins.
fn pinned_toolchain(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("rust-toolchain.toml")).ok()?;
    let parsed: toml::Value = toml::from_str(&text).ok()?;
    parsed
        .get("toolchain")?
        .get("channel")?
        .as_str()
        .map(str::to_owned)
}

/// Whether a toolchain name is one version rather than a moving channel.
fn is_exact_version(channel: &str) -> bool {
    channel
        .split('.')
        .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// The toolchain a `dtolnay/rust-toolchain` step asks for.
fn requested_toolchain(line: &str) -> Option<String> {
    let value = line.trim_start().strip_prefix("toolchain:")?;
    let value = value.split(" #").next().unwrap_or(value).trim();
    let value = value.trim_matches(|c| c == '"' || c == '\'');
    (!value.is_empty()).then(|| value.to_owned())
}

/// The entries of an install-action `tool:` list that carry no version.
fn unversioned_tools(
    at: &str,
    line: &str,
    register: &[(String, String)],
    installed: &mut Vec<String>,
) -> Vec<Problem> {
    let Some(list) = line.trim_start().strip_prefix("tool:") else {
        return Vec::new();
    };
    list.split(',')
        .map(|entry| entry.split(" #").next().unwrap_or(entry).trim())
        .filter(|entry| !entry.is_empty() && !entry.contains('@'))
        .map(|entry| {
            let known = register.iter().find(|(tool, _)| tool == entry).map_or_else(
                || "and no version is written down for it anywhere".to_owned(),
                |(tool, version)| {
                    installed.push(tool.clone());
                    format!("while {TOOL_REGISTER} names `{version}`")
                },
            );
            problem(
                at,
                format!(
                    "installs `{entry}` at whatever version is newest today {known}. A packaging \
                     or verification tool decides what an artifact is, so it is named exactly — \
                     `{entry}@<version>` (spec §44.2)"
                ),
            )
        })
        .collect()
}

// --- locked release builds (spec §44.3, §44.4) --------------------------------------------------

/// Build commands whose dependency graph must come from the committed lockfile.
const BUILD_COMMANDS: &[&str] = &["cargo build", "cross build"];

/// Checks that every build on the release path resolves nothing (spec §44.3, §44.4).
///
/// `Cargo.lock` is committed, and `--locked` is what makes it authoritative: without the flag
/// cargo repairs a stale lockfile in silence, so the release is built against a graph nobody
/// reviewed and the same commit builds different bytes on two days. Spec §44.4 asks for the
/// opposite — a release build that *fails* when resolution would change.
///
/// Every `cargo build` and `cross build` in the release path carries the flag, and the rule is
/// per line rather than per file on purpose: a fallback that retries the build without the flag
/// (`cargo build --locked || cargo build`) turns the guarantee off exactly when it fires.
///
/// The release path is the workflows, the Dockerfiles and the scripts at the top of `scripts/`.
/// `scripts/demo/` is a developer convenience that builds nothing a user installs.
#[must_use]
pub fn check_locked_builds(root: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    for file in release_path_files(root) {
        let location = relative(root, &file);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                continue;
            }
            let Some(command) = BUILD_COMMANDS
                .iter()
                .find(|command| line.contains(**command))
            else {
                continue;
            };
            if line.contains("--locked") {
                continue;
            }
            problems.push(problem(
                format!("{location}:{}", index + 1),
                format!(
                    "runs `{command}` without `--locked`, so cargo may re-resolve the dependency \
                     graph and build the release against crates the committed `Cargo.lock` never \
                     named. Add `--locked`; a release build must fail rather than repair a stale \
                     lockfile (spec §44.4)"
                ),
            ));
        }
    }
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// Whether a workflow may ask for a toolchain other than the pinned one on this line.
///
/// §44.3 is about the toolchain a *release* is built by, and v0.4.1 asks for three jobs that
/// cannot run on it: §41.2's coverage-guided fuzzing needs libFuzzer's `-Z sanitizer`, and §42.2
/// and §42.3 need Miri and the sanitizers, all of which are nightly-only. A rule that forbade
/// them would forbid the specification.
///
/// The exception is narrow in both directions. The line must say which section requires the
/// toolchain, so the reason is beside the request rather than in somebody's memory; and the
/// workflow must build no artifact, so a job that could produce a release cannot excuse itself
/// however well it explains. `ci.yml` runs `scripts/package.sh`, so nothing in it is excused.
fn excused_toolchain(workflow: &str, line: &str) -> bool {
    let names_a_section = line.contains("spec §") || line.contains("§4");
    let builds_an_artifact = workflow.contains("scripts/package.sh")
        || workflow.contains("scripts/release-check.sh")
        || workflow.contains("softprops/action-gh-release");
    names_a_section && !builds_an_artifact
}

/// The files a release is built by: the workflows, the Dockerfiles, the top-level scripts.
fn release_path_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect(
        &root.join(".github").join("workflows"),
        &is_yaml,
        &mut files,
    );
    collect(&root.join("docker"), &is_dockerfile, &mut files);
    if let Ok(entries) = std::fs::read_dir(root.join("scripts")) {
        files.extend(
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file() && is_shell(path)),
        );
    }
    files.sort();
    files.dedup();
    files
}
