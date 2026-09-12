//! Which operators compare a field of a given type, and what the language says each one means.
//!
//! Three sources already existed and none of them held this relation. The grammar fixes the
//! vocabulary (`docs/contracts/grammar.ebnf`, the parser's [`BinaryOp`]), the language contract
//! documents it (`docs/contracts/language.yaml`), and the evaluator decides what executes
//! (`crate::expr`). What completion needs on top is the answer to "after a field of this declared
//! type, which comparisons are worth offering?" — and that answer is kept here, once, as a subset
//! of what the evaluator accepts. `tests/operator_typing.rs` evaluates every pair it names, so a
//! comparison offered here is one that runs (ADR-0860).

use std::sync::OnceLock;

use ono_parser::BinaryOp;
use ono_value::FieldType;
use serde::Deserialize;

/// `docs/contracts/language.yaml` as it is on disk. Unlike the registries it is not transcoded at
/// build time (ADR-0571): it is read only when completion first asks, never at startup, and one of
/// its maps has keys JSON cannot carry.
const LANGUAGE_FILE: &str = include_str!("../../../docs/contracts/language.yaml");

/// Equality alone: the value can be told apart from another, and nothing more is meaningful.
const EQUALITY: &[BinaryOp] = &[BinaryOp::Eq, BinaryOp::NotEq];

/// Equality, ordering and membership: numbers, quantities, instants, ports, and enums, which
/// order by their declared variants (ADR-0222).
const ORDERED: &[BinaryOp] = &[
    BinaryOp::Eq,
    BinaryOp::NotEq,
    BinaryOp::Lt,
    BinaryOp::LtEq,
    BinaryOp::Gt,
    BinaryOp::GtEq,
    BinaryOp::In,
    BinaryOp::NotIn,
];

/// Equality, regex matching and membership: text, where ordering is executable but is a byte
/// order nobody means when they filter.
const TEXT: &[BinaryOp] = &[
    BinaryOp::Eq,
    BinaryOp::NotEq,
    BinaryOp::Match,
    BinaryOp::NotMatch,
    BinaryOp::In,
    BinaryOp::NotIn,
];

/// Equality and membership: identities such as addresses and uuids.
const IDENTITY: &[BinaryOp] = &[BinaryOp::Eq, BinaryOp::NotEq, BinaryOp::In, BinaryOp::NotIn];

/// Every comparison, for a field whose declared type promises nothing.
const EVERY: &[BinaryOp] = &[
    BinaryOp::Eq,
    BinaryOp::NotEq,
    BinaryOp::Lt,
    BinaryOp::LtEq,
    BinaryOp::Gt,
    BinaryOp::GtEq,
    BinaryOp::In,
    BinaryOp::NotIn,
    BinaryOp::Match,
    BinaryOp::NotMatch,
];

/// The comparison and membership operators offered after a field of type `ty` (ADR-0860).
///
/// ```
/// use ono_parser::BinaryOp;
/// use ono_value::FieldType;
///
/// assert!(ono_command::comparisons_for(&FieldType::ByteSize).contains(&BinaryOp::Gt));
/// assert!(!ono_command::comparisons_for(&FieldType::Bool).contains(&BinaryOp::Gt));
/// ```
#[must_use]
pub fn comparisons_for(ty: &FieldType) -> &'static [BinaryOp] {
    match ty {
        FieldType::Int
        | FieldType::Float
        | FieldType::Decimal
        | FieldType::ByteSize
        | FieldType::Percent
        | FieldType::Duration
        | FieldType::Timestamp
        | FieldType::Port
        | FieldType::Enum(_) => ORDERED,
        FieldType::String | FieldType::Path => TEXT,
        FieldType::Ip | FieldType::IpNetwork | FieldType::Uuid | FieldType::Ref(_) => IDENTITY,
        FieldType::Any => EVERY,
        FieldType::Bool
        | FieldType::Bytes
        | FieldType::Regex
        | FieldType::List(_)
        | FieldType::Map
        | FieldType::Record(_)
        | FieldType::Error => EQUALITY,
    }
}

/// How `op` is written in source, as `docs/contracts/grammar.ebnf` spells it.
#[must_use]
pub const fn operator_symbol(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Or => "or",
        BinaryOp::And => "and",
        BinaryOp::Eq => "==",
        BinaryOp::NotEq => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::LtEq => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::GtEq => ">=",
        BinaryOp::In => "in",
        BinaryOp::NotIn => "not in",
        BinaryOp::Match => "~=",
        BinaryOp::NotMatch => "!~=",
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Rem => "%",
    }
}

/// The one line `docs/contracts/language.yaml` gives the infix operator `op`.
pub(crate) fn operator_doc(op: BinaryOp) -> Option<&'static str> {
    let symbol = operator_symbol(op);
    language()
        .operators
        .expression
        .iter()
        .find(|entry| entry.arity == "binary" && entry.symbol == symbol)
        .map(|entry| entry.doc.as_str())
}

/// The unit suffixes a literal compared with a field of type `ty` may carry, each with its doc —
/// `docs/contracts/language.yaml`'s `unit_dimensions`, which restate the lexer's suffix table.
pub(crate) fn units_for(ty: &FieldType) -> Vec<(&'static str, &'static str)> {
    let dimension = match ty {
        FieldType::ByteSize => "bytesize",
        FieldType::Duration => "duration",
        FieldType::Percent => "ratio",
        _ => return Vec::new(),
    };
    language()
        .unit_dimensions
        .iter()
        .filter(|entry| entry.name == dimension)
        .flat_map(|entry| entry.units.iter())
        .map(|unit| (unit.suffix.as_str(), unit.doc.as_str()))
        .collect()
}

/// The part of the language contract completion reads.
#[derive(Debug, Default, Deserialize)]
struct Language {
    #[serde(default)]
    operators: Operators,
    #[serde(default)]
    unit_dimensions: Vec<Dimension>,
}

#[derive(Debug, Default, Deserialize)]
struct Operators {
    #[serde(default)]
    expression: Vec<OperatorEntry>,
}

#[derive(Debug, Deserialize)]
struct OperatorEntry {
    symbol: String,
    arity: String,
    doc: String,
}

#[derive(Debug, Deserialize)]
struct Dimension {
    name: String,
    units: Vec<UnitEntry>,
}

#[derive(Debug, Deserialize)]
struct UnitEntry {
    suffix: String,
    doc: String,
}

/// The language contract, read once, when completion first asks. A contract that did not parse
/// leaves completion without docs and units rather than taking the shell down with it; the tests
/// below fail first, because they read every doc and every unit through it.
fn language() -> &'static Language {
    static LANGUAGE: OnceLock<Language> = OnceLock::new();
    LANGUAGE.get_or_init(|| serde_yaml_ng::from_str(LANGUAGE_FILE).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use ono_parser::BinaryOp;
    use ono_value::FieldType;

    use super::{EVERY, language, operator_doc, operator_symbol, units_for};

    #[test]
    fn should_document_every_comparison_it_offers() {
        for op in EVERY {
            assert!(
                operator_doc(*op).is_some(),
                "`{}` has no doc in docs/contracts/language.yaml",
                operator_symbol(*op)
            );
        }
        assert!(operator_doc(BinaryOp::And).is_some());
    }

    #[test]
    fn should_offer_only_units_the_lexer_reads_as_that_dimension() {
        // language.yaml restates the lexer's suffix table; a suffix it lists that the lexer does
        // not read would be completed into a line that does not parse.
        assert!(!language().unit_dimensions.is_empty());
        for ty in [FieldType::ByteSize, FieldType::Duration] {
            let units = units_for(&ty);
            assert!(!units.is_empty());
            for (suffix, _) in units {
                let line = format!("where x > 5{suffix}");
                let unit = ono_parser::tokens(&line)
                    .into_iter()
                    .find(|token| token.kind == ono_parser::TokenKind::Unit);
                assert_eq!(
                    unit.map(|token| token.text(&line).to_owned()),
                    Some(format!("5{suffix}")),
                    "`5{suffix}` lexes as one unit literal"
                );
                let parsed = ono_parser::parse(&line);
                assert!(!parsed.has_errors(), "`{line}` parses");
            }
        }
    }
}
