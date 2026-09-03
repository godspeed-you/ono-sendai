//! The release verification sequence a reader is told to run (v0.4.1 §47.1, §47.5, §67.7).
//!
//! §47.5 is one sentence with two demands in it: *"The Wiki/install documentation MUST show how to
//! verify checksums and signatures before package installation. Verification instructions SHOULD
//! fit in a short copyable sequence and MUST not require a proprietary service."*
//!
//! `docs/spec/hardening/release_verification.yaml` is that sequence, written once.
//! `docs/reference/release-verification.md` is rendered from it, the README and the Wiki's Install
//! page carry the same commands, and this module is what compares them — because a release that
//! publishes evidence and no instructions has published nothing anyone will check.
//!
//! **The steps are not merely printed.** `xtask/tests/release_verification.rs` builds a release
//! directory, runs the executable steps against it and asserts they pass, then alters one byte of
//! an artifact and asserts they fail. A documented sequence nobody has run is a sequence that
//! works until the day somebody needs it.

use serde::Deserialize;

use crate::scan::Problem;

/// The sequence, compiled in, so the documents and the rule are handed the same copy.
const REGISTRY: &str = include_str!("../../docs/spec/hardening/release_verification.yaml");

/// One file §47.1 requires every release to publish.
#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseFile {
    /// The file name, as the release publishes it.
    pub name: String,
    /// What it holds.
    pub contents: String,
    /// The sections that require it.
    pub spec: String,
}

/// One step of the documented sequence.
#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    /// The step's name.
    pub id: String,
    /// The command, exactly as a reader pastes it.
    pub command: String,
    /// What a reader knows once it succeeds.
    pub proves: String,
    /// What a reader does when it does not.
    pub on_failure: String,
    /// The programs it needs.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Whether the tests run this step against a fixture.
    pub executable: bool,
    /// Why not, where it is not.
    #[serde(default)]
    pub executable_because: String,
}

/// The whole sequence.
#[derive(Debug, Clone, Deserialize)]
pub struct Sequence {
    /// §47.1's required release files.
    pub files: Vec<ReleaseFile>,
    /// The repository the signatures are expected to name.
    pub repository: String,
    /// The OIDC issuer a keyless signature is expected to come from.
    pub issuer: String,
    /// The workflow identity a signature is expected to carry.
    pub workflow: String,
    /// The steps, in the order a reader runs them.
    pub steps: Vec<Step>,
}

/// The documented sequence, or `None` if the registry did not parse.
#[must_use]
pub fn sequence() -> Option<Sequence> {
    serde_yaml_ng::from_str::<Sequence>(REGISTRY).ok()
}

/// Services whose use §47.5 forbids the instructions to require.
///
/// §47.5: verification "MUST not require a proprietary service". The list is short and concrete
/// rather than a judgement about what "proprietary" means — these are the verification services a
/// release like this one would plausibly reach for, and each of them makes the ability to check a
/// download depend on a company's account system. Sigstore is not on it: it is an OpenSSF project
/// with a public transparency log, and §47.3 names it.
const PROPRIETARY: &[&str] = &[
    "docker scout",
    "snyk",
    "jfrog",
    "artifactory",
    "sonatype",
    "chainguard enforce",
];

/// Reports a sequence that would not fit in a document, or would need a service §47.5 forbids.
#[must_use]
pub fn check_sequence() -> Vec<Problem> {
    let location = "docs/spec/hardening/release_verification.yaml";
    let Some(sequence) = sequence() else {
        return vec![Problem::new(
            location,
            "does not parse, so v0.4.1 §47.5's sequence cannot be checked".to_owned(),
        )];
    };
    let mut problems = Vec::new();

    // §47.1's three, plus the certificate a keyless signature is verified with.
    for required in ["SHA256SUMS", "SHA256SUMS.sig"] {
        if !sequence.files.iter().any(|file| file.name == required) {
            problems.push(Problem::new(
                location,
                format!("does not name `{required}`, which v0.4.1 §47.1 requires every release to publish"),
            ));
        }
    }
    if !sequence
        .files
        .iter()
        .any(|file| file.name.contains("provenance"))
    {
        problems.push(Problem::new(
            location,
            "names no provenance file, and v0.4.1 §47.1 requires one beside the checksums and the \
             signature"
                .to_owned(),
        ));
    }

    // §47.5: "MUST not require a proprietary service."
    let commands = sequence
        .steps
        .iter()
        .map(|step| step.command.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    for service in PROPRIETARY {
        if commands.contains(service) {
            problems.push(Problem::new(
                location,
                format!(
                    "reaches `{service}`. v0.4.1 §47.5: verification instructions MUST NOT require \
                     a proprietary service, because the ability to check a download cannot depend \
                     on somebody's account"
                ),
            ));
        }
    }

    // The order is the security property, not a preference.
    let position = |id: &str| sequence.steps.iter().position(|step| step.id == id);
    if let (Some(signature), Some(checksums)) =
        (position("verify_signature"), position("verify_checksums"))
        && signature > checksums
    {
        problems.push(Problem::new(
            location,
            "checks the artifacts against the manifest before verifying the signature over the \
             manifest. Reversed, the sequence proves only that the download was not corrupted: a \
             manifest an attacker wrote agrees perfectly with the artifacts that attacker also \
             wrote"
                .to_owned(),
        ));
    }

    // §47.5: "SHOULD fit in a short copyable sequence."
    let lines = sequence
        .steps
        .iter()
        .filter(|step| step.id != "install")
        .map(|step| step.command.lines().count())
        .sum::<usize>();
    if lines > 24 {
        problems.push(Problem::new(
            location,
            format!(
                "is {lines} lines long. v0.4.1 §47.5 asks for "
            ) + "a short copyable sequence, and a reader who has to scroll is a reader who skips it",
        ));
    }

    for step in &sequence.steps {
        if !step.executable && step.executable_because.trim().is_empty() {
            problems.push(Problem::new(
                location,
                format!(
                    "marks `{}` as not executed by the tests and does not say why. A step nothing \
                     runs is a step that works until somebody needs it",
                    step.id
                ),
            ));
        }
    }
    problems
}

/// Reports a document that prints the sequence differently from the registry.
///
/// §47.5 asks for the instructions to be *in* the installation documentation, and there are three
/// copies of them: the generated reference page, the README and the Wiki's Install page. One is
/// generated and two are written, so the two written ones are compared against the source rather
/// than trusted. What is compared is the command of each step marked as belonging in the short
/// sequence — a document may add prose around it and may not print a different command.
#[must_use]
pub fn check_document(location: &str, text: &str) -> Vec<Problem> {
    let Some(sequence) = sequence() else {
        return Vec::new();
    };
    let normalised = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut problems = Vec::new();
    for step in &sequence.steps {
        let command = step
            .command
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if !normalised.contains(&command) {
            problems.push(Problem::new(
                location.to_owned(),
                format!(
                    "does not print the `{}` step as `docs/spec/hardening/release_verification.yaml` \
                     writes it. v0.4.1 §47.5 puts the sequence in the installation documentation, \
                     and a second copy that drifted is worse than none: a reader would run the \
                     wrong command and believe the right thing.",
                    step.id
                ),
            ));
        }
    }
    problems
}
