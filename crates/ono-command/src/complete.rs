//! Completion candidates from metadata (spec §15.1).
//!
//! Spec §34 budgets 50 ms for the first results, so everything here is a lookup in the registry
//! and nothing is a search. What the registry cannot know — the users on this machine, the
//! services of this host — is not guessed at: [`ValueCompleter`] is the hook the caller fills in,
//! and without it the candidate list is honestly empty rather than plausibly wrong.

use std::sync::Arc;

use ono_value::Schema;

use crate::contract::{ArgumentMode, CommandContract, DeclaredType, ParameterSpec};
use crate::registry::CommandRegistry;

/// What kind of thing a candidate is, so the editor can present it accordingly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CandidateKind {
    /// A verb in command position.
    Verb,
    /// A target word after a verb.
    Target,
    /// A `--named` option.
    Option,
    /// A value for a selector or an option.
    Value,
    /// A field of the schema flowing into the stage (spec §15.1).
    Field,
}

/// One completion candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    text: String,
    kind: CandidateKind,
    doc: Option<String>,
}

impl Candidate {
    /// A candidate of an explicit kind.
    #[must_use]
    pub fn new(text: impl Into<String>, kind: CandidateKind) -> Self {
        Self {
            text: text.into(),
            kind,
            doc: None,
        }
    }

    /// A verb candidate.
    #[must_use]
    pub fn verb(text: impl Into<String>) -> Self {
        Self::new(text, CandidateKind::Verb)
    }

    /// A target candidate.
    #[must_use]
    pub fn target(text: impl Into<String>) -> Self {
        Self::new(text, CandidateKind::Target)
    }

    /// An option candidate, written as it would be typed.
    #[must_use]
    pub fn option(text: impl Into<String>) -> Self {
        Self::new(text, CandidateKind::Option)
    }

    /// A value candidate.
    #[must_use]
    pub fn value(text: impl Into<String>) -> Self {
        Self::new(text, CandidateKind::Value)
    }

    /// A field candidate.
    #[must_use]
    pub fn field(text: impl Into<String>) -> Self {
        Self::new(text, CandidateKind::Field)
    }

    /// Attaches the one line the editor shows beside the candidate.
    #[must_use]
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = Some(doc.into());
        self
    }

    /// The text that replaces the token being typed.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// What kind of thing it is.
    #[must_use]
    pub fn kind(&self) -> CandidateKind {
        self.kind
    }

    /// The line shown beside it, where the registry documents one.
    #[must_use]
    pub fn doc(&self) -> Option<&str> {
        self.doc.as_deref()
    }
}

/// The stage the cursor is in: what has been typed so far, and what is being typed now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StageContext {
    head: Option<String>,
    words: Vec<String>,
    prefix: String,
    /// The pipeline text before the `|` that opened this stage, when there is one: what
    /// decides which schema's fields `where` and `select` complete (spec §15.1, ADR-0074).
    upstream: Option<String>,
}

impl StageContext {
    /// A context assembled from parts, for a caller that already has a parsed stage.
    #[must_use]
    pub fn new(head: Option<&str>, words: &[&str], prefix: &str) -> Self {
        Self {
            head: head.map(str::to_owned),
            words: words.iter().map(|word| (*word).to_owned()).collect(),
            prefix: prefix.to_owned(),
            upstream: None,
        }
    }

    /// The context at `cursor` in `line`.
    ///
    /// Only the stage the cursor is in matters, so everything up to the last `|`, `;`, `&&` or
    /// `||` is discarded. The token under the cursor is the prefix; everything before it is
    /// already typed.
    #[must_use]
    pub fn from_line(line: &str, cursor: usize) -> Self {
        let typed = &line[..cursor.min(line.len())];
        let cut = typed.rfind(['|', ';', '&']).map_or(0, |index| index + 1);
        let stage = &typed[cut..];
        // Only a pipe hands a schema on; a `;` or an `&&` starts the stage from nothing.
        let upstream = cut
            .checked_sub(1)
            .filter(|index| typed.as_bytes().get(*index) == Some(&b'|'))
            .map(|index| typed[..index].to_owned());

        let mut tokens: Vec<String> = stage.split_whitespace().map(str::to_owned).collect();
        let prefix = if stage.ends_with(char::is_whitespace) || stage.is_empty() {
            String::new()
        } else {
            tokens.pop().unwrap_or_default()
        };
        let head = if tokens.is_empty() {
            None
        } else {
            Some(tokens.remove(0))
        };
        Self {
            head,
            words: tokens,
            prefix,
            upstream,
        }
    }

    /// The stage's head, once it has been typed in full.
    #[must_use]
    pub fn head(&self) -> Option<&str> {
        self.head.as_deref()
    }

    /// The arguments already typed in full.
    #[must_use]
    pub fn words(&self) -> &[String] {
        &self.words
    }

    /// The token under the cursor.
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }
}

/// What only a provider can complete: the users on this machine, the services of this host.
///
/// The registry offers what metadata knows and stops there; this hook is where the caller adds
/// the rest. Spec §15.1 wants completion to be provider-aware, and this is the seam.
pub trait ValueCompleter {
    /// The values for `parameter` of `command` that begin with `prefix`.
    fn complete(
        &self,
        command: &CommandContract,
        parameter: &ParameterSpec,
        prefix: &str,
    ) -> Vec<Candidate>;
}

/// The candidates for the token under the cursor, sorted and without repeats.
///
/// ```
/// use ono_command::{CommandRegistry, StageContext};
///
/// let registry = CommandRegistry::embedded()?;
/// let candidates = ono_command::complete(registry, &StageContext::from_line("get pro", 7), None);
/// assert_eq!(candidates[0].text(), "process");
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[must_use]
pub fn complete(
    registry: &CommandRegistry,
    context: &StageContext,
    values: Option<&dyn ValueCompleter>,
) -> Vec<Candidate> {
    let mut candidates = gather(registry, context, values);
    candidates.sort_by(|left, right| left.text.cmp(&right.text));
    candidates.dedup_by(|left, right| left.text == right.text);
    candidates
}

fn gather(
    registry: &CommandRegistry,
    context: &StageContext,
    values: Option<&dyn ValueCompleter>,
) -> Vec<Candidate> {
    let Some(head) = context.head() else {
        return registry
            .verbs()
            .iter()
            .filter(|verb| verb.verb().starts_with(context.prefix()))
            .map(|verb| Candidate::verb(verb.verb()).with_doc(verb.semantics()))
            .collect();
    };

    let command = resolve(registry, head, context.words());

    if let Some(rest) = context.prefix().strip_prefix("--") {
        let Some(command) = command else {
            return Vec::new();
        };
        return match rest.split_once('=') {
            Some((name, written)) => option_values(command, name, written, values),
            None => command
                .options()
                .iter()
                .filter(|option| option.name().starts_with(rest))
                .map(|option| {
                    Candidate::option(format!("--{}", option.name())).with_doc(option.doc())
                })
                .collect(),
        };
    }

    // A target word is still expected while nothing but the verb has been typed.
    if context.words().is_empty() {
        let targets = registry.targets_for_verb(head);
        if !targets.is_empty() {
            return targets
                .into_iter()
                .filter(|target| target.starts_with(context.prefix()))
                .map(|target| {
                    let doc = registry
                        .target(target)
                        .map(|entry| entry.summary().to_owned());
                    let candidate = Candidate::target(target);
                    match doc {
                        Some(doc) => candidate.with_doc(doc),
                        None => candidate,
                    }
                })
                .collect();
        }
    }

    let Some(command) = command else {
        return Vec::new();
    };
    let selector = next_selector(command, context);
    // Spec §15.1: `get process | where <tab>` shows Process fields. The schema is the one the
    // stages before the pipe hand on, read from the contracts — nothing runs (ADR-0074).
    if let Some(selector) = selector
        && reads_fields(command, selector)
        && let Some(schema) = upstream_schema(registry, context)
    {
        return schema
            .fields()
            .iter()
            .filter(|field| field.name().starts_with(context.prefix()))
            .map(|field| {
                let candidate = Candidate::field(field.name());
                match field.doc() {
                    Some(doc) => candidate.with_doc(doc),
                    None => candidate,
                }
            })
            .collect();
    }
    match selector {
        Some(selector) => selector_values(command, selector, context.prefix(), values),
        None => Vec::new(),
    }
}

/// Whether an argument bound to `selector` reads fields of the stream: an expression-mode
/// parameter that carries values. A string parameter — `sort cpu desc`'s direction — is
/// vocabulary, exactly as the pre-flight check treats it.
fn reads_fields(command: &CommandContract, selector: &ParameterSpec) -> bool {
    command.argument_mode() == ArgumentMode::Expression
        && selector.declared_type() != &DeclaredType::String
}

/// The schema flowing into the stage under the cursor, where the pipeline before it declares one.
fn upstream_schema(registry: &CommandRegistry, context: &StageContext) -> Option<Arc<Schema>> {
    let upstream = context.upstream.as_deref()?;
    let parsed = ono_parser::parse(upstream);
    let pipeline = parsed.program().statements.last()?.as_pipeline()?;
    let schemas: Vec<Arc<Schema>> = ono_value::builtin_schemas().schemas().cloned().collect();
    crate::check::schema_after(registry, &schemas, pipeline)
}

/// The command a head and the words typed after it name, if the registry has one.
fn resolve<'a>(
    registry: &'a CommandRegistry,
    head: &str,
    words: &[String],
) -> Option<&'a CommandContract> {
    words
        .first()
        .and_then(|target| registry.find(head, Some(target)))
        .or_else(|| registry.find(head, None))
}

/// The selector the next positional word would bind to.
fn next_selector<'a>(
    command: &'a CommandContract,
    context: &StageContext,
) -> Option<&'a ParameterSpec> {
    let target_words = usize::from(command.target().is_some());
    let mut positional = context
        .words()
        .iter()
        .filter(|word| !word.starts_with("--"))
        .count();
    positional = positional.saturating_sub(target_words);
    command.selectors().get(positional).or_else(|| {
        command
            .selectors()
            .last()
            .filter(|last| last.is_repeatable())
    })
}

fn selector_values(
    command: &CommandContract,
    selector: &ParameterSpec,
    prefix: &str,
    values: Option<&dyn ValueCompleter>,
) -> Vec<Candidate> {
    // An expression-mode selector is a field path or a predicate, which only a schema can
    // complete; the closed set of its declared type would be a wrong answer offered confidently.
    if command.argument_mode() == ArgumentMode::Words {
        let closed = selector.closed_set();
        if !closed.is_empty() {
            return closed
                .into_iter()
                .filter(|value| value.starts_with(prefix))
                .map(|value| Candidate::value(value).with_doc(selector.doc()))
                .collect();
        }
    }
    values.map_or_else(Vec::new, |hook| hook.complete(command, selector, prefix))
}

fn option_values(
    command: &CommandContract,
    name: &str,
    written: &str,
    values: Option<&dyn ValueCompleter>,
) -> Vec<Candidate> {
    let Some(option) = command.option(name) else {
        return Vec::new();
    };
    let closed = option.closed_set();
    let offered = if closed.is_empty() {
        values.map_or_else(Vec::new, |hook| hook.complete(command, option, written))
    } else {
        closed
            .into_iter()
            .filter(|value| value.starts_with(written))
            .map(|value| Candidate::value(value).with_doc(option.doc()))
            .collect()
    };
    offered
        .into_iter()
        .map(|candidate| Candidate {
            text: format!("--{name}={}", candidate.text),
            kind: CandidateKind::Value,
            doc: candidate.doc,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::StageContext;

    #[test]
    fn should_read_the_stage_under_the_cursor() {
        let context = StageContext::from_line("get process | where cp", 22);
        assert_eq!(context.head(), Some("where"));
        assert_eq!(context.prefix(), "cp");
        assert!(context.words().is_empty());
    }

    #[test]
    fn should_treat_a_trailing_space_as_a_finished_word() {
        let context = StageContext::from_line("get process ", 12);
        assert_eq!(context.head(), Some("get"));
        assert_eq!(context.words(), ["process"]);
        assert_eq!(context.prefix(), "");
    }
}
