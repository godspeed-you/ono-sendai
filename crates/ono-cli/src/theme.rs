//! Finding the theme the configuration names (spec §44, §30).
//!
//! `theme.name` selects it, and it is resolved in the layer order the rest of the configuration
//! uses (ADR-0010): a theme this build ships, then `/etc/ono/themes/<name>.toml`, then
//! `<config dir>/themes/<name>.toml`. The last one that exists wins, so an administrator can put
//! a house theme on the machine and a user can still write their own.
//!
//! Nothing here can stop a shell from starting. A theme that cannot be found, read or understood
//! is reported the way a bad setting is — on stderr, and as a value through
//! `get config --problems` — and the shell paints with the default theme (ADR-0010, ADR-0332).

use std::path::PathBuf;

use ono_core::ErrorCode;
use ono_render::Theme;
use ono_value::ErrorValue;

use crate::report::Reporter;
use crate::session::Session;

/// Resolves `theme.name` into the theme the session paints with.
///
/// Problems are reported through `reporter`, recorded in the settings, and never fatal.
pub fn load(session: &mut Session, reporter: &Reporter) {
    let name = session
        .settings()
        .text("theme.name")
        .unwrap_or("ono")
        .to_owned();

    match resolve(session, &name) {
        Ok(theme) => session.set_theme(theme),
        Err(error) => {
            reporter.error(&error);
            session.settings_mut().note_problem(&error);
        }
    }
}

/// The theme called `name`, from the files first and the built-ins second.
fn resolve(session: &Session, name: &str) -> Result<Theme, ErrorValue> {
    let mut found: Option<Theme> = Theme::named(name);

    for directory in directories(session) {
        let path = directory.join(format!("{name}.toml"));
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            // A theme file that is simply not there is the ordinary case.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(crate::builtin::io_error(&path, &error)),
        };
        found = Some(Theme::parse(name, &text).map_err(|error| {
            error.with_help(format!(
                "the theme is `{}`; until it reads, the shell paints with the default theme",
                path.display()
            ))
        })?);
    }

    found.ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::TypeUnknownField,
            format!("no theme is called `{name}`"),
        )
        .with_help(format!(
            "this build ships {}; a theme of your own is a `themes/{name}.toml` beside your \
             configuration (spec §30)",
            Theme::builtin_names().join(", ")
        ))
    })
}

/// Where theme files live, in the order they override each other (spec §30).
fn directories(session: &Session) -> Vec<PathBuf> {
    let mut directories = vec![
        PathBuf::from("/etc")
            .join(ono_core::SHORT_NAME)
            .join("themes"),
    ];
    directories
        .extend(crate::config::user_config_dir(session).map(|directory| directory.join("themes")));
    directories
}
