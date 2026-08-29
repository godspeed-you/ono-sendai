//! A theme a user can choose, and a theme a user can write.
//!
//! Spec §44 asks for a theme system that operates on semantic tokens and forbids output whose
//! meaning depends on colour alone; spec §30 names the configuration domain (`theme`) and the
//! place a theme file lives (`~/.config/ono/themes/*.toml`). The tokens have existed since phase
//! B; what is asserted here is that a theme can be *selected* and *loaded*, that a broken one
//! never costs a user their shell, and that no theme can change what the shell prints where
//! there is no colour.
//!
//! Every test asserts what a user sees (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md §16)"
)]

use ono_testkit::{Scratch, Shell, scratch};

/// A shell whose whole configuration tree lives in `dir`, painting on a terminal.
fn isolated(dir: &Scratch) -> Shell {
    Shell::new()
        .env("HOME", dir.path().display().to_string())
        .env(
            "XDG_CONFIG_HOME",
            dir.path().join("xdg").display().to_string(),
        )
        .env(
            "ONO_CONFIG_DIR",
            dir.path().join("ono").display().to_string(),
        )
        .env_remove("ONO_CONFIG")
        .env_remove("ONO_THEME_NAME")
        .env_remove("NO_COLOR")
}

#[test]
fn should_list_the_theme_domain_among_the_configuration_settings() {
    let dir = scratch();
    let run = isolated(&dir)
        .args(["-c", "get config theme. | to json"])
        .run();
    run.assert_success();
    assert!(
        run.stdout().contains("theme.name"),
        "spec §30 lists `theme` among the configuration domains, and `get config` must show it \
         with its provenance: {}",
        run.stdout()
    );
    assert!(
        run.stdout().contains("\"ono\""),
        "the default is the restrained `ono` theme of spec §44: {}",
        run.stdout()
    );
}

#[test]
fn should_report_the_theme_it_is_painting_with_when_a_second_one_is_chosen() {
    let dir = scratch();
    let run = isolated(&dir)
        .args([
            "-c",
            "set config theme.name = neon; get config theme.name | to json",
        ])
        .run();
    run.assert_success();
    assert!(
        run.stdout().contains("neon"),
        "a theme the shell ships must be selectable by name: {}",
        run.stdout()
    );
}

#[test]
fn should_read_the_theme_file_from_the_users_themes_directory() {
    // Nothing is painted into a pipe, so what proves the file was *found* is that its contents
    // are held to the format, with the path in the refusal. Whether the colour reaches a terminal
    // is acceptance case `150`, which has one.
    let dir = scratch();
    dir.write(
        "ono/themes/lab.toml",
        "extends = \"ono\"\n[tokens]\n\"ui.table.header\" = { blink = true }\n",
    );
    dir.write("ono/config.ono", "set config theme.name = \"lab\"\n");

    let run = isolated(&dir).args(["-c", "get config theme.name"]).run();
    run.assert_success();
    assert!(
        run.stderr().contains("themes/lab.toml"),
        "a theme is looked for in `<config dir>/themes/<name>.toml` (spec §30): {}",
        run.stderr()
    );
}

#[test]
fn should_load_a_theme_file_that_overrides_only_some_tokens_without_complaint() {
    let dir = scratch();
    dir.write(
        "ono/themes/lab.toml",
        "extends = \"neon\"\n[tokens]\n\"ui.table.header\" = { color = 201, bold = true }\n",
    );
    dir.write("ono/config.ono", "set config theme.name = \"lab\"\n");

    let run = isolated(&dir)
        .args(["-c", "get config --problems | count | to json"])
        .run();
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[0]",
        "a theme file that names real tokens is not a problem; stderr: {}",
        run.stderr()
    );
}

#[test]
fn should_report_a_theme_that_does_not_exist_and_keep_the_shell_working() {
    let dir = scratch();
    dir.write("ono/config.ono", "set config theme.name = \"nosuch\"\n");
    let run = isolated(&dir).args(["-c", "get config theme.name"]).run();

    run.assert_success();
    assert!(
        run.stderr().contains("nosuch"),
        "a theme nobody can find is said out loud, not silently ignored: {}",
        run.stderr()
    );
    assert!(
        run.stdout().contains("nosuch"),
        "the setting still reports what the user asked for: {}",
        run.stdout()
    );
}

#[test]
fn should_report_a_broken_theme_file_and_keep_the_shell_working() {
    let dir = scratch();
    dir.write(
        "ono/themes/broken.toml",
        "[tokens]\n\"ui.dangerous\" = { color = 9 }\n",
    );
    dir.write("ono/config.ono", "set config theme.name = \"broken\"\n");
    let run = isolated(&dir)
        .args(["-c", "get process | take 1 | count"])
        .run();

    run.assert_success();
    assert!(
        run.stderr().contains("ui.dangerous"),
        "a theme file that names a token nobody defines says which one: {}",
        run.stderr()
    );
}

#[test]
fn should_print_the_same_bytes_under_every_theme_when_colour_is_refused() {
    // Spec §44's closing rule, at the level a user meets it: whatever theme is configured, a
    // reader who has turned colour off, or whose output is a pipe, sees exactly the same text.
    let dir = scratch();
    dir.write(
        "ono/themes/loud.toml",
        "[tokens]\n\"ui.fg\" = { color = 16 }\n\"ui.value.string\" = { color = 16 }\n\
         \"ui.table.header\" = { color = 16 }\n",
    );

    let mut outputs = Vec::new();
    for theme in ["ono", "neon", "loud"] {
        let run = isolated(&dir)
            .env("ONO_THEME_NAME", theme)
            .env("NO_COLOR", "1")
            .env("TERM", "xterm-256color")
            .args(["-c", "get config render.table.max_rows"])
            .run();
        run.assert_success();
        assert!(
            !run.stdout().contains('\u{1b}'),
            "`{theme}` emitted an escape sequence with colour refused: {:?}",
            run.stdout()
        );
        outputs.push(run.stdout().to_owned());
    }
    assert!(
        outputs.windows(2).all(|pair| pair[0] == pair[1]),
        "with colour refused every theme must print the same bytes (spec §44): {outputs:?}"
    );
}
