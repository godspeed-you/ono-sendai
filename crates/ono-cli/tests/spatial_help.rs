//! `help spatial` — spec v0.4 §38.1, and the `docs/ACCEPTANCE.md` §4.7.1 box that requires the
//! spatial commands to teach themselves.
//!
//! §38 opens with "Spatial commands MUST teach themselves" and §38.1 makes the overview a MUST,
//! naming the eleven verbs it explains and one line for each. A dogfooding session found the
//! topic missing: `help look` was complete, and `help spatial` — the page a user reaches for
//! first, before they know any verb to ask about — answered `resolve.command_not_found`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_testkit::scratch;

mod support;
use support::isolated;

/// The verbs §38.1 lists in its overview, each with what it is for.
const OVERVIEW: [(&str, &str); 11] = [
    ("look", "where you are"),
    ("map", "topology"),
    ("enter", "move into"),
    ("follow", "relationship"),
    ("jump", "another known place"),
    ("back", "trail"),
    ("up", "canonical parent"),
    ("home", "system root"),
    ("near", "neighbo"),
    ("find", "search"),
    ("trail", "where you moved"),
];

fn ono(script: &str) -> ono_testkit::Run {
    let dir = scratch();
    isolated(&dir).args(["-c", script]).run()
}

#[test]
fn should_explain_every_spatial_verb_of_the_overview_when_help_spatial_runs() {
    // §38.1: "Ono MUST provide a concise overview explaining:" — the eleven lines that follow.
    let run = ono("help spatial");
    run.assert_success();
    let page = run.stdout().to_lowercase();
    for (verb, meaning) in OVERVIEW {
        assert!(
            page.contains(verb),
            "§38.1: `help spatial` explains `{verb}`; got:\n{}",
            run.stdout()
        );
        assert!(
            page.contains(meaning),
            "§38.1: `help spatial`'s line for `{verb}` says what it is for ({meaning:?}); got:\n{}",
            run.stdout()
        );
    }
}

#[test]
fn should_send_the_reader_from_the_overview_to_one_verbs_own_page() {
    // §38's "spatial commands MUST teach themselves": the overview is a way in, so it names the
    // page that holds the detail, and that page exists.
    let overview = ono("help spatial");
    overview.assert_success();
    assert!(
        overview.stdout().contains("help look"),
        "§38.1: the overview points at the full page of a verb; got:\n{}",
        overview.stdout()
    );
    let page = ono("help look");
    page.assert_success();
    assert!(
        page.stdout().contains("--json"),
        "`help look` is the full page, not a second overview; got:\n{}",
        page.stdout()
    );
}

#[test]
fn should_offer_the_overview_among_the_topics_help_lists() {
    // A topic nothing points at is a topic nobody finds (§38).
    let run = ono("help");
    run.assert_success();
    assert!(
        run.stdout().contains("help spatial"),
        "`help` names the spatial overview among its topics; got:\n{}",
        run.stdout()
    );
}

#[test]
fn should_name_the_relations_of_the_current_place_when_help_here_runs() {
    // §38.2: "At any place: `help here` … SHOULD show spatial operations supported by that
    // place." The other half of §38.1's overview: what applies *here*, which the overview cannot
    // know. A process place offers `children`, `cgroup` and `user`; the SYSTEM root offers none
    // of them, and a help page that said the same in both places would be the overview again.
    let run = ono("enter process 1; help here");
    run.assert_success();
    let text = run.stdout();
    for exit in ["children", "cgroup", "user"] {
        assert!(
            text.contains(exit),
            "§38.2: `help here` names the relations this place offers — `{exit}` is missing \
             from {text:?}"
        );
    }
    assert!(
        text.contains("near") && text.contains("follow"),
        "§38.2: and what to do with them, got {text:?}"
    );
}

#[test]
fn should_say_what_the_root_place_offers_when_help_here_runs_there() {
    let run = ono("help here");
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains("SYSTEM") || text.contains("local"),
        "§38.2: the page says which place it is about, got {text:?}"
    );
    assert!(
        !text.contains("cgroup"),
        "§38.2: it is about *this* place, not about every place, got {text:?}"
    );
}

#[test]
fn should_offer_here_among_the_topics_help_lists() {
    let run = ono("help");
    run.assert_success();
    assert!(
        run.stdout().contains("help here"),
        "§38.2: the landing page names the context-sensitive page, got {:?}",
        run.output()
    );
}
