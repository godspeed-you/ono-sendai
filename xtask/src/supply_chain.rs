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
    let mut problems = Vec::new();
    for file in action_definitions(root) {
        let location = relative(root, &file);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let Some(reference) = uses_reference(line) else {
                continue;
            };
            if is_repository_local_action(&reference) {
                continue;
            }
            if let Some((_, git_ref)) = reference.rsplit_once('@')
                && is_commit_sha(git_ref)
            {
                continue;
            }
            problems.push(problem(
                format!("{location}:{}", index + 1),
                format!(
                    "uses `{reference}`, which is a name its owner can repoint at any commit. \
                     Pin the full commit SHA and keep the version in a trailing comment — \
                     `uses: owner/action@<40-hex> # v1.2.3` — resolved with \
                     `gh api repos/owner/action/commits/<tag> --jq .sha` (spec §43.1, §62.1)"
                ),
            ));
        }
    }
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// The `uses:` value of one line, with any trailing comment and quoting removed.
///
/// Comments are skipped rather than parsed: a line that only *mentions* a reference — a note
/// about the version that was replaced — is prose, and reporting it would teach people to stop
/// writing the note.
fn uses_reference(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    let after_dash = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim_start();
    let value = after_dash.strip_prefix("uses:")?;
    let value = value.split(" #").next().unwrap_or(value).trim();
    let value = value.trim_matches(|c| c == '"' || c == '\'');
    (!value.is_empty()).then(|| value.to_owned())
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
    let mut problems = Vec::new();
    for file in image_sources(root) {
        let location = relative(root, &file);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let references = if is_dockerfile(&file) {
            dockerfile_images(&text)
        } else if is_yaml(&file) {
            workflow_images(&text)
        } else {
            shell_images(&text)
        };
        for (line, reference) in references {
            if reference.contains('$')
                || reference.starts_with(LOCAL_IMAGE_PREFIX)
                || has_digest(&reference)
            {
                continue;
            }
            problems.push(problem(
                format!("{location}:{line}"),
                format!(
                    "pulls `{reference}`, a tag its publisher can repoint, so the same commit \
                     builds a different artifact tomorrow. Append the digest and keep the tag \
                     readable — `{reference}@sha256:<64-hex>` — resolved with \
                     `docker manifest inspect {reference}` (spec §44.1, §62.2)"
                ),
            ));
        }
    }
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
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
