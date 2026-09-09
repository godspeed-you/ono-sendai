//! Reading one statement of a plan block (spec §5.2, §31, ADR-0813).
//!
//! §5.2 writes a plan as a block of statements, and ADR-0813 fixes what a statement is: an
//! ordinary Ono command spelling, resolved through the same registry the shell resolves a command
//! through. That is what makes §6.1's plannability question answerable at all — an operation is
//! plannable when a contract declares its semantics, and the contract is the one
//! `docs/contracts/commands/` already holds.
//!
//! One head is treated apart. A `verify` line asks a question about the system afterwards rather
//! than changing it, so §23.1 makes it a verification contract and §4.7's APPLYING phase does not
//! run it (ADR-0813).
//!
//! Nothing here builds a command line. A word is a word: it is coerced to the type its parameter
//! declares and carried as a [`Value`], and §43.6's quoting question never arises because there
//! is no string to quote it into.

use std::sync::Arc;

use ono_command::{CommandContract, CommandRegistry};
use ono_core::ErrorCode;
use ono_value::{ErrorValue, Value};

/// The head that makes a statement a question rather than a change (§23.1, ADR-0813).
pub(crate) const VERIFY: &str = "verify";

/// Words that join two operands in a plan statement and name neither of them.
///
/// `copy file ./nginx.conf to /etc/nginx/nginx.conf` reads the way a person writes it, and `to`
/// is punctuation. Skipping them here keeps the selector positions the contract declares.
const CONNECTIVES: &[&str] = &["to", "from", "into", "onto", "with", "as", "at"];

/// One argument of a statement: the name the contract gives it, and its value already typed.
pub(crate) type Argument = (Arc<str>, Value);

/// One line of a plan block.
#[derive(Debug)]
pub(crate) enum Statement<'r> {
    /// A change: a command the registry resolves, with its arguments already typed.
    Mutation(Mutation<'r>),
    /// A `verify` line, which §23.1 makes a contract rather than an action.
    Check(Check),
}

/// A resolved change statement.
#[derive(Debug)]
pub(crate) struct Mutation<'r> {
    /// The command contract the head named.
    pub(crate) contract: &'r CommandContract,
    /// The selector values, by the names the contract gives them, in contract order.
    pub(crate) selectors: Vec<Argument>,
    /// The option values, by name.
    pub(crate) options: Vec<Argument>,
    /// The line as the operator wrote it, kept for `explain` (§4.3).
    pub(crate) text: Arc<str>,
}

impl Mutation<'_> {
    /// The selector that names the object being acted on.
    ///
    /// The first declared selector that was written, which is the rule the rest of the shell
    /// already resolves a mutation's target by (ADR-0082 §2): `copy file <source> to
    /// <destination>` acts on the source and carries the destination as an argument.
    pub(crate) fn object_selector(&self) -> Option<&Argument> {
        self.contract.selectors().iter().find_map(|spec| {
            self.selectors
                .iter()
                .find(|(name, _)| name.as_ref() == spec.name())
        })
    }

    /// The value of one selector, where the line gave it.
    pub(crate) fn selector(&self, name: &str) -> Option<&Value> {
        self.selectors
            .iter()
            .find(|(declared, _)| declared.as_ref() == name)
            .map(|(_, value)| value)
    }
}

/// A `verify` line: what to look at, and what should be true of it.
#[derive(Debug)]
pub(crate) struct Check {
    /// The object, spelled as the plan block writes it — `service nginx`, `socket :443`.
    pub(crate) subject: Arc<str>,
    /// The condition, as a person and a machine both read it.
    pub(crate) expression: Arc<str>,
}

/// The statements of a plan block, in order, with blank lines and comments dropped.
pub(crate) fn statements(text: &str) -> Vec<&str> {
    text.lines()
        .flat_map(|line| line.split(';'))
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

/// Reads one statement against the command registry.
///
/// # Errors
///
/// `resolve.command_not_found` when nothing in the registry answers to the head, which §6.2 turns
/// into a refusal rather than a guess.
pub(crate) fn parse<'r>(
    registry: &'r CommandRegistry,
    line: &str,
) -> Result<Statement<'r>, ErrorValue> {
    let mut words = line.split_whitespace();
    let head = words.next().ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            "an empty statement names no operation".to_owned(),
        )
    })?;
    let rest: Vec<&str> = words.collect();
    if head == VERIFY {
        return Ok(Statement::Check(check(registry, &rest)));
    }
    let (contract, remainder) = match rest
        .first()
        .and_then(|word| registry.find(head, Some(word)))
    {
        Some(contract) => (contract, &rest[1..]),
        None => (
            registry.find(head, None).ok_or_else(|| {
                ErrorValue::new(
                    ErrorCode::ResolveCommandNotFound,
                    format!("no command answers to `{line}`"),
                )
                .with_help(
                    "a plan block holds ordinary Ono command spellings (ADR-0813). `help` lists \
                     them",
                )
            })?,
            &rest[..],
        ),
    };
    let (selectors, options) = bind(contract, remainder)?;
    Ok(Statement::Mutation(Mutation {
        contract,
        selectors,
        options,
        text: Arc::from(line),
    }))
}

/// Splits a `verify` line into the object it is about and the condition it asserts.
///
/// §31 writes both shapes: `verify service nginx state == running` names a field and a value, and
/// `verify socket :443 exists` names only that the object is there. The subject is the target word
/// and the object's label, because that pair is what a provider can be asked for.
fn check(registry: &CommandRegistry, words: &[&str]) -> Check {
    let names_target = words
        .first()
        .is_some_and(|word| registry.target(word).is_some());
    let split = if names_target { 2.min(words.len()) } else { 1 };
    let subject = words[..split].join(" ");
    let expression = words[split..].join(" ");
    Check {
        subject: Arc::from(subject.as_str()),
        expression: Arc::from(if expression.is_empty() {
            "exists".to_owned()
        } else {
            expression
        }),
    }
}

/// Binds the words of a statement to the selectors and options the contract declares.
///
/// Bare words fill the declared selectors in order; a surplus word after a connective becomes the
/// `source` argument, which is where §31's `write file <path> from <content>` puts the thing being
/// read. An option's value is coerced to the type its parameter declares, so a plan carries an
/// `int` where the contract says `int` rather than the digits somebody typed.
fn bind(
    contract: &CommandContract,
    words: &[&str],
) -> Result<(Vec<Argument>, Vec<Argument>), ErrorValue> {
    let mut positional: Vec<&str> = Vec::new();
    let mut options: Vec<Argument> = Vec::new();
    let mut index = 0;
    while index < words.len() {
        let word = words[index];
        index += 1;
        let Some(flag) = word.strip_prefix("--") else {
            if !CONNECTIVES.contains(&word) {
                positional.push(word);
            }
            continue;
        };
        let (name, inline) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (flag, None),
        };
        let spec = contract.option(name).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("`{}` declares no option `--{name}`", contract.spelling()),
            )
            .with_help(format!(
                "`help {}` lists what it takes",
                contract.spelling()
            ))
        })?;
        let value = match (inline, spec.declared_type().name().as_str()) {
            (Some(text), _) => spec.declared_type().coerce(text)?,
            (None, "bool") => Value::Bool(true),
            (None, _) => {
                let text = words.get(index).ok_or_else(|| {
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        format!("`--{name}` was written without a value"),
                    )
                })?;
                index += 1;
                spec.declared_type().coerce(text)?
            }
        };
        options.push((Arc::from(name), value));
    }

    let mut selectors: Vec<Argument> = Vec::new();
    let mut remaining = positional.into_iter();
    for spec in contract.selectors() {
        let Some(text) = remaining.next() else { break };
        selectors.push((Arc::from(spec.name()), spec.declared_type().coerce(text)?));
    }
    let surplus: Vec<&str> = remaining.collect();
    if !surplus.is_empty() {
        options.push((Arc::from("source"), Value::string(&surplus.join(" "))));
    }
    Ok((selectors, options))
}
