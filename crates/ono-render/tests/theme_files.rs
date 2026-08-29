//! Themes that can be loaded, and the one guarantee a loaded theme may not break.
//!
//! Spec §44 asks for themes that operate on semantic tokens, names a default "Ono" theme and a
//! more aggressive cyberpunk one, and closes with the rule that outranks every palette: "No
//! functionality may depend on color alone." Spec §30 says where a theme file lives —
//! `~/.config/ono/themes/*.toml`. This suite is about the format and the rule; where the files
//! are found is `crates/ono-cli/tests/theme.rs`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md §16)"
)]

use ono_render::{Color, Presentation, Theme, Token};

#[test]
fn should_ship_a_second_theme_that_uses_accent_colour_more_aggressively() {
    let names = Theme::builtin_names();
    assert!(
        names.contains(&"ono"),
        "the default theme spec §44 describes is called `ono`: {names:?}"
    );
    assert!(
        names.len() >= 2,
        "spec §44 contrasts the restrained default with a cyberpunk theme; a theme setting with \
         one theme to choose from is not a theme system: {names:?}"
    );

    let restrained = Theme::named("ono").expect("the default theme");
    let bold = Theme::builtin_names()
        .iter()
        .filter(|name| **name != "ono")
        .map(|name| Theme::named(name).expect("a built-in theme"))
        .next()
        .expect("a second built-in theme");
    assert_ne!(
        restrained.style(Token::Accent).colour(),
        bold.style(Token::Accent).colour(),
        "two themes that paint the accent identically are one theme with two names"
    );
}

#[test]
fn should_give_every_token_a_style_in_every_built_in_theme() {
    for name in Theme::builtin_names() {
        let theme = Theme::named(name).expect("a built-in theme");
        for token in Token::ALL {
            let _ = theme.style(*token);
        }
        assert_eq!(theme.name(), *name, "a theme knows its own name");
    }
}

#[test]
fn should_read_a_theme_file_into_the_tokens_it_names() {
    let theme = Theme::parse(
        "neon-lab",
        r#"
        extends = "ono"

        [tokens]
        "ui.danger" = { color = 197, bold = true, marker = "!!!" }
        "ui.value.null" = { color = "default", dim = true, marker = "-" }
        "#,
    )
    .expect("a valid theme file");

    assert_eq!(theme.name(), "neon-lab");
    assert_eq!(theme.style(Token::Danger).colour(), Color::Indexed(197));
    assert_eq!(theme.style(Token::Danger).marker(), Some("!!!"));
    assert_eq!(theme.style(Token::ValueNull).colour(), Color::Default);
    assert_eq!(theme.style(Token::ValueNull).marker(), Some("-"));
}

#[test]
fn should_keep_the_extended_themes_styles_for_the_tokens_a_file_does_not_name() {
    let base = Theme::named("ono").expect("the default theme");
    let theme = Theme::parse(
        "partial",
        "extends = \"ono\"\n[tokens]\n\"ui.danger\" = { color = 9 }\n",
    )
    .expect("a valid theme file");

    assert_eq!(
        theme.style(Token::Success).marker(),
        base.style(Token::Success).marker(),
        "a theme that overrides one token must not silently unstyle the other twenty-three"
    );
    assert_eq!(theme.style(Token::Danger).colour(), Color::Indexed(9));
}

#[test]
fn should_refuse_a_token_name_the_specification_does_not_define() {
    let error = Theme::parse("typo", "[tokens]\n\"ui.dangerous\" = { color = 9 }\n")
        .expect_err("an unknown token must be refused, not ignored");
    assert!(
        error.message().contains("ui.dangerous"),
        "the refusal names the token nobody defines: {}",
        error.message()
    );
}

#[test]
fn should_refuse_a_style_key_it_does_not_implement() {
    let error = Theme::parse("wishful", "[tokens]\n\"ui.danger\" = { blink = true }\n")
        .expect_err("a key nothing implements must be refused, not ignored");
    assert!(
        error.message().contains("blink"),
        "the refusal names the key nothing implements: {}",
        error.message()
    );
}

#[test]
fn should_refuse_a_theme_that_extends_one_nobody_ships() {
    let error = Theme::parse("orphan", "extends = \"nosuch\"\n[tokens]\n")
        .expect_err("a base that does not exist must be refused");
    assert!(
        error.message().contains("nosuch"),
        "the refusal names the base it cannot find: {}",
        error.message()
    );
}

#[test]
fn should_refuse_a_marker_that_could_drive_the_terminal() {
    for hostile in ["\u{1b}[2J", "ok\nroot", "\u{7}"] {
        let file = format!("[tokens]\n\"ui.success\" = {{ marker = {hostile:?} }}\n");
        let error = Theme::parse("hostile", &file)
            .expect_err("a marker is printed verbatim, so it may not carry control characters");
        assert!(
            error.message().contains("ui.success"),
            "the refusal names the token whose marker was rejected: {}",
            error.message()
        );
    }
}

#[test]
fn should_refuse_a_marker_long_enough_to_break_a_layout() {
    let error = Theme::parse(
        "shouty",
        "[tokens]\n\"ui.success\" = { marker = \"absolutely fine\" }\n",
    )
    .expect_err("a marker sits inside a table cell; it is a mark, not a sentence");
    assert!(
        error.message().contains("ui.success"),
        "the refusal names the token: {}",
        error.message()
    );
}

#[test]
fn should_paint_nothing_at_all_whatever_the_theme_when_the_destination_takes_no_colour() {
    // Spec §44's closing rule, held against every theme that can exist: a theme decides colour on
    // a colour-capable terminal and decides nothing anywhere else. A theme file therefore cannot
    // make output unreadable in a pipe, on a dumb terminal or under NO_COLOR, because in those
    // destinations no theme is consulted at all.
    let loud = Theme::parse(
        "loud",
        "[tokens]\n\"ui.fg\" = { color = 16 }\n\"ui.value.string\" = { color = 16 }\n",
    )
    .expect("a valid theme file");

    for theme in [Theme::default(), loud] {
        for presentation in [
            Presentation::Pipe,
            Presentation::Redirect,
            Presentation::Script,
            Presentation::Plain,
        ] {
            for token in Token::ALL {
                assert_eq!(
                    theme.paint("nginx", *token, presentation),
                    "nginx",
                    "`{}` painted {} at {presentation:?}; with colour disabled every theme must \
                     paint the same bytes (spec §44)",
                    theme.name(),
                    token.name()
                );
            }
        }
    }
}

#[test]
fn should_keep_danger_distinguishable_from_success_without_colour_in_every_built_in_theme() {
    for name in Theme::builtin_names() {
        let theme = Theme::named(name).expect("a built-in theme");
        assert_ne!(
            theme.style(Token::Danger).marker(),
            theme.style(Token::Success).marker(),
            "`{name}` distinguishes danger from success by colour alone, which spec §44 forbids"
        );
        assert!(
            theme.style(Token::Danger).marker().is_some(),
            "`{name}` leaves danger with nothing a monochrome reader can see"
        );
    }
}

#[test]
fn should_refuse_a_theme_that_leaves_danger_indistinguishable_from_success() {
    let error = Theme::parse(
        "colour-only",
        "[tokens]\n\"ui.danger\" = { color = 9, marker = \"ok\" }\n",
    )
    .expect_err("a theme may not make two opposite meanings look the same without colour");
    assert!(
        error.message().contains("ui.danger"),
        "the refusal names the token that lost its distinction: {}",
        error.message()
    );
}
