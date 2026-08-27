//! How much presentation a destination may receive, and how a theme paints it.
//!
//! Spec §4.6 makes this progressive enhancement, not a mode switch: an operation MUST NOT behave
//! differently *semantically* because a table happens to be interactive. These tests hold the
//! line between "looks better" and "is different".

use ono_render::{Presentation, Style, Theme, Token};

#[test]
fn should_offer_the_richest_presentation_only_to_a_terminal_when_choosing() {
    assert!(Presentation::Terminal.allows_color());
    assert!(Presentation::Terminal.allows_interaction());

    // A pipe gets structure, never cursor control (spec §4.6).
    assert!(!Presentation::Pipe.allows_color());
    assert!(!Presentation::Pipe.allows_interaction());

    assert!(!Presentation::Redirect.allows_color());
    assert!(!Presentation::Redirect.allows_interaction());

    // A script never interacts with a terminal it was not given (spec §17.4).
    assert!(!Presentation::Script.allows_color());
    assert!(!Presentation::Script.allows_interaction());
}

#[test]
fn should_respect_a_users_refusal_of_colour_however_capable_the_terminal_is() {
    // NO_COLOR is a convention users rely on; ignoring it because the terminal is capable is
    // exactly the "it knows better than you" behaviour spec §4.3 argues against.
    let choice = Presentation::choose(true, &[("NO_COLOR", "1")]);
    assert_eq!(choice, Presentation::Plain);
    assert!(!choice.allows_color());
    assert!(!choice.allows_interaction());
}

#[test]
fn should_treat_a_dumb_terminal_as_plain_when_choosing() {
    assert_eq!(
        Presentation::choose(true, &[("TERM", "dumb")]),
        Presentation::Plain
    );
    assert_eq!(
        Presentation::choose(true, &[("TERM", "xterm-256color")]),
        Presentation::Terminal
    );
}

#[test]
fn should_not_offer_a_terminal_presentation_when_output_is_not_a_terminal() {
    assert_eq!(Presentation::choose(false, &[]), Presentation::Pipe);
    assert_eq!(
        Presentation::choose(false, &[("TERM", "xterm")]),
        Presentation::Pipe
    );
}

#[test]
fn should_paint_nothing_at_all_when_the_destination_takes_no_colour() {
    let theme = Theme::default();
    for presentation in [
        Presentation::Pipe,
        Presentation::Redirect,
        Presentation::Script,
        Presentation::Plain,
    ] {
        let painted = theme.paint("nginx", Token::ValueString, presentation);
        assert_eq!(
            painted, "nginx",
            "{presentation:?} must receive the value and nothing else"
        );
    }
}

#[test]
fn should_wrap_a_value_in_its_tokens_style_when_the_destination_is_a_terminal() {
    let theme = Theme::default();
    let painted = theme.paint("nginx", Token::Danger, Presentation::Terminal);
    assert!(painted.contains("nginx"));
    assert!(
        painted.starts_with("\u{1b}["),
        "expected an escape sequence, got {painted:?}"
    );
    assert!(
        painted.ends_with("\u{1b}[0m"),
        "styling must be closed, got {painted:?}"
    );
}

#[test]
fn should_emit_no_escape_sequence_at_all_for_a_token_the_theme_leaves_unstyled() {
    // An escape that paints nothing is still bytes in the output and still noise in a capture.
    let theme = Theme::default();
    assert_eq!(
        theme.paint("nginx", Token::Foreground, Presentation::Terminal),
        "nginx"
    );
}

#[test]
fn should_give_every_semantic_token_a_style_when_a_theme_is_complete() {
    // Spec §44 lists the tokens a theme operates on. A missing one means some part of the shell
    // has no way to say what it means.
    let theme = Theme::default();
    for token in Token::ALL {
        let _: Style = theme.style(*token);
    }
    assert_eq!(Token::ALL.len(), 24, "spec §44 names 24 tokens");
}

#[test]
fn should_name_every_token_as_the_specification_spells_it_when_rendered() {
    let names: Vec<&str> = Token::ALL.iter().map(|token| token.name()).collect();
    for expected in [
        "ui.fg",
        "ui.dim",
        "ui.accent",
        "ui.success",
        "ui.warning",
        "ui.danger",
        "ui.border",
        "ui.selection",
        "ui.prompt.link",
        "ui.prompt.context",
        "ui.prompt.root",
        "ui.table.header",
        "ui.table.key",
        "ui.value.string",
        "ui.value.number",
        "ui.value.unit",
        "ui.value.null",
        "ui.error.code",
        "ui.error.hint",
        "ui.graph.node",
        "ui.graph.edge",
        "ui.graph.edge_inferred",
    ] {
        assert!(names.contains(&expected), "spec §44 names {expected}");
    }
}

#[test]
fn should_resolve_a_token_from_its_name_when_a_theme_file_is_read() {
    for token in Token::ALL {
        assert_eq!(Token::from_name(token.name()), Some(*token));
    }
    assert_eq!(Token::from_name("ui.not.a.token"), None);
}

#[test]
fn should_distinguish_danger_from_success_by_more_than_colour_alone() {
    // Spec §44: "No functionality may depend on color alone."
    let theme = Theme::default();
    assert_ne!(
        theme.style(Token::Danger).marker(),
        theme.style(Token::Success).marker(),
        "a reader who cannot see colour must still be able to tell these apart"
    );
}

#[test]
fn should_carry_no_escape_sequences_from_a_value_into_the_terminal_when_painting() {
    // Spec §49 and docs/ACCEPTANCE.md §4.4: untrusted structured data must not be able to drive
    // the terminal. A process whose name contains an escape sequence is the ordinary case, not
    // an exotic one.
    let theme = Theme::default();
    let hostile = "nginx\u{1b}]0;pwned\u{7}\u{1b}[2J";
    let painted = theme.paint(hostile, Token::Danger, Presentation::Terminal);
    assert!(
        !painted.contains("\u{1b}]"),
        "a window-title sequence survived: {painted:?}"
    );
    assert_eq!(
        painted.matches('\u{1b}').count(),
        2,
        "the only escapes may be the theme's own opening and closing ones: {painted:?}"
    );
    assert!(
        painted.contains("nginx"),
        "the readable part must survive: {painted:?}"
    );
    assert!(
        painted.contains("pwned"),
        "the hostile text is shown inert rather than hidden, so a reader can see it: {painted:?}"
    );
}

#[test]
fn should_sanitise_control_characters_even_when_no_colour_is_applied() {
    let theme = Theme::default();
    let painted = theme.paint("a\u{1b}[31mb", Token::ValueString, Presentation::Pipe);
    assert!(!painted.contains('\u{1b}'), "got {painted:?}");
    assert!(painted.contains('a') && painted.contains('b'));
}
