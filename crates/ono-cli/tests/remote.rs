//! Remote links at the shell boundary (spec §21, §14.4): a link is created, entered, asked, and
//! left — with the answers visibly coming from the other side.
//!
//! The `local` transport spawns this very binary as `ono --agent` over a pipe pair, so the
//! whole path — handshake, negotiation, mounted providers, provenance re-tagging — runs against
//! a real second process without a network.

use ono_testkit::ono;

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

#[test]
fn should_adapt_on_the_remote_side_inside_a_link_frame() {
    // Spec v0.3 §1.54 (ADAPT-011): inside a link frame the adapter and the executable are the
    // remote's, and the records say so. The local transport's agent is this binary, so the
    // remote has the bundled packs and the same tools.
    let run = ono("link host testbox --transport local; \
         enter link testbox; \
         findmnt | where target == \"/\" | select target filesystem | to json; \
         leave");
    run.assert_success();
    assert!(
        run.stdout().contains("\"target\":\"/\""),
        "typed records came back over the link: {:?}",
        run.stdout()
    );
    let provenance = ono("link host testbox --transport local; \
         enter link testbox; \
         findmnt | where target == \"/\" | inspect | to json; \
         leave");
    provenance.assert_success();
    let text = provenance.stdout();
    assert!(
        text.contains("\"link\":\"testbox\"")
            && text.contains("adapter:org.ono.compat.util-linux.findmnt"),
        "provenance carries the host and the adapter (spec v0.3 §1.54): {text:?}"
    );
}

#[test]
fn should_explain_that_a_remote_host_negotiates_its_own_adapters() {
    let run = ono("link host testbox --transport local; \
         enter link testbox; \
         explain findmnt | where target == \"/\"; \
         leave");
    run.assert_success();
    assert!(
        run.stdout().contains("on testbox")
            && run
                .stdout()
                .contains("adapted by org.ono.compat.util-linux.findmnt"),
        "explain says where the negotiation happened: {:?}",
        run.stdout()
    );
}

#[test]
fn should_degrade_to_the_local_program_when_the_remote_has_no_adapter() {
    // A byte consumer keeps the classic pipeline, locally, as v0.2 did; a structured demand
    // the remote cannot meet is a visible refusal, never a silent local table.
    let bytes = ono(
        "link host testbox --transport local; enter link testbox; grep -c root /etc/passwd; leave",
    );
    bytes.assert_success();
    assert!(
        bytes.stdout().trim().ends_with(char::is_numeric),
        "got {:?}",
        bytes.stdout()
    );
    // The refused pipeline is the last statement, so its status is the run's.
    let refused = ono(
        "link host testbox --transport local; enter link testbox; grep root /etc/passwd | where a == 1",
    );
    assert_ne!(refused.status().code(), 0);
    assert!(
        refused.stderr().contains("testbox") || refused.stderr().contains("Ono-Sendai-E0"),
        "the refusal names the host or the structured error: {:?}",
        refused.stderr()
    );
}

#[test]
fn should_answer_for_this_sessions_links_and_jobs_from_inside_a_link_frame() {
    // §14.4 makes the active link frame decide where *provider calls* run, and a link, a job and
    // a context are not observations of a machine — they are facts about this session. Answering
    // `get link` from the far side made `get link | detach link` unspellable from inside the
    // link it would detach, while `detach link` itself already acted here (ADR-0269).
    let run = ono("link host testbox --transport local; \
         enter link testbox; \
         get link | count | to json; \
         get job | count | to json; \
         get context | count | to json; \
         leave");
    run.assert_success();
    let counts: Vec<&str> = run
        .stdout()
        .lines()
        .filter(|line| line.starts_with('['))
        .collect();
    assert_eq!(
        counts.first().copied(),
        Some("[1]"),
        "§14.4: the session's own link table is what `get link` answers with, got {:?}",
        run.output()
    );
    assert_eq!(
        counts.get(1).copied(),
        Some("[0]"),
        "the session's own job table, empty here, got {:?}",
        run.output()
    );
    assert!(
        counts.get(2).is_some_and(|count| *count != "[0]"),
        "the session's own context stack, which holds the link frame, got {:?}",
        run.output()
    );
}

#[test]
fn should_detach_the_link_it_is_standing_in_when_the_link_table_feeds_the_mutation() {
    // The composition the split above made unspellable: §14.5 asks that every operation remain
    // expressible, and `get link | detach link` from inside the frame is the one a user reaches
    // for when a link stops answering.
    let run = ono("link host testbox --transport local; \
         enter link testbox; \
         get link | detach link | select status | to json");
    run.assert_success();
    assert!(
        run.stdout().contains("\"status\":\"success\""),
        "the link the session stands in is the one detached, got {:?}",
        run.output()
    );
}
