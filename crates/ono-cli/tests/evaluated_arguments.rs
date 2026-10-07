//! An evaluated selector or option satisfies its declared type, or the command is refused by
//! name — never run as though the argument had not been written (issue #168).
//!
//! A words-mode argument is reinterpreted against the type the command declares (ADR-0009). An
//! expression — `--verb ("get")`, `get user $name`, `--verb ["get"]` — has no value until it is
//! evaluated, and the value it then has is held to the same declaration: coerced the way a word
//! would be where it fits, bound to the selector whose type it fits, and refused with
//! `type.mismatch` naming the parameter, its declared type and the type it received where it fits
//! none (spec §2.6: a filter that silently did not apply is worse than a refusal; spec §43).
//! Contracts: `docs/contracts/commands/meta.yaml` (`get command --verb: string`), `identity.yaml`
//! (`get user` selectors `uid: int`, `name: string`), `file.yaml` (`get file` path, repeatable).

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_testkit::{Shell, ono, scratch};

#[test]
fn should_refuse_the_option_by_name_when_a_string_option_evaluates_to_a_list() {
    let run = ono("get command --verb [\"get\"] | count | to json");
    assert!(
        !run.status().is_success(),
        "meta.yaml declares `--verb` a `string`; a list is not one, and the command must not run \
         as if the filter had not been written; got {:?}",
        run.output()
    );
    let stderr = run.stderr();
    assert!(
        stderr.contains("Ono-Sendai-E0201"),
        "errors.yaml E0201 type.mismatch, got {stderr:?}"
    );
    for named in ["--verb", "string", "list"] {
        assert!(
            stderr.contains(named),
            "the refusal names the option, its declared type and the type it received \
             (`{named}` missing), got {stderr:?}"
        );
    }
    assert!(
        run.stdout().trim().is_empty(),
        "nothing is counted when the filter could not apply, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_refuse_a_record_literal_where_a_string_option_is_declared() {
    let run = ono("get command --verb ({verb: \"get\"}) | count | to json");
    assert!(
        !run.status().is_success() && run.stderr().contains("Ono-Sendai-E0201"),
        "a record literal — a `map` in the value model — is not a `string` either, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("--verb") && run.stderr().contains("map"),
        "named by option and received type, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_apply_an_evaluated_option_exactly_as_the_word_when_its_value_fits() {
    let all = ono("get command | count | to json");
    let worded = ono("get command --verb get | count | to json");
    let evaluated = ono("get command --verb (\"get\") | count | to json");
    let bound = ono("let verb = \"get\"; get command --verb $verb | count | to json");
    for run in [&all, &worded, &evaluated, &bound] {
        run.assert_success();
    }
    assert_eq!(
        evaluated.stdout(),
        worded.stdout(),
        "`--verb (\"get\")` is `--verb get`"
    );
    assert_eq!(
        bound.stdout(),
        worded.stdout(),
        "`--verb $verb` is `--verb get`"
    );
    assert_ne!(
        worded.stdout(),
        all.stdout(),
        "`--verb get` narrows the registry, so the comparison above means something"
    );
}

#[test]
fn should_bind_an_evaluated_selector_to_the_selector_its_value_fits() {
    // `get user` declares `uid: int` before `name: string` (ADR-0095). A word binds to the first
    // selector whose type it fits; a variable's value is held to the same rule once evaluated,
    // rather than being compared with every uid and matching none — or, before #168, being
    // dropped and answering every account.
    let named = ono("let who = \"root\"; get user $who | select name uid | to json");
    named.assert_success();
    assert_eq!(
        named.stdout().trim(),
        r#"[{"name":"root","uid":0}]"#,
        "`get user $who` with `$who = \"root\"` is `get user root`"
    );
    let numbered = ono("let who = 0; get user $who | select name | to json");
    numbered.assert_success();
    assert_eq!(
        numbered.stdout().trim(),
        r#"[{"name":"root"}]"#,
        "`get user $who` with `$who = 0` is `get user 0`"
    );
}

#[test]
fn should_accept_a_list_when_the_selector_is_repeatable() {
    let dir = scratch();
    for name in ["a", "b", "c"] {
        std::fs::write(dir.path().join(name), name).expect("the fixture file is written");
    }
    let run = Shell::new()
        .args(["-c", "get file ([\"a\", \"b\"]) | select name | to json"])
        .cwd(dir.path())
        .run();
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"name":"a"},{"name":"b"}]"#,
        "file.yaml declares `get file`'s path repeatable, so a list names several paths"
    );
}
