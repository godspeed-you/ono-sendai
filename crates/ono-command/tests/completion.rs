//! Completion is metadata lookup, not search (spec §15.1, §34).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a shared helper in a test binary states its preconditions the same way a #[test] \
              body does (AGENTS.md section 16)"
)]

use ono_command::{Candidate, CandidateKind, CommandRegistry, StageContext};

fn registry() -> &'static CommandRegistry {
    CommandRegistry::embedded().expect("the embedded command contracts must parse")
}

fn complete(line: &str) -> Vec<Candidate> {
    let cursor = line.len();
    ono_command::complete(registry(), &StageContext::from_line(line, cursor), None)
}

fn texts(candidates: &[Candidate]) -> Vec<&str> {
    candidates
        .iter()
        .map(ono_command::Candidate::text)
        .collect()
}

#[test]
fn should_offer_verbs_while_the_head_is_being_typed() {
    let candidates = complete("");
    let names = texts(&candidates);

    assert!(names.contains(&"get"));
    assert!(names.contains(&"watch"));
    assert!(names.contains(&"stop"));
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.kind() == CandidateKind::Verb)
    );
    assert!(
        candidates.iter().any(|candidate| candidate.text() == "get"
            && candidate.doc() == Some("Obtain current objects or state.")),
        "a verb candidate carries the semantics `docs/spec/verbs.yaml` declares"
    );
}

#[test]
fn should_offer_the_targets_of_the_typed_verb() {
    let candidates = complete("get ");
    let names = texts(&candidates);

    assert!(names.contains(&"process"), "spec §15.1's own example");
    assert!(names.contains(&"service"));
    assert!(names.contains(&"file"));
    assert!(names.contains(&"socket"));
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.kind() == CandidateKind::Target)
    );
}

#[test]
fn should_narrow_the_candidates_by_the_typed_prefix() {
    let all = complete("get ").len();
    let narrowed = complete("get pro");

    assert!(narrowed.len() < all, "a prefix must remove candidates");
    assert_eq!(texts(&narrowed), ["process"]);

    let verbs = complete("wat");
    assert_eq!(texts(&verbs), ["watch"]);
}

#[test]
fn should_offer_the_options_the_command_declares() {
    let candidates = complete("get process --");
    let names = texts(&candidates);

    assert!(names.contains(&"--tree"));
    assert!(names.contains(&"--user"));
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.kind() == CandidateKind::Option)
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.text() == "--tree"
                && candidate
                    .doc()
                    .is_some_and(|doc| doc.contains("parent/child"))),
        "an option candidate carries its declared documentation"
    );

    assert_eq!(texts(&complete("get process --tr")), ["--tree"]);
}

#[test]
fn should_offer_the_closed_set_of_values_a_bool_option_accepts() {
    let candidates = complete("get process --tree=");
    let names = texts(&candidates);

    assert_eq!(names, ["--tree=false", "--tree=true"]);
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.kind() == CandidateKind::Value)
    );

    assert_eq!(texts(&complete("get process --tree=t")), ["--tree=true"]);
}

#[test]
fn should_offer_a_declared_default_as_a_value_candidate() {
    let candidates = complete("kill process 4419 --signal=");
    let names = texts(&candidates);

    assert_eq!(
        names,
        ["--signal=SIGKILL"],
        "the only value metadata declares for `--signal` is its default"
    );
}

#[test]
fn should_leave_provider_backed_value_completion_to_the_caller() {
    struct Users;
    impl ono_command::ValueCompleter for Users {
        fn complete(
            &self,
            _command: &ono_command::CommandContract,
            parameter: &ono_command::ParameterSpec,
            prefix: &str,
        ) -> Vec<Candidate> {
            ["root", "daemon", "deploy"]
                .into_iter()
                .filter(|name| name.starts_with(prefix))
                .map(|name| Candidate::value(name).with_doc(parameter.name()))
                .collect()
        }
    }

    let context = StageContext::from_line("get process --user=d", 20);
    let with_hook = ono_command::complete(registry(), &context, Some(&Users));
    assert_eq!(
        texts(&with_hook),
        ["--user=daemon", "--user=deploy"],
        "the hook fills in what only a provider knows"
    );

    let without_hook = ono_command::complete(registry(), &context, None);
    assert!(
        without_hook.is_empty(),
        "without the hook the registry offers nothing it cannot know"
    );
}

#[test]
fn should_offer_nothing_for_a_head_no_command_answers_to() {
    assert!(complete("frobnicate ").is_empty());
}
