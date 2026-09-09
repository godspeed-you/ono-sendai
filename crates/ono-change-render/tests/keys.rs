//! Appendix E.3's reference inspector bindings.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_render::{BINDINGS, InspectorAction, binding_for, key_help};

#[test]
fn should_carry_every_binding_appendix_e_three_lists() {
    let keys: Vec<&str> = BINDINGS.iter().map(|binding| binding.keys()).collect();
    assert_eq!(
        keys,
        vec![
            "j/k or arrows",
            "Enter",
            "Space",
            "I",
            "P",
            "R",
            "V",
            "M",
            "T",
            "A",
            "Esc"
        ],
        "Appendix E.3's reference bindings, in the order the appendix teaches them"
    );
}

#[test]
fn should_carry_every_meaning_appendix_e_three_fixes() {
    let meanings: Vec<&str> = BINDINGS
        .iter()
        .map(|binding| binding.action().meaning())
        .collect();
    assert_eq!(
        meanings,
        vec![
            "move",
            "inspect selected item",
            "expand/collapse",
            "impact",
            "protection",
            "recovery preview",
            "verification contracts",
            "map --plan",
            "timeline context",
            "apply (opens gate if required)",
            "back"
        ],
        "Appendix E.3: bindings MAY be configurable, and these meanings guide discoverability"
    );
}

#[test]
fn should_bind_each_key_to_exactly_one_action() {
    let mut keys: Vec<&str> = BINDINGS.iter().map(|binding| binding.keys()).collect();
    keys.sort_unstable();
    let before = keys.len();
    keys.dedup();
    assert_eq!(
        before,
        keys.len(),
        "a key with two meanings is a key a user cannot learn"
    );
}

#[test]
fn should_bind_each_action_to_exactly_one_key() {
    let mut actions: Vec<InspectorAction> =
        BINDINGS.iter().map(|binding| binding.action()).collect();
    actions.sort_unstable();
    let before = actions.len();
    actions.dedup();
    assert_eq!(
        before,
        actions.len(),
        "Appendix E.3's table is a mapping, and two keys for one meaning teaches neither"
    );
}

#[test]
fn should_find_the_key_bound_to_an_action() {
    assert_eq!(
        binding_for(InspectorAction::Protection).map(|binding| binding.keys()),
        Some("P"),
        "§20.4: protection has to be reachable, and `help` finds it by meaning"
    );
}

#[test]
fn should_bind_a_key_to_the_protection_pane_and_to_the_recovery_preview_separately() {
    assert_ne!(
        binding_for(InspectorAction::Protection).map(|binding| binding.keys()),
        binding_for(InspectorAction::RecoveryPreview).map(|binding| binding.keys()),
        "§10.4: protection and recovery are different questions, and they open different panes"
    );
}

#[test]
fn should_print_one_help_line_per_binding() {
    let lines = key_help(80);
    assert_eq!(
        lines.len(),
        BINDINGS.len(),
        "Appendix E.3's meanings guide discoverability, which means all of them are listed"
    );
    for binding in BINDINGS {
        assert!(
            lines
                .iter()
                .any(|line| line.contains(binding.keys())
                    && line.contains(binding.action().meaning())),
            "the key and its meaning appear on the same line, or neither is discoverable"
        );
    }
}

#[test]
fn should_lay_the_key_help_out_at_the_width_it_was_given() {
    for width in [40usize, 80] {
        for line in key_help(width) {
            assert!(
                line.chars().count() <= width,
                "v0.4 §39.3: help fits the terminal it is asked for"
            );
        }
    }
}

#[test]
fn should_open_the_gate_rather_than_apply_from_the_binding_itself() {
    assert_eq!(
        InspectorAction::Apply.meaning(),
        "apply (opens gate if required)",
        "§19.4 and §40.1: a key press is not an acknowledgement, and the meaning says so"
    );
}
