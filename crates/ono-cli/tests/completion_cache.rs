//! What a completion leaves in the provider cache (spec §15.1, v0.4.1 §36.2; ADR-0252, ADR-0550).
//!
//! The cache behind `ProviderValues` is one per process and fills from whatever read runs first.
//! This test fills it on purpose, so it has a test binary of its own: beside
//! `completion.rs::should_stop_discovery_at_the_hard_budget_and_answer_what_it_has`, which asserts
//! that a one-nanosecond budget finds the cache cold, a patient read here could warm it first on a
//! slow runner — the CI run for v0.6.1 (34701566431) failed exactly that way.

#![allow(
    clippy::expect_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::{Duration, Instant};

#[test]
fn should_not_answer_the_next_completion_with_what_a_budget_stopped() {
    // §36.2 stops discovery at the hard budget and answers with what the completer had. ADR-0252
    // then keeps what a provider said, so the keystroke after the first is instant — and its
    // doc comment says which answer that is: "it is then in the cache, and the next keystroke has
    // it", meaning the read the budget outlived, never the truncation the budget caused.
    //
    // A read the budget cut short is not what the target holds; it is how far one keystroke got.
    // Remembering it makes a single impatient Tab the shell's answer for the whole freshness
    // window, which is the opposite of what the cache is for.
    let registry = ono_command::CommandRegistry::embedded().expect("the embedded contracts parse");
    let command = registry
        .commands()
        .iter()
        .find(|command| command.id() == "ono.user.get")
        .expect("`get user` is a stable command");
    let parameter = command
        .selectors()
        .first()
        .expect("`get user` takes a selector");

    let impatient = ono_cli::complete::ProviderValues::new(Vec::new())
        .budgeted(Duration::from_nanos(1), Duration::from_nanos(1));
    assert!(
        ono_command::ValueCompleter::complete(&impatient, command, parameter, "").is_empty(),
        "a budget of one nanosecond admits no discovery"
    );
    // Long enough that the read that budget stopped has finished doing whatever it does with what
    // it found. Waiting is what makes this deterministic rather than a race between two threads:
    // whatever the impatient read leaves behind is in place before the patient one starts.
    std::thread::sleep(Duration::from_millis(250));

    let patient = ono_cli::complete::ProviderValues::new(Vec::new());
    let mut offered = ono_command::ValueCompleter::complete(&patient, command, parameter, "");
    let deadline = Instant::now() + ono_testkit::under_load(Duration::from_secs(2));
    while offered.is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        offered = ono_command::ValueCompleter::complete(&patient, command, parameter, "");
    }
    assert!(
        !offered.is_empty(),
        "§15.1, §36.2: a completion whose budget stopped it leaves nothing behind, so the next \
         one still reads this machine's accounts. Got {offered:?}"
    );
}
