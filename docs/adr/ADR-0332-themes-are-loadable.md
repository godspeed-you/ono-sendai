# ADR-0332: A theme is chosen by a setting and written as a file

- Status: accepted
- Date: 2026-08-29
- Spec refs: §44 (semantic tokens, the default and cyberpunk themes, "no functionality may depend
  on color alone"), §30 (configuration domains and `~/.config/ono/themes/*.toml`), §4.3 (colour),
  §49 (a value may never drive the terminal); ADR-0010, ADR-0094
- Decided by: agent (autonomous)

## Context

Spec §44's twenty-four semantic tokens existed and were tested, and nothing could use them: every
call site in the shell constructed `Theme::default()`, there was no `theme` configuration domain,
no reader for the `~/.config/ono/themes/*.toml` that §30 names, and no second theme. `Token::from_name`
documented "as a theme file names it" about a theme file nobody could write. A token vocabulary
with one hard-coded mapping is a palette, not a theme system.

## Decision

**One setting selects the theme: `theme.name`, a string, default `"ono"`.** It joins the
catalogue of ADR-0094, so it is set in a configuration file, at the prompt, or through
`ONO_THEME_NAME`, and `get config theme.name` shows the value with the layer that set it — no new
mechanism.

**The name is resolved in the layer order the rest of the configuration uses**: a theme this build
ships, then `/etc/ono/themes/<name>.toml`, then `<config dir>/themes/<name>.toml`. The last one
that exists wins, so an administrator can install a house theme and a user can still replace it.
A name that matches nothing is reported and the shell paints with the default — ADR-0010's promise
that a bad setting never stops the shell from starting holds for a bad *theme* too.

**Two themes ship**, because a theme setting with one theme to choose is not a theme system, and
because §44 asks for both: `ono`, dark and restrained, and `neon`, which uses the accent colours
harder. `neon` inherits every marker of `ono` unchanged rather than restating them — a louder
palette may not cost a reader without colour the mark they depend on.

**A theme file is TOML**: an optional `extends` naming the built-in theme it starts from, and a
`[tokens]` table keyed by the §44 token names, each value a table of `color` (0–255 or
`"default"`), `bold`, `dim`, `underline` and `marker`. Tokens the file does not name keep the
base's style. Anything the shell does not implement is refused, not ignored: an unknown token
name, an unknown style key, a value of the wrong kind, or a base theme that does not exist. A file
that is quietly half-applied is worse than one that is refused, because the user sees an effect
and cannot tell which half arrived.

**Three rules protect the reader from the theme**, which is §44's closing sentence made
mechanical:

1. *A theme is consulted only where there is colour.* `Theme::paint` returns the sanitised text
   unchanged for `Pipe`, `Redirect`, `Script` and `Plain`, so under `NO_COLOR`, on a dumb
   terminal and in every pipe, every theme prints identical bytes. No theme file can make output
   unreadable there, because no theme file is read there.
2. *A marker is held to what a value is held to.* It is printed verbatim beside the value it
   marks, so a marker containing a control character is refused (§49), and one longer than four
   characters is refused because it sits in a table cell.
3. *A theme may not make two opposite meanings look the same without colour.* `ui.danger`,
   `ui.warning` and `ui.success` must have markers, and pairwise different ones. A theme that
   gives danger the success marker is refused by name.

**The session owns the theme.** `config::load` resolves it once every layer is in, and the sinks,
the reporter, the REPL, the live view and the job output take it from the session instead of
constructing a default. That is what makes the setting have an effect at all.

**A theme file's refusals speak the configuration's error vocabulary** — `type.unknown_field` for
a name nothing defines, `type.mismatch` for a value of the wrong shape — because a theme file is
configuration, and `crates/ono-cli/src/settings.rs` already answers a bad configuration key that
way. A `theme.*` error family would be a second vocabulary for the same kind of mistake.

## Consequences

`Style` is no longer `Copy`: a marker read from a file cannot be a `&'static str`, so it is an
`Arc<str>` and `Style` is `Clone`. `Style::marker` returns `Option<&str>`. Nothing outside
`ono-render` constructed a `Style`, so the change did not reach a call site.

`ono-render` gains `toml` and `ono-core` as dependencies. The format lives there rather than in
`ono-cli` because the token vocabulary and the readability rules are the renderer's; `ono-cli`
owns only *where* the files are.

Encoded by `crates/ono-render/tests/theme_files.rs` (twelve cases, including
`should_paint_nothing_at_all_whatever_the_theme_when_the_destination_takes_no_colour`,
`should_refuse_a_marker_that_could_drive_the_terminal` and
`should_refuse_a_theme_that_leaves_danger_indistinguishable_from_success`),
`crates/ono-cli/tests/theme.rs` (seven cases) and acceptance case `150-themes`, which reads the
colour off a real terminal because a piped run is painted by no theme at all.

What is not delivered: no renderer *emits* the markers yet. They are the theme's answer to a
colourless destination and every theme is required to carry distinct ones, but the tables and the
error renderer print the plain text there. Emitting them is a rendering change with its own
snapshot consequences, and it is recorded here rather than half-done.

## Alternatives considered

- **Making the theme a set of `theme.<token>` settings** instead of a file. Rejected: twenty-four
  settings per theme, no way to distribute one, and `get config` becomes a colour table.
- **Reading themes from `docs/contracts/` like a registry.** Rejected: a theme is a user's preference,
  not a contract, and spec §30 already says where it lives.
- **Interning file markers into `&'static str` with `Box::leak`** to keep `Style: Copy`. Rejected:
  leaking to preserve a marker trait on a type nobody copies in a hot path is a cost with no
  payer.
- **Refusing any theme whose colours are hard to read** (contrast checking). Rejected as
  unfalsifiable without knowing the terminal's palette; the guarantee that matters is that the
  *colourless* rendering is beyond the theme's reach, and that one is exact.
