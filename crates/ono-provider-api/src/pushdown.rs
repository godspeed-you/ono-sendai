//! Which comparisons a provider can evaluate in the system it asks, and the comparisons a query
//! hands it (ADR-0980).
//!
//! A `where` over a provider-backed target is decided by the pipeline, record by record, after
//! the provider produced each one. Where the first match lies far into a large or remote source,
//! everything before it is read, decoded and thrown away. A provider that can ask its source the
//! same question says so here: a [`PushdownCapability`] names one field of one target, the type
//! of literal it is compared with and the operators the source evaluates with exactly Ono's
//! meaning. A [`Comparison`] is one such question, already resolved to a value.
//!
//! The handover is an optimisation and nothing else. The pipeline decides the whole predicate
//! again over whatever the provider returns, so a provider may ignore a comparison, or exclude
//! only some of the records it rejects; it may never exclude a record the comparison keeps.

use ono_value::Value;

/// The operators a comparison may use.
///
/// The vocabulary is closed, and it is the comparison operators of the language: a provider
/// declares the subset its source evaluates with the same meaning, and an operator it does not
/// declare is never handed to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ComparisonOperator {
    /// `==`.
    Equal,
    /// `!=`.
    NotEqual,
    /// `<`.
    Less,
    /// `<=`.
    LessOrEqual,
    /// `>`.
    Greater,
    /// `>=`.
    GreaterOrEqual,
}

impl ComparisonOperator {
    /// Every operator, in the order the contracts list them.
    pub const ALL: [Self; 6] = [
        Self::Equal,
        Self::NotEqual,
        Self::Less,
        Self::LessOrEqual,
        Self::Greater,
        Self::GreaterOrEqual,
    ];

    /// The operator as the language and the provider contracts spell it.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Less => "<",
            Self::LessOrEqual => "<=",
            Self::Greater => ">",
            Self::GreaterOrEqual => ">=",
        }
    }

    /// The operator a spelling names, if it names one.
    ///
    /// ```
    /// use ono_provider_api::ComparisonOperator;
    ///
    /// assert_eq!(ComparisonOperator::from_symbol("<="), Some(ComparisonOperator::LessOrEqual));
    /// assert_eq!(ComparisonOperator::from_symbol("~="), None);
    /// ```
    #[must_use]
    pub fn from_symbol(symbol: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|operator| operator.symbol() == symbol)
    }

    /// The operator that asks the same question with the operands exchanged: `3 >= x` is
    /// `x <= 3`.
    #[must_use]
    pub const fn mirrored(self) -> Self {
        match self {
            Self::Equal => Self::Equal,
            Self::NotEqual => Self::NotEqual,
            Self::Less => Self::Greater,
            Self::LessOrEqual => Self::GreaterOrEqual,
            Self::Greater => Self::Less,
            Self::GreaterOrEqual => Self::LessOrEqual,
        }
    }
}

/// The type of literal a declared field is compared with.
///
/// Closed, and deliberately short: these are the scalars whose comparison has one meaning on both
/// sides of a provider boundary. Units, timestamps, enums and anything nullable are compared by
/// the pipeline alone until a provider and an ADR state their native meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LiteralType {
    /// An integer.
    Int,
    /// A string.
    String,
    /// A boolean.
    Bool,
}

impl LiteralType {
    /// Every literal type, in the order the contracts list them.
    pub const ALL: [Self; 3] = [Self::Int, Self::String, Self::Bool];

    /// The type as the schemas and the provider contracts spell it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::String => "string",
            Self::Bool => "bool",
        }
    }

    /// The literal type a spelling names, if it names one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|ty| ty.name() == name)
    }

    /// The literal type of `value`, where it is one a comparison may carry.
    #[must_use]
    pub const fn of(value: &Value) -> Option<Self> {
        match value {
            Value::Int(_) => Some(Self::Int),
            Value::String(_) => Some(Self::String),
            Value::Bool(_) => Some(Self::Bool),
            _ => None,
        }
    }
}

/// One field compared with one literal: `depth <= 3`.
///
/// The field is always on the left. A predicate written the other way round is mirrored before
/// it becomes one of these, so a provider reads one shape.
///
/// It displays as the language writes it, which is how a plan quotes it:
///
/// ```
/// use ono_provider_api::{Comparison, ComparisonOperator};
/// use ono_value::Value;
///
/// let depth = Comparison::new("depth", ComparisonOperator::LessOrEqual, Value::Int(3));
/// let state = Comparison::new("state", ComparisonOperator::Equal, Value::string("running"));
/// assert_eq!(depth.to_string(), "depth <= 3");
/// assert_eq!(state.to_string(), "state == \"running\"");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    field: String,
    operator: ComparisonOperator,
    literal: Value,
}

impl Comparison {
    /// The comparison `field operator literal`.
    #[must_use]
    pub fn new(field: impl Into<String>, operator: ComparisonOperator, literal: Value) -> Self {
        Self {
            field: field.into(),
            operator,
            literal,
        }
    }

    /// The field being compared.
    #[must_use]
    pub fn field(&self) -> &str {
        &self.field
    }

    /// The operator.
    #[must_use]
    pub const fn operator(&self) -> ComparisonOperator {
        self.operator
    }

    /// The literal the field is compared with.
    #[must_use]
    pub const fn literal(&self) -> &Value {
        &self.literal
    }
}

impl std::fmt::Display for Comparison {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (field, operator) = (&self.field, self.operator.symbol());
        match &self.literal {
            Value::Int(number) => write!(f, "{field} {operator} {number}"),
            Value::Bool(state) => write!(f, "{field} {operator} {state}"),
            Value::String(text) => write!(f, "{field} {operator} \"{text}\""),
            other => write!(f, "{field} {operator} {other:?}"),
        }
    }
}

/// A provider's statement that its source evaluates comparisons on one field of one target
/// (ADR-0980).
///
/// The statement is a promise about meaning: for every record the provider could produce for the
/// target, a declared comparison decided by the source keeps and rejects exactly what the same
/// comparison decided by the pipeline would. A field whose value may be unknown, or whose order
/// on the source's side is not the pipeline's, is not declared.
///
/// ```
/// use ono_provider_api::{Comparison, ComparisonOperator, LiteralType, PushdownCapability};
/// use ono_value::Value;
///
/// let declared = PushdownCapability::new(
///     "queue",
///     "depth",
///     LiteralType::Int,
///     [ComparisonOperator::Equal, ComparisonOperator::LessOrEqual],
/// );
/// let ceiling = Comparison::new("depth", ComparisonOperator::LessOrEqual, Value::Int(3));
/// assert!(declared.accepts("queue", &ceiling));
///
/// // Another operator, another field, another literal type, another target: none is declared.
/// let floor = Comparison::new("depth", ComparisonOperator::GreaterOrEqual, Value::Int(3));
/// let text = Comparison::new("depth", ComparisonOperator::LessOrEqual, Value::string("3"));
/// let name = Comparison::new("name", ComparisonOperator::Equal, Value::string("mail"));
/// assert!(!declared.accepts("queue", &floor));
/// assert!(!declared.accepts("queue", &text));
/// assert!(!declared.accepts("queue", &name));
/// assert!(!declared.accepts("process", &ceiling));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushdownCapability {
    target: String,
    field: String,
    literal: LiteralType,
    operators: Vec<ComparisonOperator>,
}

impl PushdownCapability {
    /// The source evaluates `operators` on `field` of `target` against a literal of type
    /// `literal`.
    #[must_use]
    pub fn new(
        target: impl Into<String>,
        field: impl Into<String>,
        literal: LiteralType,
        operators: impl IntoIterator<Item = ComparisonOperator>,
    ) -> Self {
        Self {
            target: target.into(),
            field: field.into(),
            literal,
            operators: operators.into_iter().collect(),
        }
    }

    /// The target the declaration is about.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// The field the source can compare.
    #[must_use]
    pub fn field(&self) -> &str {
        &self.field
    }

    /// The type of literal the field is compared with.
    #[must_use]
    pub const fn literal(&self) -> LiteralType {
        self.literal
    }

    /// The operators the source evaluates, in the order they were declared.
    #[must_use]
    pub fn operators(&self) -> &[ComparisonOperator] {
        &self.operators
    }

    /// Whether `comparison`, asked of `target`, is one this declaration covers: the same target,
    /// the same field, a declared operator and a literal of the declared type.
    #[must_use]
    pub fn accepts(&self, target: &str, comparison: &Comparison) -> bool {
        self.target == target
            && self.field == comparison.field()
            && self.operators.contains(&comparison.operator())
            && LiteralType::of(comparison.literal()) == Some(self.literal)
    }
}
