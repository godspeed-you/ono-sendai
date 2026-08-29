//! The context stack of spec §14: `enter` pushes, `leave` pops, and nothing about it is magic —
//! every effect is visible and every narrowed query is expressible without the context.

use ono_testkit::Shell;

fn ono(script: &str) -> ono_testkit::Run {
    Shell::new().args(["-c", script]).run()
}

#[test]
fn should_enter_a_directory_and_leave_back_out_of_it() {
    // Spec §14.2: `enter dir /etc` is `cd` in effect, with the stack's mental model — so `leave`
    // undoes it, which plain `cd` never promised.
    let run = ono("enter dir /etc; pwd; leave; pwd");
    run.assert_success();

    let lines: Vec<&str> = run.stdout().lines().collect();
    assert!(
        lines.contains(&"/etc"),
        "entering the directory moved the session there, got {lines:?}"
    );
    assert_ne!(
        lines.last().copied(),
        Some("/etc"),
        "leaving restored where the session stood before, got {lines:?}"
    );
}

#[test]
fn should_stay_on_the_ground_when_there_is_nothing_to_leave() {
    // ADR-0023: `leave` at the bottom of the stack is a no-op with a diagnostic — not an error,
    // and never a fall-through to something else.
    let run = ono("leave");
    run.assert_success();
    assert!(
        !run.stderr().is_empty(),
        "the no-op says so rather than pretending something was popped"
    );
}

#[test]
fn should_show_the_stack_when_asked_for_the_context() {
    let run = ono("enter dir /etc; get context");
    run.assert_success();

    let text = run.stdout();
    assert!(
        text.contains("local"),
        "the ground frame is part of the stack (spec §14.1), got {text:?}"
    );
    assert!(
        text.contains("/etc"),
        "the entered directory appears with its identity, got {text:?}"
    );
}

#[test]
fn should_refuse_to_enter_an_object_that_does_not_exist() {
    // Entering is a statement about a real object; a name nothing answers to is refused with a
    // structured error rather than pushing a frame that would narrow every later query to
    // nothing.
    let run = ono("enter service ono-definitely-not-a-unit");
    assert!(
        !run.status().is_success(),
        "entering nothing must fail, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E"),
        "the refusal is structured (spec §43), got {:?}",
        run.stderr()
    );
}

#[test]
fn should_leave_every_frame_at_once_when_asked() {
    let run = ono("enter dir /etc; enter dir /usr; leave --all; pwd");
    run.assert_success();
    let last = run.stdout().lines().last().unwrap_or_default().to_owned();
    assert!(
        last != "/usr" && last != "/etc",
        "leaving all frames restores the ground, got {last:?}"
    );
}

// --- `explain` prints the spelling a frame narrows to (ADR-0023, ADR-0225) -------------------

#[test]
fn should_print_the_narrowed_spelling_when_explaining_inside_an_object_frame() {
    // ADR-0023: "`enter service nginx` then `get process` is exactly `get process --service
    // nginx`, and `explain` prints the second form when asked about the first."
    let run = ono("enter process 1; explain get process");
    run.assert_success();
    assert!(
        run.stdout().contains("get process 1"),
        "spec §14.5: the frame's contribution has a spelling, and the plan shows it: {}",
        run.stdout()
    );
}

#[test]
fn should_print_the_option_a_frame_of_another_target_fills_in() {
    let run = ono("enter user root; explain get process");
    run.assert_success();
    assert!(
        run.stdout().contains("get process --user root"),
        "a frame of another target contributes the option named after it: {}",
        run.stdout()
    );
}

#[test]
fn should_say_nothing_about_narrowing_when_no_frame_is_in_force() {
    let run = ono("explain get process");
    run.assert_success();
    assert!(
        !run.stdout().contains("narrowed"),
        "a plan outside a frame has no narrowing to report: {}",
        run.stdout()
    );
}

#[test]
fn should_keep_what_was_typed_when_a_frame_would_have_filled_it_in() {
    // Spec §14.5 / ADR-0076 §2: what was typed wins, so there is nothing to narrow.
    let run = ono("enter process 1; explain get process 5");
    run.assert_success();
    assert!(
        !run.stdout().contains("narrowed"),
        "`get process 5` inside `enter process 1` is `get process 5`: {}",
        run.stdout()
    );
}
