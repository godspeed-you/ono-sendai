//! The structured error record of spec §16.1, as a value the pipeline can carry (spec §25).

use std::fmt;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use ono_core::{ErrorCode, ErrorKind};

use crate::{MapValue, SchemaId, Value};

/// A reference to the thing an error or an action is about (spec §16.1, §11.5).
///
/// A reference is an identity, not the object itself: keeping the object alive inside every
/// error would make error values as expensive as the data they describe.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueRef {
    /// A filesystem path.
    Path(Arc<Path>),
    /// A plain name, such as a service or interface name.
    Name(Arc<str>),
    /// An object identified by its schema and its identity fields (spec §27.3).
    Object {
        /// The schema the object belongs to.
        schema: SchemaId,
        /// The object's identity fields and their values.
        identity: Arc<MapValue>,
    },
}

impl ValueRef {
    /// A reference to a filesystem path.
    #[must_use]
    pub fn path(path: &Path) -> Self {
        Self::Path(Arc::from(path))
    }

    /// A reference to something named.
    #[must_use]
    pub fn name(name: &str) -> Self {
        Self::Name(name.into())
    }

    /// A reference to an object, by schema and identity.
    #[must_use]
    pub fn object(schema: SchemaId, identity: MapValue) -> Self {
        Self::Object {
            schema,
            identity: Arc::new(identity),
        }
    }

    /// The reference as an ordinary value, so it can travel in a record field.
    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            ValueRef::Path(path) => Value::Path(Arc::clone(path)),
            ValueRef::Name(name) => Value::String(Arc::clone(name)),
            ValueRef::Object { identity, .. } => Value::Map(Arc::clone(identity)),
        }
    }
}

impl fmt::Display for ValueRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueRef::Path(path) => write!(f, "{}", path.display()),
            ValueRef::Name(name) => f.write_str(name),
            ValueRef::Object { schema, identity } => write!(f, "{schema} {identity}"),
        }
    }
}

/// A structured error, exactly the record of spec §16.1.
///
/// Errors are built in a chain rather than assembled field by field, so a call site reads as one
/// statement:
///
/// ```
/// use ono_core::ErrorCode;
/// use ono_value::{ErrorValue, ValueRef};
/// use std::path::Path;
///
/// let error = ErrorValue::new(ErrorCode::IoPermissionDenied, "access denied")
///     .with_target(ValueRef::path(Path::new("/etc/shadow")))
///     .with_help("requires root or read capability");
/// assert_eq!(
///     error.to_string(),
///     "access denied: /etc/shadow\nrequires root or read capability"
/// );
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorValue {
    code: ErrorCode,
    message: Arc<str>,
    target: Option<ValueRef>,
    source: Option<Arc<ErrorValue>>,
    help: Option<Arc<str>>,
    retryable: Option<bool>,
    metadata: MapValue,
}

impl ErrorValue {
    /// Creates an error with a stable code and a human message.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<Arc<str>>) -> Self {
        Self {
            code,
            message: message.into(),
            target: None,
            source: None,
            help: None,
            retryable: None,
            metadata: MapValue::new(),
        }
    }

    /// Names what the error is about.
    #[must_use]
    pub fn with_target(mut self, target: ValueRef) -> Self {
        self.target = Some(target);
        self
    }

    /// Records the error this one was raised from.
    #[must_use]
    pub fn with_source(mut self, source: ErrorValue) -> Self {
        self.source = Some(Arc::new(source));
        self
    }

    /// Adds the second line a user sees: what to do about it.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<Arc<str>>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// States whether repeating the operation could succeed.
    #[must_use]
    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = Some(retryable);
        self
    }

    /// Attaches a machine-readable detail such as an errno or a provider name.
    #[must_use]
    pub fn with_metadata(mut self, key: &str, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// The stable code of spec §43.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    /// The kind the code belongs to (ADR-0006). Scripts branch on this.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.code.kind()
    }

    /// The human message, without target or help.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// What the error is about, if it is about a particular thing.
    #[must_use]
    pub const fn target(&self) -> Option<&ValueRef> {
        self.target.as_ref()
    }

    /// The error this one was raised from.
    #[must_use]
    pub fn cause(&self) -> Option<&ErrorValue> {
        self.source.as_deref()
    }

    /// The suggestion shown under the message.
    #[must_use]
    pub fn help(&self) -> Option<&str> {
        self.help.as_deref()
    }

    /// Whether repeating the operation could succeed, or `None` when nothing is known about it.
    #[must_use]
    pub const fn retryable(&self) -> Option<bool> {
        self.retryable
    }

    /// The machine-readable details.
    #[must_use]
    pub const fn metadata(&self) -> &MapValue {
        &self.metadata
    }

    /// One field of `ono.error/1` by name, as spec §16.1 declares it (ADR-0215).
    ///
    /// An error reached as a value has the fields its own schema declares, so a path can descend
    /// into it: `error.name` is the dotted selector, `error.source.message` walks the chain.
    /// `None` names no field of an error at all, which a path step reports as
    /// [`ono_core::ErrorCode::TypeUnknownField`] or, written `?.`, as null.
    ///
    /// ```
    /// use ono_core::ErrorCode;
    /// use ono_value::{ErrorValue, Value};
    ///
    /// let error = ErrorValue::new(ErrorCode::IoNotFound, "gone");
    /// assert_eq!(error.field("name"), Some(Value::string("io.not_found")));
    /// assert_eq!(error.field("cpy"), None);
    /// ```
    #[must_use]
    pub fn field(&self, name: &str) -> Option<Value> {
        Some(match name {
            "code" => Value::string(self.code.code()),
            "name" => Value::string(self.code.name()),
            "kind" => Value::string(self.kind().as_str()),
            "message" => Value::string(&self.message),
            "target" => self.target.as_ref().map_or(Value::Null, ValueRef::to_value),
            "source" => self
                .source
                .as_ref()
                .map_or(Value::Null, |source| Value::Error(Arc::clone(source))),
            "help" => self.help.as_deref().map_or(Value::Null, Value::string),
            "retryable" => self.retryable.map_or(Value::Null, Value::Bool),
            // A span belongs to source text, and an error value carries none of its own.
            "span" => Value::Null,
            "metadata" => Value::Map(Arc::new(self.metadata.clone())),
            _ => return None,
        })
    }

    /// This error and every error it was raised from, outermost first.
    pub fn chain(&self) -> impl Iterator<Item = &ErrorValue> {
        let mut next = Some(self);
        std::iter::from_fn(move || {
            let current = next?;
            next = current.cause();
            Some(current)
        })
    }

    /// The terse form of spec §16.2: the message, the target it is about, and the help line.
    #[must_use]
    pub fn render_terse(&self) -> String {
        let mut out = String::from(&*self.message);
        if let Some(target) = &self.target {
            let _ = write!(out, ": {target}");
        }
        if let Some(help) = &self.help {
            let _ = write!(out, "\n{help}");
        }
        out
    }

    /// The rich form `inspect @error` shows: code, kind, details and the whole causal chain.
    #[must_use]
    pub fn render_full(&self) -> String {
        let mut out = String::new();
        self.write_full(&mut out, 0);
        out
    }

    /// The error as an ordinary value, so it can travel through a pipeline or sit in a field.
    #[must_use]
    pub fn into_value(self) -> Value {
        Value::Error(Arc::new(self))
    }

    fn write_full(&self, out: &mut String, indent: usize) {
        let pad = " ".repeat(indent);
        let _ = writeln!(
            out,
            "{pad}{} {} ({})",
            self.code.code(),
            self.code.name(),
            self.kind()
        );
        let _ = writeln!(out, "{pad}{}", self.message);
        if let Some(target) = &self.target {
            let _ = writeln!(out, "{pad}target     {target}");
        }
        if let Some(help) = &self.help {
            let _ = writeln!(out, "{pad}help       {help}");
        }
        if let Some(retryable) = self.retryable {
            let _ = writeln!(out, "{pad}retryable  {retryable}");
        }
        for (key, value) in self.metadata.iter() {
            let _ = writeln!(out, "{pad}metadata   {key} = {value}");
        }
        if let Some(cause) = self.cause() {
            let _ = writeln!(out, "{pad}caused by:");
            cause.write_full(out, indent + 2);
        }
    }
}

impl fmt::Display for ErrorValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render_terse())
    }
}

impl std::error::Error for ErrorValue {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|error| error as &(dyn std::error::Error + 'static))
    }
}

impl From<ErrorValue> for Value {
    fn from(error: ErrorValue) -> Self {
        error.into_value()
    }
}
