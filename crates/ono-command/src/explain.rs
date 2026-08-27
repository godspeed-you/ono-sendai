//! `explain`: the execution plan of spec §42, produced without executing anything (spec §15.3).
//!
//! Every fact in a plan comes from the registry, the capability vocabulary or the provider
//! registry's *declarations*. No provider is queried, no action is attempted and no stream is
//! opened, which is exactly what makes `explain` safe to type in front of a destructive pipeline.

use std::fmt::Write as _;
use std::sync::Arc;

use ono_adapter::{Consumer, OutputDemand, Stdout};
use ono_parser::{Argument, Expr, Pipeline, RedirectOp, RedirectTarget, Stage, StageList};
use ono_provider_api::{ProviderRegistry, Risk};
use ono_value::{MapValue, Value};

use crate::contract::{IoType, Privilege};
use crate::registry::CommandRegistry;

/// The width the label column is padded to, matching spec §42.1's layout.
const LABEL: usize = 12;

/// What a stage's head resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A native command of the registry — step 4 of ADR-0011's resolution order.
    Native {
        /// The stable command id.
        id: String,
    },
    /// No native command answers to the head, so the evaluator goes on to look for a function, an
    /// alias or an executable on `PATH` (ADR-0011 steps 2, 3 and 5).
    External {
        /// The head word as it was typed.
        head: String,
    },
    /// The stage's head is a value rather than a command: a variable, or a parenthesised
    /// pipeline.
    Value,
}

/// One stage of an execution plan.
#[derive(Debug, Clone, PartialEq)]
pub struct StagePlan {
    ordinal: usize,
    source: String,
    resolution: Resolution,
    provider: Option<String>,
    capability: Option<String>,
    input: String,
    output: String,
    element_schema: Option<String>,
    streaming: bool,
    privilege: Option<Privilege>,
    risk: Option<Risk>,
    fields: Vec<String>,
    notes: Vec<String>,
    demand: Option<(OutputDemand, String)>,
    /// The input type the contract declares, before the upstream type was threaded in.
    declared_input: Option<String>,
    /// Whether the stage is `raw <program>`, which bypasses adaptation (spec v0.3 §1.17).
    raw: bool,
}

impl StagePlan {
    /// The stage's position in the pipeline, counting from one as spec §42.1 does.
    #[must_use]
    pub fn ordinal(&self) -> usize {
        self.ordinal
    }

    /// The stage exactly as it was typed.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// What the head resolved to.
    #[must_use]
    pub fn resolution(&self) -> &Resolution {
        &self.resolution
    }

    /// The command id, when the head resolved to a native command.
    #[must_use]
    pub fn command(&self) -> Option<&str> {
        match &self.resolution {
            Resolution::Native { id } => Some(id),
            _ => None,
        }
    }

    /// The provider that would answer, when one is registered for the target.
    #[must_use]
    pub fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }

    /// The provider capability the stage needs.
    #[must_use]
    pub fn capability(&self) -> Option<&str> {
        self.capability.as_deref()
    }

    /// What flows into the stage — the previous stage's output, where there is one.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }

    /// What flows out of it.
    #[must_use]
    pub fn output(&self) -> &str {
        &self.output
    }

    /// The schema of the values leaving the stage, where the plan could keep track of one.
    #[must_use]
    pub fn element_schema(&self) -> Option<&str> {
        self.element_schema.as_deref()
    }

    /// Whether the stage produces its output incrementally.
    #[must_use]
    pub fn is_streaming(&self) -> bool {
        self.streaming
    }

    /// What the user needs before the stage runs.
    #[must_use]
    pub fn privilege(&self) -> Option<Privilege> {
        self.privilege
    }

    /// How much the stage could change or reveal.
    #[must_use]
    pub fn risk(&self) -> Option<Risk> {
        self.risk
    }

    /// The fields the stage's expression arguments read, as spec §42.1's `field` line shows.
    #[must_use]
    pub fn fields(&self) -> &[String] {
        &self.fields
    }

    /// Anything else worth saying about the stage.
    #[must_use]
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Whether the stage bypasses adaptation with the `raw` keyword (spec v0.3 §1.17).
    #[must_use]
    pub fn is_raw(&self) -> bool {
        self.raw
    }

    /// What the stage's stdout is asked to carry, for a stage that is a child process.
    ///
    /// Decided backwards from the consumer (spec v0.3 §1.4): a native command over objects asks
    /// for values, a process or a file keeps bytes, the terminal invites the renderer. A native
    /// stage has no stdout of its own and answers `None`.
    #[must_use]
    pub fn demand(&self) -> Option<&OutputDemand> {
        self.demand.as_ref().map(|(demand, _)| demand)
    }

    fn to_value(&self) -> Value {
        let mut map = MapValue::default();
        map.insert("ordinal".into(), Value::Int(self.ordinal as i128));
        map.insert("source".into(), Value::string(&self.source));
        map.insert(
            "command".into(),
            self.command().map_or(Value::Null, Value::string),
        );
        map.insert(
            "provider".into(),
            self.provider().map_or(Value::Null, Value::string),
        );
        map.insert(
            "capability".into(),
            self.capability().map_or(Value::Null, Value::string),
        );
        map.insert("input".into(), Value::string(&self.input));
        map.insert("output".into(), Value::string(&self.output));
        map.insert("streaming".into(), Value::Bool(self.streaming));
        map.insert(
            "privilege".into(),
            self.privilege
                .map_or(Value::Null, |privilege| Value::string(privilege.as_str())),
        );
        map.insert(
            "risk".into(),
            self.risk
                .map_or(Value::Null, |risk| Value::string(risk.as_str())),
        );
        map.insert(
            "fields".into(),
            Value::list(self.fields.iter().map(|field| Value::string(field))),
        );
        map.insert(
            "demand".into(),
            self.demand()
                .map_or(Value::Null, |demand| Value::string(&demand.to_string())),
        );
        Value::Map(Arc::new(map))
    }

    fn render(&self, into: &mut String) {
        let _ = writeln!(into, "{}. {}", self.ordinal, self.source);
        if let Some(id) = self.command() {
            row(into, "command", id);
        }
        if let Resolution::External { head } = &self.resolution {
            row(
                into,
                "resolution",
                &format!("`{head}` is not a native command"),
            );
        }
        if let Some(provider) = self.provider() {
            row(into, "provider", provider);
        }
        if let Some(capability) = self.capability() {
            row(into, "capability", capability);
        }
        if !self.fields.is_empty() {
            row(into, "field", &self.fields.join(", "));
        }
        row(into, "input", &self.input);
        row(into, "output", &self.output);
        if self.raw {
            row(
                into,
                "adaptation",
                &format!("bypassed (`{}`, spec v0.3 §1.17)", ono_adapter::RAW),
            );
        }
        if let Some((demand, reason)) = &self.demand {
            row(into, "demand", &format!("{demand} ({reason})"));
        }
        row(into, "streaming", if self.streaming { "yes" } else { "no" });
        if let Some(privilege) = self.privilege {
            row(into, "privilege", privilege.as_str());
        }
        if let Some(risk) = self.risk {
            row(into, "risk", risk.as_str());
        }
        for note in &self.notes {
            row(into, "note", note);
        }
    }
}

/// The plan of a whole pipeline: what each stage resolves to, and what it would do.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionPlan {
    source: String,
    stages: Vec<StagePlan>,
}

impl ExecutionPlan {
    /// The pipeline the plan was made for, exactly as it was typed.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The stages, in pipeline order.
    #[must_use]
    pub fn stages(&self) -> &[StagePlan] {
        &self.stages
    }

    /// Whether any stage would change something outside the shell.
    #[must_use]
    pub fn is_mutating(&self) -> bool {
        self.stages
            .iter()
            .any(|stage| stage.risk.is_some_and(Risk::changes_the_world))
    }

    /// The plan as plain text, in the shape of spec §42.1.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = String::from("PIPELINE\n");
        for stage in &self.stages {
            stage.render(&mut text);
            let _ = writeln!(text);
        }
        text
    }

    /// The plan as structured data, so a script can read it without parsing the rendering.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut map = MapValue::default();
        map.insert("source".into(), Value::string(&self.source));
        map.insert("mutating".into(), Value::Bool(self.is_mutating()));
        map.insert(
            "stages".into(),
            Value::list(self.stages.iter().map(StagePlan::to_value)),
        );
        Value::Map(Arc::new(map))
    }
}

fn row(into: &mut String, label: &str, value: &str) {
    let _ = writeln!(into, "   {label:<width$} {value}", width = LABEL);
}

/// The plan for `pipeline`, without executing any part of it.
///
/// `source` is the text the pipeline was parsed from, which is what each stage's `source` line
/// quotes. `providers` names the provider that would answer; without it the plan reports the
/// required capability alone.
///
/// A stage the registry cannot resolve is not an error: `explain` must be able to describe a
/// pipeline that mixes native commands with external programs, and ADR-0011 puts `PATH` after the
/// registry rather than instead of it.
///
/// ```
/// let registry = ono_command::CommandRegistry::embedded()?;
/// let parsed = ono_parser::parse("get process | to json");
/// let pipeline = parsed.program().statements[0].as_pipeline().expect("a pipeline");
/// let plan = ono_command::plan(registry, None, pipeline, "get process | to json");
/// assert_eq!(plan.stages()[0].command(), Some("ono.process.get"));
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[must_use]
pub fn plan(
    registry: &CommandRegistry,
    providers: Option<&ProviderRegistry>,
    pipeline: &Pipeline,
    source: &str,
) -> ExecutionPlan {
    plan_for(registry, providers, pipeline, source, Stdout::Stream)
}

/// The plan for `pipeline` as it would run with the shell's stdout being `stdout`.
///
/// The last stage of a pipeline has no consumer inside it, so what its stdout is asked to carry
/// depends on where the shell's own stdout goes (spec v0.3 §1.4): `plan` assumes a stream, which
/// is what a script and a redirected `ono -c` see; the interactive shell says so here.
#[must_use]
pub fn plan_for(
    registry: &CommandRegistry,
    providers: Option<&ProviderRegistry>,
    pipeline: &Pipeline,
    source: &str,
    stdout: Stdout,
) -> ExecutionPlan {
    let mut stages: Vec<StagePlan> = Vec::new();
    let mut ordinal = 1;

    let lists =
        std::iter::once(&pipeline.head).chain(pipeline.tail.iter().map(|chained| &chained.list));
    for list in lists {
        let mut upstream: Option<IoType> = None;
        let first = stages.len();
        for stage in &list.stages {
            let planned = plan_stage(
                registry,
                providers,
                stage,
                source,
                ordinal,
                upstream.as_ref(),
            );
            upstream = Some(IoType::from_text(&planned.output));
            stages.push(planned);
            ordinal += 1;
        }
        plan_demands(&mut stages[first..], list, stdout);
    }

    ExecutionPlan {
        source: source.to_owned(),
        stages,
    }
}

/// Decides, backwards from each consumer, what every external stage's stdout must carry.
///
/// Spec v0.3 §1.5 wants the demand to be "part of execution planning, not an after-the-fact
/// renderer trick", which is why it is settled here, on the plan, before anything is spawned.
fn plan_demands(stages: &mut [StagePlan], list: &StageList, stdout: Stdout) {
    let count = stages.len();
    for index in 0..count {
        if !matches!(stages[index].resolution, Resolution::External { .. }) {
            continue;
        }
        if stages[index].raw {
            stages[index].demand = Some((
                OutputDemand::RawBytes,
                format!("`{}` bypasses adaptation", ono_adapter::RAW),
            ));
            continue;
        }
        let redirected = list.stages.get(index).and_then(stdout_redirection);
        let (demand, reason) = match redirected {
            Some(Redirected::File(path)) => (
                OutputDemand::for_consumer(Consumer::File { path: &path }),
                format!("stdout goes to {path}"),
            ),
            Some(Redirected::Descriptor(fd)) => (
                OutputDemand::for_consumer(Consumer::Descriptor),
                format!("stdout is duplicated onto descriptor {fd}"),
            ),
            None => match stages.get(index + 1) {
                Some(next) => match &next.resolution {
                    Resolution::Native { .. } => {
                        // What the consumer is declared over decides the demand, not the bytes
                        // the plan threaded into it: `where` is defined over objects even when
                        // the stage before it is a program.
                        let declared = next.declared_input.as_deref().unwrap_or(&next.input);
                        let input = IoType::from_text(declared);
                        let what = if input.admits_bytes() {
                            "bytes"
                        } else if input.admits_text() {
                            "text"
                        } else {
                            "objects"
                        };
                        (
                            OutputDemand::for_consumer(Consumer::Native { input: declared }),
                            format!("`{}` consumes {what}", next.source),
                        )
                    }
                    Resolution::External { .. } | Resolution::Value => (
                        OutputDemand::for_consumer(Consumer::Process),
                        format!("`{}` consumes bytes", next.source),
                    ),
                },
                None => match stdout {
                    Stdout::Terminal => (
                        OutputDemand::for_consumer(Consumer::Terminal),
                        "stdout is the terminal".to_owned(),
                    ),
                    Stdout::Stream => (
                        OutputDemand::for_consumer(Consumer::Stream),
                        "stdout is not a terminal".to_owned(),
                    ),
                },
            },
        };
        stages[index].demand = Some((demand, reason));
    }
}

/// Where a stage's stdout redirection sends it, when it has one.
enum Redirected {
    File(String),
    Descriptor(u32),
}

fn stdout_redirection(stage: &Stage) -> Option<Redirected> {
    stage
        .redirections
        .iter()
        .filter(|redirection| matches!(redirection.fd, None | Some(1)))
        .filter_map(|redirection| match (redirection.op, &redirection.target) {
            (RedirectOp::Write | RedirectOp::Append, RedirectTarget::Word(word)) => {
                Some(Redirected::File(word.text.clone()))
            }
            (RedirectOp::Write | RedirectOp::Append, RedirectTarget::Value(_)) => {
                Some(Redirected::File("a computed path".to_owned()))
            }
            (RedirectOp::DupWrite, RedirectTarget::Fd(fd)) => Some(Redirected::Descriptor(*fd)),
            _ => None,
        })
        .next_back()
}

fn plan_stage(
    registry: &CommandRegistry,
    providers: Option<&ProviderRegistry>,
    stage: &Stage,
    source: &str,
    ordinal: usize,
    upstream: Option<&IoType>,
) -> StagePlan {
    let text = stage.span.of(source).trim().to_owned();
    let carried = upstream.and_then(IoType::element_schema);
    let mut fields = read_fields(&stage.arguments);

    let Some(head) = stage.head.name() else {
        return StagePlan {
            ordinal,
            source: text,
            resolution: Resolution::Value,
            provider: None,
            capability: None,
            input: "null".to_owned(),
            output: upstream.map_or_else(|| "any".to_owned(), |io| io.text().to_owned()),
            element_schema: carried.map(str::to_owned),
            streaming: false,
            privilege: None,
            risk: None,
            fields,
            notes: vec!["the stage's head is a value, not a command".to_owned()],
            demand: None,
            declared_input: None,
            raw: false,
        };
    };

    if is_raw(stage) {
        let program = raw_program(stage).unwrap_or(ono_adapter::RAW);
        return StagePlan {
            ordinal,
            source: text,
            resolution: Resolution::External {
                head: program.to_owned(),
            },
            provider: None,
            capability: None,
            input: upstream.map_or_else(|| "bytes".to_owned(), |io| io.text().to_owned()),
            output: "bytes".to_owned(),
            element_schema: None,
            streaming: true,
            privilege: None,
            risk: None,
            fields,
            notes: vec![
                "the program on PATH, run with no argv rewrite, no decoder and no renderer \
                 (spec v0.3 §1.17, ADR-0054)"
                    .to_owned(),
            ],
            demand: None,
            declared_input: None,
            raw: true,
        };
    }

    let Ok(resolved) = registry.resolve(head, &stage.arguments) else {
        return StagePlan {
            ordinal,
            source: text,
            resolution: Resolution::External {
                head: head.to_owned(),
            },
            provider: None,
            capability: None,
            input: upstream.map_or_else(|| "bytes".to_owned(), |io| io.text().to_owned()),
            output: "bytes".to_owned(),
            element_schema: None,
            streaming: true,
            privilege: None,
            risk: None,
            fields,
            notes: vec![
                "resolved after the registry: a user function, an alias, or an executable on PATH \
                 (ADR-0011)"
                    .to_owned(),
            ],
            demand: None,
            declared_input: None,
            raw: false,
        };
    };

    let contract = resolved.contract;
    // What flows in is what the stage before emitted, whenever the declared input names no
    // concrete element of its own. That is what lets spec §42.1's later stages report the process
    // stream rather than `stream<any>`.
    let input = match upstream {
        Some(previous) if contract.input().is_open() => previous.clone(),
        _ => contract.input().clone(),
    };
    let output = concrete(contract.output(), carried);
    let element_schema = output.element_schema().map(str::to_owned).or_else(|| {
        output
            .is_open()
            .then(|| carried.map(str::to_owned))
            .flatten()
    });

    let mut notes = Vec::new();
    match contract.bind(resolved.arguments) {
        // The fields a stage reads are the ones its first selector names: `sort memory desc`
        // reads `memory`, and `desc` is the direction rather than a field.
        Ok(bound) => {
            if let Some((_, binding)) = bound.selectors().first() {
                let mut named = Vec::new();
                for expression in binding.expressions() {
                    collect_fields(expression, &mut named);
                }
                named.dedup();
                if !named.is_empty() {
                    fields = named;
                }
            }
        }
        Err(error) => notes.push(format!("arguments do not bind: {}", error.message())),
    }

    let capability = contract
        .provider_capability()
        .and_then(|id| registry.capability(id));
    let provider = match (providers, contract.target()) {
        (Some(registered), Some(target)) => registered
            .for_target(target)
            .first()
            .map(|provider| provider.id().to_owned()),
        _ => None,
    };
    if let Some(note) = contract.note() {
        notes.push(note.trim().replace('\n', " "));
    }

    StagePlan {
        ordinal,
        source: text,
        resolution: Resolution::Native {
            id: contract.id().to_owned(),
        },
        provider,
        capability: contract.provider_capability().map(str::to_owned),
        input: input.text().to_owned(),
        output: output.text().to_owned(),
        element_schema,
        streaming: contract.is_streaming(),
        privilege: Some(contract.privilege()),
        risk: capability.map(crate::contract::CapabilitySpec::risk),
        fields,
        notes,
        demand: None,
        declared_input: Some(contract.input().text().to_owned()),
        raw: false,
    }
}

/// Whether `stage` is `raw <program> …`, the bypass of spec v0.3 §1.17.
///
/// The keyword is a bare, unqualified head: `exec:raw` is a program called `raw`.
#[must_use]
pub fn is_raw(stage: &Stage) -> bool {
    matches!(&stage.head, ono_parser::StageHead::Command(name)
        if name.namespace.is_none() && name.name == ono_adapter::RAW)
}

/// The program word behind `raw`, when the stage is a `raw` stage and names one.
#[must_use]
pub fn raw_program(stage: &Stage) -> Option<&str> {
    if !is_raw(stage) {
        return None;
    }
    match stage.arguments.first() {
        Some(Argument::Word(word)) => Some(&word.text),
        _ => None,
    }
}

/// A declared type that names no concrete element carries the upstream one through, which is what
/// lets spec §42.1's fourth stage still report `stream<ono.process/1>`.
fn concrete(declared: &IoType, carried: Option<&str>) -> IoType {
    match carried {
        Some(element) if declared.is_open() => declared.with_element(element),
        _ => declared.clone(),
    }
}

/// The field paths a stage's expression arguments read.
fn read_fields(arguments: &[Argument]) -> Vec<String> {
    let mut fields = Vec::new();
    for argument in arguments {
        if let Argument::Value(expression) = argument {
            collect_fields(expression, &mut fields);
        }
    }
    fields.dedup();
    fields
}

fn collect_fields(expression: &Expr, into: &mut Vec<String>) {
    match expression {
        Expr::Path(path) => into.push(path.name.clone()),
        Expr::Field(access) => {
            let mut base = Vec::new();
            collect_fields(&access.base, &mut base);
            match base.first() {
                Some(root) => into.push(format!("{root}.{}", access.field)),
                None => into.push(access.field.clone()),
            }
        }
        Expr::Unary(unary) => collect_fields(&unary.operand, into),
        Expr::Binary(binary) => {
            collect_fields(&binary.lhs, into);
            collect_fields(&binary.rhs, into);
        }
        Expr::Index(index) => collect_fields(&index.base, into),
        Expr::Call(call) => {
            for argument in &call.arguments {
                collect_fields(argument, into);
            }
        }
        Expr::List(list) => {
            for item in &list.items {
                collect_fields(item, into);
            }
        }
        Expr::Record(record) => {
            for field in &record.fields {
                collect_fields(&field.value, into);
            }
        }
        _ => {}
    }
}
