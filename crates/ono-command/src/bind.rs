//! Argument binding: a parsed stage becomes resolved selectors and options.
//!
//! ADR-0009 has the parser keep a words-mode argument as the exact text that was typed, precisely
//! so that this layer can reinterpret it against the type the command declares. `4419` becomes an
//! `int` because `get process` says its `pid` selector is one; `5s` becomes a `duration` because
//! `watch process` says its `--every` option is one. Everything that can go wrong is a structured
//! error naming the thing that went wrong (spec §43).

use ono_core::ErrorCode;
use ono_parser::{Argument, Expr};
use ono_value::{ErrorValue, Value};

use crate::contract::{ArgumentMode, CommandContract, DeclaredType, ParameterSpec};
use crate::suggest::closest;

/// What one selector or option was bound to.
#[derive(Debug, Clone, PartialEq)]
pub enum Binding {
    /// A word, reinterpreted as a value of the declared type.
    Value(Value),
    /// Unevaluated expressions, which is what an expression-mode argument stays until the
    /// evaluator runs it (ADR-0009). A parameter written once carries one.
    Expressions(Vec<Expr>),
}

impl Binding {
    /// The value, when the argument was a word this layer could reinterpret.
    #[must_use]
    pub fn value(&self) -> Option<&Value> {
        match self {
            Binding::Value(value) => Some(value),
            Binding::Expressions(_) => None,
        }
    }

    /// The single expression, when the argument needs evaluating.
    #[must_use]
    pub fn expression(&self) -> Option<&Expr> {
        match self {
            Binding::Expressions(expressions) => expressions.first(),
            Binding::Value(_) => None,
        }
    }

    /// Every expression bound to the parameter, for a parameter written more than once.
    #[must_use]
    pub fn expressions(&self) -> &[Expr] {
        match self {
            Binding::Expressions(expressions) => expressions,
            Binding::Value(_) => &[],
        }
    }
}

/// The selectors and options of one stage, resolved against the command's declared types.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundArguments {
    spelling: String,
    selectors: Vec<(String, Binding)>,
    options: Vec<(String, Binding)>,
    /// Narrowing conditions a context frame contributed on a field the command declares no
    /// parameter for (spec §14.3, ADR-0076). They reach the provider as selectors and nothing
    /// else — an implementation reading its declared parameters never sees them.
    ambient: Vec<(String, Value)>,
    /// The declared selectors, in declaration order, so a value an expression evaluates to later
    /// is held to the same declaration a word was held to here (issue #168, ADR-0937).
    declared_selectors: Vec<ParameterSpec>,
    /// The declared options, for the same reason.
    declared_options: Vec<ParameterSpec>,
    /// The directory the relative paths among these arguments were anchored on, when they were
    /// (issue #302, ADR-0957). A provider query built from them names it, so a path the command
    /// left out means that directory too.
    directory: Option<std::sync::Arc<std::path::Path>>,
}

impl BoundArguments {
    /// The command spelling these arguments were bound for, as errors name it.
    #[must_use]
    pub fn spelling(&self) -> &str {
        &self.spelling
    }

    /// Every bound selector, in declaration order.
    #[must_use]
    pub fn selectors(&self) -> &[(String, Binding)] {
        &self.selectors
    }

    /// Every bound option, in declaration order.
    #[must_use]
    pub fn options(&self) -> &[(String, Binding)] {
        &self.options
    }

    /// A selector's binding, whether it is a value or an expression.
    #[must_use]
    pub fn selector_binding(&self, name: &str) -> Option<&Binding> {
        find(&self.selectors, name)
    }

    /// An option's binding, whether it is a value or an expression.
    #[must_use]
    pub fn option_binding(&self, name: &str) -> Option<&Binding> {
        find(&self.options, name)
    }

    /// A selector's value, when it was written as a word.
    #[must_use]
    pub fn selector(&self, name: &str) -> Option<&Value> {
        self.selector_binding(name).and_then(Binding::value)
    }

    /// An option's value, when it was written as a word.
    #[must_use]
    pub fn option(&self, name: &str) -> Option<&Value> {
        self.option_binding(name).and_then(Binding::value)
    }

    /// A selector's unevaluated expression, for an expression-mode command.
    #[must_use]
    pub fn selector_expression(&self, name: &str) -> Option<&Expr> {
        self.selector_binding(name).and_then(Binding::expression)
    }

    /// An option's unevaluated expression.
    #[must_use]
    pub fn option_expression(&self, name: &str) -> Option<&Expr> {
        self.option_binding(name).and_then(Binding::expression)
    }

    /// The selectors a context frame contributed on fields no parameter declares.
    #[must_use]
    pub fn ambient(&self) -> &[(String, Value)] {
        &self.ambient
    }

    /// These arguments with every expression binding evaluated against `scope`.
    ///
    /// A words-mode command's argument may still arrive as an expression — `stop process $p`,
    /// `each { stop process @.pid }`, `--since (now() - 1h)` — because ADR-0009 keeps the parsed
    /// form until something knows what it means. A command that reads *values* resolves them
    /// here, once, before it acts (ADR-0085 §2, ADR-0219). An expression-mode command must not
    /// call this: its expressions are evaluated per row, against the row.
    ///
    /// # Errors
    ///
    /// Whatever evaluating an argument raises — an undefined variable, a `@` with no current
    /// value, a failed field access — naming the parameter it was written for.
    pub fn evaluated(&self, scope: &crate::Scope) -> Result<Self, ErrorValue> {
        let evaluate = |expressions: &[Expr]| -> Result<Vec<Value>, ErrorValue> {
            expressions
                .iter()
                .map(|expression| crate::expr::evaluate(expression, &Value::Null, scope))
                .collect()
        };

        let mut selectors: Vec<(String, Binding)> = Vec::with_capacity(self.selectors.len());
        for (name, binding) in &self.selectors {
            let Binding::Expressions(expressions) = binding else {
                selectors.push((name.clone(), binding.clone()));
                continue;
            };
            let values = evaluate(expressions)?;
            let Some(spec) = self.declared_selector(name) else {
                selectors.push((name.clone(), Binding::Value(gathered(values))));
                continue;
            };
            if let Some(value) = conform_all(spec, &values) {
                selectors.push((name.clone(), Binding::Value(value)));
                continue;
            }
            // Selectors are alternatives (spec §6.1, §26.2): a word binds to the first free one
            // whose type it fits, and an evaluated value is held to the same rule, so `get user
            // $who` with `$who = "root"` is `get user root` (ADR-0937).
            let free = self.declared_selectors.iter().find_map(|other| {
                let taken = self
                    .selectors
                    .iter()
                    .any(|(bound, _)| bound == other.name())
                    || selectors.iter().any(|(bound, _)| bound == other.name());
                if taken {
                    return None;
                }
                conform_all(other, &values).map(|value| (other.name().to_owned(), value))
            });
            match free {
                Some((other, value)) => selectors.push((other, Binding::Value(value))),
                None => return Err(self.mismatch("", spec, &values)),
            }
        }

        let mut options = Vec::with_capacity(self.options.len());
        for (name, binding) in &self.options {
            let Binding::Expressions(expressions) = binding else {
                options.push((name.clone(), binding.clone()));
                continue;
            };
            let values = evaluate(expressions)?;
            let value = match self.declared_option(name) {
                Some(spec) => {
                    conform_all(spec, &values).ok_or_else(|| self.mismatch("--", spec, &values))?
                }
                None => gathered(values),
            };
            options.push((name.clone(), Binding::Value(value)));
        }

        let evaluated = Self {
            spelling: self.spelling.clone(),
            selectors,
            options,
            ambient: self.ambient.clone(),
            declared_selectors: self.declared_selectors.clone(),
            declared_options: self.declared_options.clone(),
            directory: self.directory.clone(),
        };
        // An evaluated path is held to the same directory a written one is: `remove file $name`
        // in a job means the job's `$name`, wherever the foreground has gone (ADR-0957).
        Ok(match scope.working_directory() {
            Some(directory) => evaluated.anchored(directory),
            None => evaluated,
        })
    }

    /// These arguments with every relative path anchored on `directory`.
    ///
    /// A relative path means the working directory, and the process has one working directory,
    /// which the foreground moves. A command that must keep the meaning its path had when it was
    /// bound — a background job's (issue #302) — has each value bound to a `path` parameter, or
    /// to a list of them, joined onto the directory that path means. The join is lexical: `.`
    /// components fall away and `..` stays, nothing is canonicalized, and a path that does not
    /// exist yet — a destination about to be written — is anchored like one that does. Absolute
    /// paths and values of other types are untouched, and an expression still unevaluated is
    /// anchored when it is evaluated (ADR-0957).
    #[must_use]
    pub fn anchored(&self, directory: &std::sync::Arc<std::path::Path>) -> Self {
        let anchor = |declared: &[ParameterSpec], bindings: &[(String, Binding)]| {
            bindings
                .iter()
                .map(|(name, binding)| {
                    let is_path = declared
                        .iter()
                        .find(|spec| spec.name() == name)
                        .is_some_and(|spec| names_paths(spec.declared_type()));
                    match binding {
                        Binding::Value(value) if is_path => {
                            (name.clone(), Binding::Value(anchor_value(directory, value)))
                        }
                        other => (name.clone(), other.clone()),
                    }
                })
                .collect::<Vec<_>>()
        };
        Self {
            spelling: self.spelling.clone(),
            selectors: anchor(&self.declared_selectors, &self.selectors),
            options: anchor(&self.declared_options, &self.options),
            ambient: self
                .ambient
                .iter()
                .map(|(field, value)| (field.clone(), anchor_value(directory, value)))
                .collect(),
            declared_selectors: self.declared_selectors.clone(),
            declared_options: self.declared_options.clone(),
            directory: Some(std::sync::Arc::clone(directory)),
        }
    }

    /// The directory these arguments' relative paths were anchored on, if they were.
    #[must_use]
    pub fn working_directory(&self) -> Option<&std::sync::Arc<std::path::Path>> {
        self.directory.as_ref()
    }

    fn declared_selector(&self, name: &str) -> Option<&ParameterSpec> {
        self.declared_selectors
            .iter()
            .find(|spec| spec.name() == name)
    }

    fn declared_option(&self, name: &str) -> Option<&ParameterSpec> {
        self.declared_options
            .iter()
            .find(|spec| spec.name() == name)
    }

    /// `type.mismatch` for an evaluated value its parameter's declaration does not admit, naming
    /// the parameter, the declared type and the type received (issue #168).
    fn mismatch(&self, prefix: &str, spec: &ParameterSpec, values: &[Value]) -> ErrorValue {
        let received = values
            .iter()
            .find(|value| conform(spec, value).is_none())
            .map_or("value", Value::type_name);
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!(
                "`{prefix}{}` of `{}` is declared `{}`, and its value is a `{received}`",
                spec.name(),
                self.spelling,
                spec.declared_type().name(),
            ),
        )
        .with_help(format!(
            "give `{prefix}{}` a `{}`; `help {}` lists what each parameter takes",
            spec.name(),
            spec.declared_type().name(),
            self.spelling
        ))
        .with_metadata(
            "parameter",
            Value::string(&format!("{prefix}{}", spec.name())),
        )
        .with_metadata("declared", Value::string(&spec.declared_type().name()))
        .with_metadata("received", Value::string(received))
    }

    /// These arguments with `name` bound to `value` as a selector, unless it is bound already.
    ///
    /// What was typed wins over what a context fills in: a frame supplies the arguments the user
    /// did not write, never overrides the ones they did (spec §14.5, ADR-0076).
    #[must_use]
    pub fn with_selector(mut self, name: &str, value: Value) -> Self {
        if find(&self.selectors, name).is_none() {
            self.selectors
                .push((name.to_owned(), Binding::Value(value)));
        }
        self
    }

    /// These arguments with `name` bound to `value` as an option, unless it is bound already.
    #[must_use]
    pub fn with_option(mut self, name: &str, value: Value) -> Self {
        if find(&self.options, name).is_none() {
            self.options.push((name.to_owned(), Binding::Value(value)));
        }
        self
    }

    /// These arguments with an ambient narrowing on `field`.
    #[must_use]
    pub fn with_ambient(mut self, field: &str, value: Value) -> Self {
        self.ambient.push((field.to_owned(), value));
        self
    }

    /// Whether a boolean option was given and is true.
    #[must_use]
    pub fn flag(&self, name: &str) -> bool {
        matches!(self.option(name), Some(Value::Bool(true)))
    }

    /// A selector the command cannot run without.
    ///
    /// Requiredness is a property of the implementation, not of the contract: `inspect process`
    /// accepts a pid *or* a process through the pipeline, so the registry declares neither as
    /// mandatory and the implementation asks for what it actually needs.
    ///
    /// # Errors
    ///
    /// `type.mismatch` naming the selector, the command and where else the value could come from.
    pub fn require_selector(&self, name: &str) -> Result<&Value, ErrorValue> {
        self.selector(name).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{}` needs a `{name}`, and none was given", self.spelling),
            )
            .with_help(format!(
                "give it as `{} <{name}>`, or pipe the object in",
                self.spelling
            ))
        })
    }

    /// An option the command cannot run without.
    ///
    /// # Errors
    ///
    /// `type.mismatch` naming the option and the command.
    pub fn require_option(&self, name: &str) -> Result<&Value, ErrorValue> {
        self.option(name).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{}` needs `--{name}`, and none was given", self.spelling),
            )
        })
    }
}

fn find<'a>(entries: &'a [(String, Binding)], name: &str) -> Option<&'a Binding> {
    entries
        .iter()
        .find(|(bound, _)| bound == name)
        .map(|(_, binding)| binding)
}

impl CommandContract {
    /// Binds a stage's arguments to this command's declared selectors and options.
    ///
    /// The arguments are what remains after the head and, where the command takes one, the target
    /// word — which is what [`CommandRegistry::resolve`](crate::CommandRegistry::resolve) hands
    /// back.
    ///
    /// # Errors
    ///
    /// - `type.unknown_field` for an option the command does not declare, with the nearest
    ///   declared option in the help;
    /// - `type.mismatch` for a value that is not of the declared type, for an option left without
    ///   its value, and for a positional argument beyond the declared selectors;
    /// - `type.invalid_unit` for a unit of the wrong dimension, such as `--every 5MiB`.
    ///
    /// ```
    /// use ono_command::CommandRegistry;
    /// use ono_value::Value;
    ///
    /// let registry = CommandRegistry::embedded()?;
    /// let parsed = ono_parser::parse("get process 4419");
    /// let stage = &parsed.program().statements[0]
    ///     .as_pipeline()
    ///     .expect("a pipeline")
    ///     .head
    ///     .stages[0];
    /// let resolved = registry.resolve("get", &stage.arguments)?;
    /// let bound = resolved.contract.bind(resolved.arguments)?;
    /// assert_eq!(bound.selector("pid"), Some(&Value::Int(4419)));
    /// # Ok::<(), ono_value::ErrorValue>(())
    /// ```
    pub fn bind(&self, arguments: &[Argument]) -> Result<BoundArguments, ErrorValue> {
        let mut selectors: Vec<(String, Vec<Binding>)> = Vec::new();
        let mut options: Vec<(String, Vec<Binding>)> = Vec::new();
        let mut used = vec![false; self.selectors().len()];
        let mut pending: Option<&ParameterSpec> = None;
        let mut pending_flag: Option<&ParameterSpec> = None;
        // An option spec v0.4 §6.1 writes as `--changes [duration]`: it takes the next word when
        // that word is one, and means "the configured default" when it is not there (ADR-0144).
        let mut pending_optional: Option<&ParameterSpec> = None;

        for argument in arguments {
            // Spec §41 writes assignment as `set config key = value`: the bare `=` is the
            // separator of that spelling, never a value, and the words either side of it bind
            // exactly as if it were absent.
            if pending.is_none()
                && pending_optional.is_none()
                && matches!(argument, Argument::Word(word) if word.text == "=")
            {
                continue;
            }
            if let Argument::Option(written) = argument {
                if let Some(spec) = pending.take() {
                    return Err(self.missing_option_value(spec));
                }
                if let Some(spec) = pending_optional.take() {
                    push(&mut options, spec.name(), Binding::Value(Value::Bool(true)));
                }
                // A flag waits only for a word that could be `true` or `false` (ADR-0009). The
                // next option is not one, so the flag it was waiting on is already decided:
                // `get dir --all --recursive` sets both.
                if let Some(spec) = pending_flag.take() {
                    push(&mut options, spec.name(), Binding::Value(Value::Bool(true)));
                }
                let spec = self
                    .option(&written.name)
                    .ok_or_else(|| self.unknown_option(&written.name))?;
                match &written.value {
                    Some(expression) => {
                        let binding = self.bind_expression(spec, expression)?;
                        push(&mut options, spec.name(), binding);
                    }
                    None if spec.has_optional_value() => pending_optional = Some(spec),
                    None if spec.declared_type().is_flag() => {
                        // `true` and `false` are literals of the language, so a flag followed
                        // by one takes it as its value — spec §31.3 writes `--enabled false`.
                        // A literal rule, not a shape heuristic (ADR-0009): any other word
                        // leaves the flag meaning `true` exactly as before.
                        pending_flag = Some(spec);
                    }
                    None => pending = Some(spec),
                }
                continue;
            }

            if let Some(spec) = pending.take() {
                let binding = self.bind_argument(spec, argument)?;
                push(&mut options, spec.name(), binding);
                continue;
            }

            if let Some(spec) = pending_optional.take() {
                // The word is the option's value only if it *is* one. `near --changed 10s` reads
                // the window; `near --changed socket` reads the relation and leaves the window
                // to the configuration, which is what makes the value genuinely optional rather
                // than merely last (spec v0.4 §6.2).
                if let Ok(binding) = self.bind_argument(spec, argument) {
                    push(&mut options, spec.name(), binding);
                    continue;
                }
                push(&mut options, spec.name(), Binding::Value(Value::Bool(true)));
            }

            if let Some(spec) = pending_flag.take() {
                if let Argument::Word(word) = argument
                    && let Ok(explicit) = word.text.parse::<bool>()
                {
                    push(
                        &mut options,
                        spec.name(),
                        Binding::Value(Value::Bool(explicit)),
                    );
                    continue;
                }
                push(&mut options, spec.name(), Binding::Value(Value::Bool(true)));
            }

            let (index, binding) = self.positional_binding(&used, argument)?;
            used[index] = true;
            push(&mut selectors, self.selectors()[index].name(), binding);
        }

        if let Some(spec) = pending {
            return Err(self.missing_option_value(spec));
        }
        if let Some(spec) = pending_optional {
            push(&mut options, spec.name(), Binding::Value(Value::Bool(true)));
        }
        if let Some(spec) = pending_flag {
            push(&mut options, spec.name(), Binding::Value(Value::Bool(true)));
        }

        Ok(BoundArguments {
            spelling: self.spelling(),
            selectors: merge(selectors, self.selectors()),
            options: merge(options, self.options()),
            ambient: Vec::new(),
            declared_selectors: self.selectors().to_vec(),
            declared_options: self.options().to_vec(),
            directory: None,
        })
    }

    /// The selector a positional argument belongs to, and what it binds to.
    ///
    /// A command's selectors are alternatives rather than positions: `get process` declares
    /// `pid: int` and `name: string` because spec §6.1 writes `get process 4419` and spec §26.2
    /// writes `get service nginx`. So a word binds to the first selector still free whose declared
    /// type it satisfies, which is what makes both spellings work without either being quoted.
    fn positional_binding(
        &self,
        used: &[bool],
        argument: &Argument,
    ) -> Result<(usize, Binding), ErrorValue> {
        let free = (0..self.selectors().len()).filter(|index| !used[*index]);
        let reusable = (0..self.selectors().len())
            .filter(|index| used[*index] && self.selectors()[*index].is_repeatable());
        let candidates: Vec<usize> = free.chain(reusable).collect();

        if candidates.is_empty() {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!(
                    "`{}` takes {} selector(s), and `{}` is one too many",
                    self.spelling(),
                    self.selectors().len(),
                    describe(argument),
                ),
            )
            .with_help(format!("`help {}` shows what it accepts", self.spelling())));
        }

        let mut attempted = Vec::new();
        let mut first_error = None;
        for index in candidates {
            let spec = &self.selectors()[index];
            attempted.push(format!(
                "`{}` ({})",
                spec.name(),
                spec.declared_type().name()
            ));
            match self.bind_argument(spec, argument) {
                Ok(binding) => return Ok((index, binding)),
                Err(error) => first_error.get_or_insert(error),
            };
        }

        // With one candidate the underlying error is already precise, and keeping it keeps its
        // code — a wrong unit stays `type.invalid_unit` rather than becoming a generic mismatch.
        let error = first_error.unwrap_or_else(|| {
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{}` binds nothing", self.spelling()),
            )
        });
        if attempted.len() == 1 {
            return Err(error);
        }
        Err(ErrorValue::new(
            error.code(),
            format!(
                "`{}`: `{}` fits none of its selectors — {}",
                self.spelling(),
                describe(argument),
                attempted.join(", ")
            ),
        )
        .with_help(format!("`help {}` shows what it accepts", self.spelling())))
    }

    fn bind_argument(
        &self,
        spec: &ParameterSpec,
        argument: &Argument,
    ) -> Result<Binding, ErrorValue> {
        match argument {
            Argument::Word(word) => self.coerce(spec, &word.text),
            Argument::Value(expression) => self.bind_expression(spec, expression),
            Argument::Option(option) => Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!(
                    "`--{}` of `{}` is an option, not a value for `{}`",
                    option.name,
                    self.spelling(),
                    spec.name()
                ),
            )),
            Argument::Error(_) => Err(ErrorValue::new(
                ErrorCode::ParseSyntax,
                format!("an argument of `{}` could not be read", self.spelling()),
            )),
        }
    }

    /// A quoted string is text the user meant literally, so it is reinterpreted like a word.
    /// Every other expression stays unevaluated: only the evaluator can turn `@1` or `cpu > 20`
    /// into a value, and doing it here would need a runtime this layer must not have.
    fn bind_expression(
        &self,
        spec: &ParameterSpec,
        expression: &Expr,
    ) -> Result<Binding, ErrorValue> {
        if self.argument_mode() == ArgumentMode::Words
            && let Expr::Str(literal) = expression
            && let Some(text) = literal.literal_text()
        {
            return self.coerce(spec, text);
        }
        Ok(Binding::Expressions(vec![expression.clone()]))
    }

    fn coerce(&self, spec: &ParameterSpec, text: &str) -> Result<Binding, ErrorValue> {
        spec.declared_type()
            .coerce(text)
            .map(Binding::Value)
            .map_err(|error| {
                ErrorValue::new(
                    error.code(),
                    format!(
                        "`{}` of `{}`: {}",
                        spec.name(),
                        self.spelling(),
                        error.message()
                    ),
                )
                .with_help(format!(
                    "`{}` is declared as `{}`",
                    spec.name(),
                    spec.declared_type().name()
                ))
            })
    }

    fn missing_option_value(&self, spec: &ParameterSpec) -> ErrorValue {
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!(
                "`--{}` of `{}` needs a `{}` value, and none followed it",
                spec.name(),
                self.spelling(),
                spec.declared_type().name()
            ),
        )
    }

    fn unknown_option(&self, name: &str) -> ErrorValue {
        let declared: Vec<&str> = self.options().iter().map(ParameterSpec::name).collect();
        let error = ErrorValue::new(
            ErrorCode::TypeUnknownField,
            format!("`{}` has no option `--{name}`", self.spelling()),
        );
        // A word the command *does* take, written the other way round. Listing the options it
        // has is true and unhelpful when the answer is that this one is positional: `near
        // --relation process` and `near process` differ by two characters and by whether the
        // user is told anything at all (ADR-0271).
        if self
            .selectors()
            .iter()
            .any(|selector| selector.name() == name)
        {
            return error.with_help(format!(
                "`{name}` is a positional selector: write `{} <{name}>`",
                self.spelling()
            ));
        }
        match closest(name, declared.iter().copied()) {
            Some(near) => error.with_help(format!("did you mean `--{near}`?")),
            None if declared.is_empty() => {
                error.with_help(format!("`{}` takes no options", self.spelling()))
            }
            None => error.with_help(format!(
                "`{}` accepts {}",
                self.spelling(),
                declared
                    .iter()
                    .map(|name| format!("--{name}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    /// A provider query built from the bound selectors and options.
    ///
    /// This is how a command reaches a provider (spec §27.1): the selectors become narrowing
    /// conditions the provider may push down, and the options become provider options. An
    /// expression-mode binding is not included — it has no value until the evaluator runs it.
    ///
    /// # Errors
    ///
    /// `resolve.target_not_found` when the command has no target and therefore no provider to
    /// ask, such as a transform.
    pub fn query(&self, arguments: &BoundArguments) -> Result<ono_provider_api::Query, ErrorValue> {
        let target = self.target().ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("`{}` has no target and asks no provider", self.spelling()),
            )
        })?;
        let mut query = ono_provider_api::Query::target(target).for_verb(self.verb());
        for (name, binding) in arguments.selectors() {
            if let Some(value) = binding.value() {
                query = query.with(ono_provider_api::Selector::field(name, value.clone()));
            }
        }
        for (name, binding) in arguments.options() {
            if let Some(value) = binding.value() {
                query = query.option(name, value.clone());
            }
        }
        for (field, value) in arguments.ambient() {
            query = query.with(ono_provider_api::Selector::field(field, value.clone()));
        }
        // Arguments anchored on a directory ask the provider in it, so a path the command left
        // out — `get dir`, `find file` — means that directory as well (ADR-0957).
        if let Some(directory) = arguments.working_directory() {
            query = query.within(std::sync::Arc::clone(directory));
        }
        Ok(query)
    }
}

/// Whether a parameter of `declared` type carries filesystem paths.
fn names_paths(declared: &DeclaredType) -> bool {
    match declared {
        DeclaredType::Path => true,
        DeclaredType::List(inner) => names_paths(inner),
        _ => false,
    }
}

/// `value` with every relative path in it joined onto `directory`, lexically (ADR-0957).
fn anchor_value(directory: &std::path::Path, value: &Value) -> Value {
    match value {
        Value::Path(path) if path.is_relative() => Value::Path(std::sync::Arc::from(
            directory
                .join(path)
                .components()
                .collect::<std::path::PathBuf>()
                .as_path(),
        )),
        Value::List(items) => Value::list(items.iter().map(|item| anchor_value(directory, item))),
        other => other.clone(),
    }
}

/// Several evaluated values of one parameter as the one value a parameter carries: the value
/// itself when it was written once, a list of them when it was written more than once.
fn gathered(mut values: Vec<Value>) -> Value {
    match values.len() {
        1 => values.remove(0),
        _ => Value::list(values),
    }
}

/// The evaluated values of one parameter held to its declaration, gathered as [`gathered`] does;
/// `None` when one of them is not admitted.
fn conform_all(spec: &ParameterSpec, values: &[Value]) -> Option<Value> {
    values
        .iter()
        .map(|value| conform(spec, value))
        .collect::<Option<Vec<Value>>>()
        .map(gathered)
}

/// One evaluated value held to a parameter's declaration (ADR-0937).
///
/// A value already of the declared type stands. A list stands where the parameter is a list or is
/// repeatable, when every item does. Any other scalar is reinterpreted through its canonical text
/// exactly as a word would be — `"4419"` for an `int`, `443` for a `port`, `"5s"` for a
/// `duration` — so an expression is never held to a stricter rule than the word it replaces. A
/// `null` is unknown rather than mistyped (ADR-0014) and stands. Everything else is `None`.
fn conform(spec: &ParameterSpec, value: &Value) -> Option<Value> {
    match (spec.declared_type(), value) {
        (DeclaredType::List(inner), Value::List(items)) => conform_items(inner, items),
        (declared, Value::List(items)) if spec.is_repeatable() => conform_items(declared, items),
        (DeclaredType::List(inner), value) => conform_type(inner, value),
        (declared, value) => conform_type(declared, value),
    }
}

fn conform_items(declared: &DeclaredType, items: &[Value]) -> Option<Value> {
    items
        .iter()
        .map(|item| conform_type(declared, item))
        .collect::<Option<Vec<Value>>>()
        .map(Value::list)
}

fn conform_type(declared: &DeclaredType, value: &Value) -> Option<Value> {
    let fits = match (declared, value) {
        (_, Value::Null) | (DeclaredType::Value, _) => true,
        (DeclaredType::Bool, Value::Bool(_))
        | (DeclaredType::Int, Value::Int(_))
        | (DeclaredType::Float, Value::Float(_))
        | (DeclaredType::String, Value::String(_))
        | (DeclaredType::Path, Value::Path(_))
        | (DeclaredType::Duration, Value::Duration(_))
        | (DeclaredType::ByteSize, Value::ByteSize(_))
        | (DeclaredType::Timestamp, Value::Timestamp(_))
        | (DeclaredType::Port, Value::Port(_))
        | (DeclaredType::Ip, Value::Ip(_))
        | (DeclaredType::IpNetwork, Value::IpNetwork(_))
        | (DeclaredType::Record, Value::Record(_) | Value::Map(_)) => true,
        // A reference names an object by its identity text or is the object itself; resolving it
        // is the command's (ADR-0012 §7).
        (DeclaredType::Ref(_), value) => !matches!(value, Value::List(_)),
        _ => false,
    };
    if fits {
        return Some(value.clone());
    }
    if matches!(
        value,
        Value::List(_) | Value::Map(_) | Value::Record(_) | Value::Bytes(_) | Value::Error(_)
    ) {
        return None;
    }
    let text = ono_value::canonical_text(value).ok()?;
    declared.coerce(&text).ok()
}

fn push(entries: &mut Vec<(String, Vec<Binding>)>, name: &str, binding: Binding) {
    if let Some((_, bindings)) = entries.iter_mut().find(|(bound, _)| bound == name) {
        bindings.push(binding);
    } else {
        entries.push((name.to_owned(), vec![binding]));
    }
}

/// Collapses repeated writings of one parameter, then fills in the declared defaults.
fn merge(
    entries: Vec<(String, Vec<Binding>)>,
    declared: &[ParameterSpec],
) -> Vec<(String, Binding)> {
    let mut bound: Vec<(String, Binding)> = entries
        .into_iter()
        .map(|(name, bindings)| (name, collapse(bindings)))
        .collect();
    for spec in declared {
        if let Some(default) = spec.default_value()
            && !bound.iter().any(|(name, _)| name == spec.name())
        {
            bound.push((spec.name().to_owned(), Binding::Value(default.clone())));
        }
    }
    bound
}

fn collapse(mut bindings: Vec<Binding>) -> Binding {
    if bindings.len() == 1 {
        // `swap_remove` cannot fail on a vector of length one, and avoids cloning the binding.
        return bindings.swap_remove(0);
    }
    let mut values = Vec::new();
    let mut expressions = Vec::new();
    for binding in bindings {
        match binding {
            Binding::Value(Value::List(items)) => values.extend(items.iter().cloned()),
            Binding::Value(value) => values.push(value),
            Binding::Expressions(mut written) => expressions.append(&mut written),
        }
    }
    if expressions.is_empty() {
        Binding::Value(Value::list(values))
    } else {
        Binding::Expressions(expressions)
    }
}

fn describe(argument: &Argument) -> String {
    match argument {
        Argument::Word(word) => word.text.clone(),
        Argument::Option(option) => format!("--{}", option.name),
        Argument::Value(Expr::Str(literal)) => {
            literal.literal_text().unwrap_or("a string").to_owned()
        }
        Argument::Value(_) => "an expression".to_owned(),
        Argument::Error(_) => "an unreadable argument".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Binding, collapse};
    use ono_value::Value;

    #[test]
    fn should_concatenate_repeated_list_bindings() {
        let collapsed = collapse(vec![
            Binding::Value(Value::list([Value::string("a")])),
            Binding::Value(Value::string("b")),
        ]);
        assert_eq!(
            collapsed,
            Binding::Value(Value::list([Value::string("a"), Value::string("b")]))
        );
    }

    #[test]
    fn should_keep_a_single_binding_as_it_is() {
        let collapsed = collapse(vec![Binding::Value(Value::Int(1))]);
        assert_eq!(collapsed, Binding::Value(Value::Int(1)));
    }
}
