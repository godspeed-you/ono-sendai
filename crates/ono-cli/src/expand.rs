//! Word expansion, exactly as ADR-0019 fixes it: escape, then tilde, then variables, then globs.
//!
//! The rule that matters most is what is *absent*: an expanded variable is never word-split and
//! never globbed. A value's content can therefore never become a command's structure, which is
//! the mechanism behind the whole `"$@"` genre of shell bug and a large share of shell injection.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use ono_core::ErrorCode;
use ono_value::ErrorValue;

use crate::session::Session;

/// One word, expanded into the arguments it contributes.
///
/// A glob contributes several; everything else contributes exactly one.
pub fn expand_word(session: &Session, text: &str) -> Result<Vec<OsString>, ErrorValue> {
    let (literal, has_pattern) = substitute(session, text);

    if !has_pattern {
        return Ok(vec![OsString::from(literal)]);
    }
    let matches = glob(session.cwd(), &literal);
    if matches.is_empty() {
        return Err(ErrorValue::new(
            ErrorCode::IoNotFound,
            format!("no path matches `{literal}`"),
        )
        .with_help(
            "quote the pattern to pass it through literally, or check the directory you are in. \
             A pattern that matches nothing is refused rather than passed on as a filename \
             (ADR-0019).",
        ));
    }
    Ok(matches.into_iter().map(OsString::from).collect())
}

/// A word used where exactly one path is required, such as a redirection target.
pub fn expand_to_one(session: &Session, text: &str) -> Result<OsString, ErrorValue> {
    let mut expanded = expand_word(session, text)?;
    match expanded.len() {
        1 => Ok(expanded.remove(0)),
        count => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("`{text}` names {count} paths, and exactly one is needed here"),
        )
        .with_help("quote the word, or name a single path")),
    }
}

/// Applies escapes, tilde and variables, and reports whether an *unescaped* pattern character
/// survived — only those may drive a glob, so `\*` and a `*` that arrived inside a variable's
/// value stay literal (ADR-0019).
fn substitute(session: &Session, text: &str) -> (String, bool) {
    let mut out = String::with_capacity(text.len());
    let mut has_pattern = false;
    let mut chars = text.chars().peekable();
    let mut at_start = true;

    while let Some(character) = chars.next() {
        match character {
            '\\' => {
                if let Some(escaped) = chars.next() {
                    out.push(escaped);
                }
            }
            '~' if at_start && matches!(chars.peek(), None | Some('/')) => match session.home() {
                Some(home) => out.push_str(&home.to_string_lossy()),
                None => out.push('~'),
            },
            '$' => {
                let name = read_name(&mut chars);
                match name {
                    Some(name) => out.push_str(&lookup(session, &name)),
                    // A lone `$` is just a dollar sign.
                    None => out.push('$'),
                }
            }
            '*' | '?' | '[' => {
                has_pattern = true;
                out.push(character);
            }
            other => out.push(other),
        }
        at_start = false;
    }

    (out, has_pattern)
}

/// Reads the variable name after a `$`, in either the bare or the braced form.
fn read_name(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    if chars.peek() == Some(&'{') {
        chars.next();
        let mut name = String::new();
        for character in chars.by_ref() {
            if character == '}' {
                return Some(name);
            }
            name.push(character);
        }
        // An unclosed brace is not a name; the text is kept as typed.
        return None;
    }

    // `$?` is the last exit status: one character, and not an identifier.
    if chars.peek() == Some(&'?') {
        chars.next();
        return Some("?".to_owned());
    }

    let mut name = String::new();
    while let Some(&character) = chars.peek() {
        let acceptable = if name.is_empty() {
            character.is_alphabetic() || character == '_'
        } else {
            character.is_alphanumeric() || character == '_' || character == '.'
        };
        if !acceptable {
            break;
        }
        name.push(character);
        chars.next();
    }
    // A trailing `.` belongs to the surrounding text, not to the name.
    while name.ends_with('.') {
        name.pop();
    }
    if name.is_empty() { None } else { Some(name) }
}

/// Resolves `$name`: `env.NAME` names the environment explicitly; a bare name is a binding first
/// and the environment second (ADR-0010).
fn lookup(session: &Session, name: &str) -> String {
    if name == "?" || name == "status" {
        return session.status().code().to_string();
    }
    if let Some(variable) = name.strip_prefix("env.") {
        return session
            .env_var(variable)
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    if let Some(value) = session.binding(name) {
        return ono_value::canonical_text(value).unwrap_or_default();
    }
    session
        .env_var(name)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Expands a glob against the filesystem, component by component, in sorted order.
///
/// `**` is deliberately not a recursive descent (ADR-0019): a construct that differs from `*` by
/// one character and by orders of magnitude of effect is a construct people trigger by accident.
fn glob(cwd: &Path, pattern: &str) -> Vec<String> {
    let path = Path::new(pattern);
    let absolute = path.is_absolute();

    let mut components: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => components.push(part.to_string_lossy().into_owned()),
            Component::RootDir => components.push("/".to_owned()),
            Component::CurDir => components.push(".".to_owned()),
            Component::ParentDir => components.push("..".to_owned()),
            Component::Prefix(_) => {}
        }
    }

    // Each round expands one component. `prefixes` holds the paths as they will be printed;
    // `roots` holds where to look on disk, which differs for a relative pattern.
    let mut prefixes: Vec<String> = vec![String::new()];
    let mut first = true;

    for component in components {
        if component == "/" {
            prefixes = vec!["/".to_owned()];
            first = false;
            continue;
        }
        if !is_pattern(&component) {
            prefixes = prefixes
                .into_iter()
                .map(|prefix| join(&prefix, &component, first && !absolute))
                .collect();
            first = false;
            continue;
        }

        let mut next = Vec::new();
        for prefix in &prefixes {
            let directory = if prefix.is_empty() {
                cwd.to_path_buf()
            } else if absolute || prefix.starts_with('/') {
                PathBuf::from(prefix)
            } else {
                cwd.join(prefix)
            };
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };
            let mut names: Vec<String> = entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| matches(&component, name))
                .collect();
            names.sort_unstable();
            for name in names {
                next.push(join(prefix, &name, first && !absolute));
            }
        }
        prefixes = next;
        first = false;
    }

    prefixes.retain(|candidate| !candidate.is_empty());
    prefixes
}

fn join(prefix: &str, component: &str, bare: bool) -> String {
    if prefix.is_empty() || bare {
        component.to_owned()
    } else if prefix == "/" {
        format!("/{component}")
    } else {
        format!("{prefix}/{component}")
    }
}

fn is_pattern(component: &str) -> bool {
    let mut chars = component.chars();
    while let Some(character) = chars.next() {
        match character {
            '\\' => {
                chars.next();
            }
            '*' | '?' | '[' => return true,
            _ => {}
        }
    }
    false
}

/// Whether `name` matches `pattern`, with `*`, `?` and `[…]`.
///
/// A leading `.` is matched only by a literal `.`, as everywhere else, so `*` never returns the
/// hidden files nobody asked for.
fn matches(pattern: &str, name: &str) -> bool {
    if name.starts_with('.') && !pattern.starts_with('.') {
        return false;
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    matches_from(&pattern, 0, &name, 0)
}

fn matches_from(pattern: &[char], mut p: usize, name: &[char], mut n: usize) -> bool {
    // Iterative with one backtrack point for `*`, so a pathological pattern cannot blow the
    // stack or the clock the way naive recursion does.
    let mut star: Option<(usize, usize)> = None;

    while n < name.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, n));
                p += 1;
            }
            Some('?') => {
                p += 1;
                n += 1;
            }
            Some('[') => match class(pattern, p, name[n]) {
                Some((next, true)) => {
                    p = next;
                    n += 1;
                }
                Some((_, false)) | None => match star {
                    Some((star_p, star_n)) => {
                        p = star_p + 1;
                        n = star_n + 1;
                        star = Some((star_p, star_n + 1));
                    }
                    None => return false,
                },
            },
            Some(&literal) if literal == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((star_p, star_n)) => {
                    p = star_p + 1;
                    n = star_n + 1;
                    star = Some((star_p, star_n + 1));
                }
                None => return false,
            },
        }
    }

    while pattern.get(p) == Some(&'*') {
        p += 1;
    }
    p == pattern.len()
}

/// Matches a `[…]` class starting at `open`, returning where it ends and whether it matched.
fn class(pattern: &[char], open: usize, candidate: char) -> Option<(usize, bool)> {
    let mut index = open + 1;
    let negated = matches!(pattern.get(index), Some('!' | '^'));
    if negated {
        index += 1;
    }
    let mut found = false;
    let mut first = true;

    while let Some(&character) = pattern.get(index) {
        if character == ']' && !first {
            return Some((index + 1, found != negated));
        }
        first = false;
        if pattern.get(index + 1) == Some(&'-')
            && let Some(&upper) = pattern.get(index + 2)
            && upper != ']'
        {
            if character <= candidate && candidate <= upper {
                found = true;
            }
            index += 3;
            continue;
        }
        if character == candidate {
            found = true;
        }
        index += 1;
    }
    // An unclosed class is a literal `[`.
    None
}
