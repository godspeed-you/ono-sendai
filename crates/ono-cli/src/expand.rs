//! Word expansion, exactly as ADR-0019 fixes it: escape, then tilde, then variables, then globs.
//!
//! The rule that matters most is what is *absent*: an expanded variable is never word-split and
//! never globbed. A value's content can therefore never become a command's structure, which is
//! the mechanism behind the whole `"$@"` genre of shell bug and a large share of shell injection.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use ono_core::ErrorCode;
use ono_parser::{Argument, WordArg};
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

/// The arguments of a native stage with every unquoted glob resolved to the paths it matches.
///
/// Spec §17.3: a native command receives resolved objects, so `get file *.txt` and
/// `remove file *.tmp` know their exact targets before the provider hears a word. Only a bare
/// word carrying an unescaped pattern character is expanded — a quoted `"*.md"` is an
/// expression the parser kept as text, and stays the literal an option such as `--name` wants.
/// Each match takes the span of the word it came from, so a diagnostic still points at what was
/// typed. A pattern that matches nothing is refused, exactly as it is for a program (ADR-0019).
pub fn expand_globs(
    session: &Session,
    arguments: &[Argument],
) -> Result<Vec<Argument>, ErrorValue> {
    let mut expanded = Vec::with_capacity(arguments.len());
    for argument in arguments {
        let Argument::Word(word) = argument else {
            expanded.push(argument.clone());
            continue;
        };
        let (literal, has_pattern) = substitute(session, &word.text);
        if !has_pattern {
            expanded.push(argument.clone());
            continue;
        }
        let matches = glob(session.cwd(), &literal);
        if matches.is_empty() {
            return Err(ErrorValue::new(
                ErrorCode::IoNotFound,
                format!("no path matches `{literal}`"),
            )
            .with_help(
                "quote the pattern to pass it through literally, or check the directory you \
                 are in. A pattern that matches nothing is refused rather than passed on as a \
                 filename (ADR-0019).",
            ));
        }
        expanded.extend(matches.into_iter().map(|text| {
            Argument::Word(WordArg {
                text,
                span: word.span,
            })
        }));
    }
    Ok(expanded)
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
    // Indexed rather than drained, so a construct that turns out not to be one can be put back.
    // An earlier version consumed the rest of the word looking for the `}` of a `${…}` and
    // dropped everything it had read when there was none, so `cp report.txt ${dest` silently
    // became `cp report.txt $` — losing data inside an argument, which is the class of surprise
    // ADR-0019 exists to remove.
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        let character = chars[index];
        match character {
            '\\' => {
                if let Some(escaped) = chars.get(index + 1) {
                    out.push(*escaped);
                    index += 2;
                } else {
                    index += 1;
                }
            }
            '~' if index == 0 && matches!(chars.get(1), None | Some('/')) => {
                match session.home() {
                    Some(home) => out.push_str(&home.to_string_lossy()),
                    None => out.push('~'),
                }
                index += 1;
            }
            '$' => match read_name(&chars, index + 1) {
                Some((name, next)) => {
                    out.push_str(&lookup(session, &name));
                    index = next;
                }
                // Not a name after all — a lone `$`, or a `${` nobody closed. The dollar is kept
                // and everything after it is left exactly as it was typed.
                None => {
                    out.push('$');
                    index += 1;
                }
            },
            '*' | '?' | '[' => {
                has_pattern = true;
                out.push(character);
                index += 1;
            }
            other => {
                out.push(other);
                index += 1;
            }
        }
    }

    (out, has_pattern)
}

/// Reads the variable name starting at `from`, in either the bare or the braced form.
///
/// Returns the name and the index just past it, or `None` when what follows the `$` is not a
/// name — in which case nothing has been consumed and the caller keeps the text as typed.
fn read_name(chars: &[char], from: usize) -> Option<(String, usize)> {
    if chars.get(from) == Some(&'{') {
        let mut index = from + 1;
        let mut name = String::new();
        while let Some(&character) = chars.get(index) {
            if character == '}' {
                return (!name.is_empty()).then_some((name, index + 1));
            }
            name.push(character);
            index += 1;
        }
        // An unclosed brace is not a name, and nothing has been consumed.
        return None;
    }

    // `$?` is the last exit status: one character, and not an identifier.
    if chars.get(from) == Some(&'?') {
        return Some(("?".to_owned(), from + 1));
    }

    let mut index = from;
    let mut name = String::new();
    while let Some(&character) = chars.get(index) {
        let acceptable = if name.is_empty() {
            character.is_alphabetic() || character == '_'
        } else {
            character.is_alphanumeric() || character == '_' || character == '.'
        };
        if !acceptable {
            break;
        }
        name.push(character);
        index += 1;
    }
    // A trailing `.` belongs to the surrounding text, not to the name.
    while name.ends_with('.') {
        name.pop();
        index -= 1;
    }
    if name.is_empty() {
        None
    } else {
        Some((name, index))
    }
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
