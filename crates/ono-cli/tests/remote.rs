//! Remote links at the shell boundary (spec §21, §14.4): a link is created, entered, asked, and
//! left — with the answers visibly coming from the other side.
//!
//! The `local` transport spawns this very binary as `ono --agent` over a pipe pair, so the
//! whole path — handshake, negotiation, mounted providers, provenance re-tagging — runs against
//! a real second process without a network.

use ono_testkit::Shell;

fn ono(script: &str) -> ono_testkit::Run {
    Shell::new().args(["-c", script]).run()
}

#[test]
fn should_link_a_host_and_answer_queries_from_the_other_side() {
    let run = ono("link host testbox --transport local; \
         enter link testbox; \
         get process | where pid == 1 | inspect; \
         leave");
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains("testbox"),
        "the records say which host answered (spec §21.2, provenance): {text:?}"
    );
}

#[test]
fn should_list_the_links_the_session_holds() {
    let run = ono("link host testbox --transport local; get link");
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains("testbox") && text.contains("local"),
        "the link table names the host and its transport: {text:?}"
    );
}

#[test]
fn should_leave_the_link_and_answer_locally_again() {
    // Inside the frame the provider set is the remote's; leaving restores the local one. The
    // context stack rules of ADR-0023 apply to links exactly as to objects.
    let run = ono("link host testbox --transport local; \
         enter link testbox; leave; \
         get process | where pid == 1 | inspect");
    run.assert_success();
    let answer = run
        .stdout()
        .lines()
        .skip_while(|line| !line.contains("PROVENANCE"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !answer.contains("testbox"),
        "after leave, the answers are local again: {answer:?}"
    );
}

#[test]
fn should_refuse_to_enter_a_link_that_was_never_made() {
    let run = ono("enter link nowhere");
    assert!(!run.status().is_success());
    assert!(
        run.stderr().contains("Ono-Sendai-E"),
        "a missing link is a structured error: {:?}",
        run.stderr()
    );
}
