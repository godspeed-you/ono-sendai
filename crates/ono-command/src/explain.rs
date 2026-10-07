//! `explain`: the execution plan of spec §42, produced without executing anything (spec §15.3).
//!
//! Every fact in a plan comes from the registry, the capability vocabulary or the provider
//! registry's *declarations*. No provider is queried, no action is attempted and no stream is
//! opened, which is exactly what makes `explain` safe to type in front of a destructive pipeline.

use std::fmt::Write as _;
use std::sync::Arc;

use std::path::PathBuf;

use ono_adapter::{Consumer, Negotiation, OutputDemand, Stdout};
use ono_parser::{Argument, Expr, Pipeline, RedirectOp, RedirectTarget, Stage, StageList};
use ono_pipeline::MaterializationLimits;
use ono_provider_api::{ProviderRegistry, Risk};
use ono_value::{MapValue, Value};

use crate::contract::{ExecutionClass, IoType, Origin, Privilege};
use crate::invoke::ContextFrame;
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
    /// A user function — step 2 of ADR-0011's resolution order, ahead of the registry and `PATH`.
    Function {
        /// The function's name.
        name: String,
    },
}

/// What the shell knows about a user function a stage calls, for the plan to report it.
///
/// The planner has no session: the shell that does decides whether a call can be continued as a
/// stage of the stream it stands in, and hands the answer over (v0.4.1 §26.2, ADR-0951).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionPlan {
    /// Where the function was declared, as the shell reports a span.
    pub declared: String,
    /// Whether a call at the head of a pipeline streams into the stages after it, or why not.
    pub at_head: Result<(), String>,
    /// Whether a call after another stage reads that stage's stream and streams on, or why not.
    pub with_input: Result<(), String>,
}

/// One stage of an execution plan.
#[derive(Debug, Clone, PartialEq)]
pub struct StagePlan {
    ordinal: usize,
    source: String,
    resolution: Resolution,
    origin: Origin,
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
    /// The explicit spelling a context frame narrows the stage to (spec §14.5, ADR-0225).
    narrowed: Option<String>,
    demand: Option<(OutputDemand, String)>,
    /// The input type the contract declares, before the upstream type was threaded in.
    declared_input: Option<String>,
    /// Whether the stage is `raw <program>`, which bypasses adaptation (spec v0.3 §1.17).
    raw: bool,
    /// What the adapter registry answered for an external stage (spec v0.3 §1.6).
    adaptation: Option<Adaptation>,
    /// Where the stage sits in the streaming classification matrix (v0.4.1 Appendix E).
    execution: Option<ExecutionClass>,
    /// What a materializing stage may collect, in values and bytes (v0.4.1 §22.2, §22.4).
    budget: Option<(u64, u64)>,
    /// The program an external word resolves to on `PATH`, when the shell looked it up.
    path: Option<String>,
    /// What a mutating stage does, in the words of spec §42.2 (`signal TERM`).
    operation: Option<String>,
    /// What the remote agent answered about adapting this stage, inside a link frame.
    remote_adaptation: Option<String>,
}

/// The adapter registry's answer for one external stage, as the plan shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Adaptation {
    /// The state in the words of spec v0.3 §1.57.
    pub state: String,
    /// The invocation that will actually run, when the stage is adapted.
    pub argv: Option<Vec<String>>,
    /// Every adapter that answered and why the winner won, when more than none did.
    pub candidates: Option<(Vec<String>, String)>,
    /// The negotiation itself, for the executor.
    pub negotiation: Negotiation,
}

/// What a plan is made against besides the registries: where stdout goes, which adapters are
/// installed, and how a program name resolves on `PATH` (ADR-0056).
#[derive(Clone, Copy)]
pub struct PlanContext<'a> {
    /// Where the shell's own stdout goes.
    pub stdout: Stdout,
    /// The adapter registry, when adaptation is to be planned.
    pub adapters: Option<&'a ono_adapter::Registry>,
    /// Resolves a program name to the path the shell would run, when `PATH` is known.
    pub executables: Option<&'a dyn Fn(&str) -> Option<PathBuf>>,
    /// The context frames in force, so the plan can print the explicit spelling a frame narrows
    /// the stage to (spec §14.5, ADR-0023, ADR-0225).
    pub context: &'a [ContextFrame],
    /// What a materializing stage of this pipeline may collect (v0.4.1 §22.2, §22.4).
    ///
    /// The plan shows the budget the pipeline would really run under, so a user who has narrowed
    /// `limits.materialize_bytes` sees their own figure rather than Appendix A's.
    pub limits: MaterializationLimits,
    /// The user functions a head word names, when the shell knows any: step 2 of the resolution
    /// order outranks the registry, so the plan asks before it consults it (ADR-0011, ADR-0951).
    pub functions: Option<&'a dyn Fn(&str) -> Option<FunctionPlan>>,
}

impl std::fmt::Debug for PlanContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlanContext")
            .field("stdout", &self.stdout)
            .field("adapters", &self.adapters.is_some())
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl StagePlan {
    /// Where the stage sits in the streaming classification matrix of v0.4.1 Appendix E.
    #[must_use]
    pub const fn execution(&self) -> Option<ExecutionClass> {
        self.execution
    }

    /// How v0.4.1 §22.4 names the stage's execution mode.
    ///
    /// `global materialization` for a stage that holds its input, `streaming` for one that does
    /// not. §22.4 calls showing this *"a product feature of honesty, not merely debug output"*,
    /// which is why it is a field of the plan and not a line only the renderer knows.
    #[must_use]
    pub fn execution_mode(&self) -> &'static str {
        self.execution
            .map_or("streaming", ExecutionClass::execution_mode)
    }

    /// Whether the stage holds its input rather than forwarding it (v0.4.1 §22.1).
    #[must_use]
    pub fn materializes(&self) -> bool {
        self.execution.is_some_and(ExecutionClass::may_materialize)
    }

    /// Whether the stage refuses a declared-unbounded upstream immediately (v0.4.1 §22.3).
    #[must_use]
    pub fn requires_finite_input(&self) -> bool {
        self.execution
            .is_some_and(ExecutionClass::requires_finite_input)
    }

    /// What a materializing stage may collect: values and bytes (v0.4.1 §22.2).
    ///
    /// `None` for a stage that materializes nothing, because a budget shown beside a stage that
    /// cannot spend it is the opposite of what §22.4 asks for.
    #[must_use]
    pub const fn budget(&self) -> Option<(u64, u64)> {
        self.budget
    }

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

    /// Where the command the stage names came from: the core, or the package that contributed
    /// it (spec §31.64).
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
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

    /// What the adapter registry answered, for an external stage planned with a registry.
    #[must_use]
    pub fn adaptation(&self) -> Option<&Adaptation> {
        self.adaptation.as_ref()
    }

    /// The schema an adapted stage produces, when the registry answered with a plan.
    #[must_use]
    pub fn adapted_schema(&self) -> Option<&str> {
        self.adaptation
            .as_ref()
            .and_then(|adaptation| adaptation.negotiation.plan())
            .map(|plan| plan.adapter().schema())
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

    /// Records the program an external word resolves to on `PATH` (ADR-0011 T11).
    pub fn set_path(&mut self, path: impl Into<String>) {
        self.path = Some(path.into());
    }

    /// Records what a mutating stage does, as spec §42.2 words it (`signal TERM`).
    pub fn set_operation(&mut self, operation: impl Into<String>) {
        self.operation = Some(operation.into());
    }

    /// Records what the remote agent answered about adapting this stage (spec v0.3 §1.54).
    pub fn set_remote_adaptation(&mut self, state: impl Into<String>) {
        self.remote_adaptation = Some(state.into());
    }

    fn to_value(&self) -> Value {
        let text = |value: Option<&str>| value.map_or(Value::Null, Value::string);
        let mut map = MapValue::default();
        map.insert("ordinal".into(), Value::Int(self.ordinal as i128));
        map.insert("source".into(), Value::string(&self.source));
        let (resolution, head) = match &self.resolution {
            Resolution::Native { .. } => ("native", None),
            Resolution::External { head } => ("external", Some(head.as_str())),
            Resolution::Function { name } => ("function", Some(name.as_str())),
            Resolution::Value => ("value", None),
        };
        map.insert("resolution".into(), Value::string(resolution));
        map.insert("head".into(), text(head));
        map.insert("command".into(), text(self.command()));
        map.insert("origin".into(), Value::string(&self.origin.to_string()));
        map.insert("narrowed".into(), text(self.narrowed.as_deref()));
        map.insert("provider".into(), text(self.provider()));
        map.insert("capability".into(), text(self.capability()));
        map.insert(
            "fields".into(),
            Value::list(self.fields.iter().map(|field| Value::string(field))),
        );
        map.insert("input".into(), Value::string(&self.input));
        map.insert("output".into(), Value::string(&self.output));
        map.insert("streaming".into(), Value::Bool(self.streaming));
        // v0.4.1 §22.4 is "a product feature of honesty, not merely debug output", so what the
        // stage will hold is a field a script can read and not only a line a renderer prints.
        map.insert("execution".into(), Value::string(self.execution_mode()));
        map.insert(
            "execution_class".into(),
            text(self.execution.map(ExecutionClass::id)),
        );
        map.insert(
            "requires".into(),
            text(self.requires_finite_input().then_some("finite input")),
        );
        map.insert(
            "budget_items".into(),
            self.budget
                .map_or(Value::Null, |(items, _)| Value::Int(i128::from(items))),
        );
        map.insert(
            "budget_bytes".into(),
            self.budget
                .map_or(Value::Null, |(_, bytes)| Value::Int(i128::from(bytes))),
        );
        map.insert(
            "privilege".into(),
            text(self.privilege.map(Privilege::as_str)),
        );
        map.insert("risk".into(), text(self.risk.map(Risk::as_str)));
        map.insert("operation".into(), text(self.operation.as_deref()));
        map.insert("raw".into(), Value::Bool(self.raw));
        map.insert(
            "demand".into(),
            self.demand()
                .map_or(Value::Null, |demand| Value::string(&demand.to_string())),
        );
        map.insert(
            "demand_reason".into(),
            text(self.demand.as_ref().map(|(_, reason)| reason.as_str())),
        );
        let adaptation = self.adaptation.as_ref();
        map.insert(
            "adaptation".into(),
            text(adaptation.map(|adaptation| adaptation.state.as_str())),
        );
        map.insert(
            "argv".into(),
            adaptation
                .and_then(|adaptation| adaptation.argv.as_ref())
                .map_or(Value::Null, |argv| {
                    Value::list(argv.iter().map(|word| Value::string(word)))
                }),
        );
        let candidates = adaptation.and_then(|adaptation| adaptation.candidates.as_ref());
        map.insert(
            "candidates".into(),
            candidates.map_or(Value::Null, |(candidates, _)| {
                Value::list(candidates.iter().map(|candidate| Value::string(candidate)))
            }),
        );
        map.insert(
            "selection".into(),
            text(candidates.map(|(_, selection)| selection.as_str())),
        );
        map.insert(
            "remote_adaptation".into(),
            text(self.remote_adaptation.as_deref()),
        );
        map.insert("path".into(), text(self.path.as_deref()));
        map.insert(
            "notes".into(),
            Value::list(self.notes.iter().map(|note| Value::string(note))),
        );
        Value::Map(Arc::new(map))
    }
}

/// The plan of a whole pipeline: what each stage resolves to, and what it would do.
///
/// One value carries everything `explain` says, and the rendering is drawn from that value
/// ([`render_plan`]): text and data cannot drift because the text is made from the data
/// (ADR-0942).
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionPlan {
    subject: String,
    source: String,
    stages: Vec<StagePlan>,
    /// Every alias expanded on the way to `source`: its name and its expansion.
    aliases: Vec<(String, String)>,
    /// What a prefix assignment sets for this pipeline alone (spec §54, ADR-0943).
    environment: Vec<(String, String)>,
    /// The execution context of a link frame, as labelled rows (spec §42.2).
    context: Option<Vec<(String, Option<String>)>>,
    notes: Vec<String>,
    /// Whether the subject named a sealed change plan rather than a pipeline (ADR-0814).
    change_plan: bool,
}

impl ExecutionPlan {
    /// The explanation of a sealed v0.6 plan: no stages, the explanation itself as notes
    /// (ADR-0814).
    #[must_use]
    pub fn of_change_plan(subject: &str, lines: Vec<String>) -> Self {
        Self {
            subject: subject.to_owned(),
            source: subject.to_owned(),
            stages: Vec::new(),
            aliases: Vec::new(),
            environment: Vec::new(),
            context: None,
            notes: lines,
            change_plan: true,
        }
    }

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

    /// The stages, for the shell to add what only it knows about them.
    pub fn stages_mut(&mut self) -> &mut [StagePlan] {
        &mut self.stages
    }

    /// Records what `explain` was asked about, when it differs from what was planned — an alias,
    /// a prefix assignment.
    pub fn set_subject(&mut self, subject: impl Into<String>) {
        self.subject = subject.into();
    }

    /// Records an alias expanded on the way to the planned pipeline (ADR-0011 step 3).
    pub fn push_alias(&mut self, name: impl Into<String>, expansion: impl Into<String>) {
        self.aliases.push((name.into(), expansion.into()));
    }

    /// Records a variable a prefix assignment sets for this pipeline (spec §54, ADR-0943).
    pub fn push_environment(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.environment.push((name.into(), value.into()));
    }

    /// Records the execution context of a link frame, as labelled rows (spec §42.2).
    pub fn set_context(&mut self, rows: Vec<(String, Option<String>)>) {
        self.context = Some(rows);
    }

    /// Adds a sentence to the plan's notes.
    pub fn push_note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }

    /// Whether any stage would change something outside the shell.
    #[must_use]
    pub fn is_mutating(&self) -> bool {
        self.stages
            .iter()
            .any(|stage| stage.risk.is_some_and(Risk::changes_the_world))
    }

    /// Per stage, the schema an adapter gives it — what the pre-flight check of spec §11.3
    /// needs to know about programs (ADR-0067).
    #[must_use]
    pub fn adapted_schemas(&self) -> Vec<Option<String>> {
        self.stages
            .iter()
            .map(|stage| stage.adapted_schema().map(str::to_owned))
            .collect()
    }

    /// The plan as plain text, in the shape of spec §42.1: the rendering of [`Self::to_value`].
    #[must_use]
    pub fn render(&self) -> String {
        render_plan(&self.to_value()).join("\n")
    }

    /// The plan as an `ono.execution-plan/1` record, so a script can read it without parsing the
    /// rendering.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let pairs = |pairs: &[(String, String)], first: &str, second: &str| {
            Value::list(pairs.iter().map(|(a, b)| {
                let mut map = MapValue::default();
                map.insert(first.into(), Value::string(a));
                map.insert(second.into(), Value::string(b));
                Value::Map(Arc::new(map))
            }))
        };
        let context = self.context.as_ref().map_or(Value::Null, |rows| {
            let mut map = MapValue::default();
            for (label, value) in rows {
                map.insert(
                    label.as_str().into(),
                    value.as_deref().map_or(Value::Null, Value::string),
                );
            }
            Value::Map(Arc::new(map))
        });
        let fields = vec![
            ("subject", Value::string(&self.subject)),
            (
                "kind",
                Value::string(if self.change_plan {
                    "change-plan"
                } else {
                    "pipeline"
                }),
            ),
            ("source", Value::string(&self.source)),
            ("mutating", Value::Bool(self.is_mutating())),
            ("aliases", pairs(&self.aliases, "name", "expansion")),
            ("environment", pairs(&self.environment, "name", "value")),
            ("context", context),
            (
                "stages",
                Value::list(self.stages.iter().map(StagePlan::to_value)),
            ),
            (
                "notes",
                Value::list(self.notes.iter().map(|note| Value::string(note))),
            ),
        ];
        let record = ono_value::builtin_schemas()
            .get(&ono_value::SchemaId::new("ono.execution-plan", 1))
            .and_then(|schema| {
                let provenance = ono_value::Provenance::local("ono.shell", schema.id().clone());
                let mut builder = ono_value::RecordValue::builder(schema, provenance);
                for (field, value) in &fields {
                    builder = builder.set(field, value.clone()).ok()?;
                }
                Some(builder.build().into_value())
            });
        record.unwrap_or_else(|| {
            let mut map = MapValue::default();
            for (field, value) in fields {
                map.insert(field.into(), value);
            }
            Value::Map(Arc::new(map))
        })
    }
}

/// The lines an `ono.execution-plan/1` record renders as: spec §42.1's PIPELINE layout, then the
/// execution context and the mutations of §42.2, then everything else the plan notes.
///
/// Every line is drawn from a field of the value, so what a terminal shows and what `to json`
/// writes cannot say different things (ADR-0942). The text is not sanitised here: the plan
/// quotes source text and paths a user does not control, and the caller that writes the lines
/// to a terminal neutralises them (ADR-0015 T1).
#[must_use]
pub fn render_plan(plan: &Value) -> Vec<String> {
    let field = |name: &str| -> Value {
        match plan {
            Value::Record(record) => record.get(name).cloned().unwrap_or(Value::Null),
            Value::Map(map) => map.get(name).cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        }
    };
    let list = |value: Value| -> Vec<Value> {
        value
            .as_list()
            .map(|items| items.to_vec())
            .unwrap_or_default()
    };
    let mut pieces: Vec<String> = Vec::new();

    for alias in list(field("aliases")) {
        pieces.push(format!(
            "  `{}` is an alias for `{}` — step 3 of the resolution order; explaining the \
             expansion",
            entry_text(&alias, "name").unwrap_or_default(),
            entry_text(&alias, "expansion").unwrap_or_default()
        ));
    }
    let notes: Vec<String> = list(field("notes"))
        .iter()
        .filter_map(|note| note.as_str().ok().map(str::to_owned))
        .collect();
    if field("kind").as_str().ok() == Some("change-plan") {
        pieces.extend(notes);
        return pieces
            .iter()
            .flat_map(|piece| piece.split('\n'))
            .map(str::to_owned)
            .collect();
    }

    let stages = list(field("stages"));
    let mut text = String::from("PIPELINE\n");
    for (name, value) in list(field("environment"))
        .iter()
        .filter_map(|entry| Some((entry_text(entry, "name")?, entry_text(entry, "value")?)))
    {
        row(&mut text, "environment", &format!("{name}={value}"));
    }
    for stage in &stages {
        render_stage(stage, &mut text);
        let _ = writeln!(text);
    }
    pieces.push(text);

    let context = field("context");
    let host = entry_text(&context, "host");
    if !context.is_null() {
        let mut block = String::from("EXECUTION CONTEXT\n");
        for label in ["link", "transport", "mode", "answers", "identity"] {
            if let Some(value) = entry_text(&context, label) {
                row(&mut block, label, &value);
            }
        }
        pieces.push(block);
    }
    for stage in &stages {
        let Some(risk) = entry_text(stage, "risk") else {
            continue;
        };
        if !matches!(risk.as_str(), "mutate" | "destructive") {
            continue;
        }
        let mut block = String::from("MUTATION\n");
        row(
            &mut block,
            "stage",
            &format!(
                "{}. {}",
                entry_int(stage, "ordinal"),
                entry_text(stage, "source").unwrap_or_default()
            ),
        );
        row(
            &mut block,
            "operation",
            &entry_text(stage, "operation").unwrap_or_else(|| "unknown".to_owned()),
        );
        row(
            &mut block,
            "targets",
            &entry_text(stage, "input").unwrap_or_default(),
        );
        row(
            &mut block,
            "risk",
            &if context.is_null() {
                risk
            } else {
                format!("{risk} + remote")
            },
        );
        if let Some(privilege) = entry_text(stage, "privilege") {
            row(&mut block, "privilege", &privilege);
        }
        pieces.push(block);
    }
    for stage in &stages {
        if let Some(state) = entry_text(stage, "remote_adaptation") {
            pieces.push(format!(
                "  adaptation on {}: {state}",
                host.as_deref().unwrap_or("the link")
            ));
        }
    }
    pieces.extend(notes);
    pieces
        .iter()
        .flat_map(|piece| piece.split('\n'))
        .map(str::to_owned)
        .collect()
}

/// One stage of the PIPELINE block, in spec §42.1's layout.
fn render_stage(stage: &Value, into: &mut String) {
    let text = |name: &str| entry_text(stage, name);
    let _ = writeln!(
        into,
        "{}. {}",
        entry_int(stage, "ordinal"),
        text("source").unwrap_or_default()
    );
    if let Some(id) = text("command") {
        row(into, "command", &id);
    }
    // A core command's origin is the answer nobody asks for; a contributed one's is the first
    // question about it (spec §31.64).
    if let Some(origin) = text("origin").filter(|origin| origin != "core") {
        row(into, "origin", &origin);
    }
    if let Some(narrowed) = text("narrowed") {
        row(into, "narrowed", &narrowed);
    }
    let head = text("head").unwrap_or_default();
    match text("resolution").as_deref() {
        Some("external") => row(
            into,
            "resolution",
            &format!("`{head}` is not a native command"),
        ),
        Some("function") => row(
            into,
            "resolution",
            &format!("user function `{head}` — step 2 of the resolution order (ADR-0011)"),
        ),
        _ => {}
    }
    if let Some(provider) = text("provider") {
        row(into, "provider", &provider);
    }
    if let Some(capability) = text("capability") {
        row(into, "capability", &capability);
    }
    let fields = entry_strings(stage, "fields");
    if !fields.is_empty() {
        row(into, "field", &fields.join(", "));
    }
    row(into, "input", &text("input").unwrap_or_default());
    row(into, "output", &text("output").unwrap_or_default());
    if entry_bool(stage, "raw") {
        row(
            into,
            "adaptation",
            &format!("bypassed (`{}`, spec v0.3 §1.17)", ono_adapter::RAW),
        );
    }
    if let Some(demand) = text("demand") {
        row(
            into,
            "demand",
            &format!("{demand} ({})", text("demand_reason").unwrap_or_default()),
        );
    }
    if let Some(adaptation) = text("adaptation") {
        row(into, "adaptation", &adaptation);
        let argv = entry_strings(stage, "argv");
        if !argv.is_empty() {
            row(into, "argv", &argv.join(" "));
        }
        let candidates = entry_strings(stage, "candidates");
        if let Some(selection) = text("selection") {
            row(
                into,
                "candidates",
                &format!("{} ({selection})", candidates.join(", ")),
            );
        }
    }
    row(
        into,
        "streaming",
        if entry_bool(stage, "streaming") {
            "yes"
        } else {
            "no"
        },
    );
    // v0.4.1 §22.4's three lines, and only where they say something. A budget printed beside a
    // stage that materializes nothing would be noise pretending to be a guarantee.
    if text("execution_class").is_some() {
        row(into, "execution", &text("execution").unwrap_or_default());
        if let Some(requires) = text("requires") {
            row(into, "requires", &requires);
        }
    }
    if let (Some(items), Some(bytes)) = (
        entry_u64(stage, "budget_items"),
        entry_u64(stage, "budget_bytes"),
    ) {
        row(
            into,
            "budget",
            &format!("{items} values / {}", human_bytes(bytes)),
        );
    }
    if let Some(privilege) = text("privilege") {
        row(into, "privilege", &privilege);
    }
    if let Some(risk) = text("risk") {
        row(into, "risk", &risk);
    }
    for note in entry_strings(stage, "notes") {
        row(into, "note", &note);
    }
}

/// One entry of a map, or of a record, as text; `None` where it is absent or null.
fn entry_text(value: &Value, name: &str) -> Option<String> {
    let entry = match value {
        Value::Map(map) => map.get(name),
        Value::Record(record) => record.get(name),
        _ => None,
    }?;
    match entry {
        Value::Null => None,
        Value::String(_) => entry.as_str().ok().map(str::to_owned),
        other => Some(other.to_string()),
    }
}

fn entry_int(value: &Value, name: &str) -> i128 {
    match value.as_map().ok().and_then(|map| map.get(name)) {
        Some(Value::Int(number)) => *number,
        _ => 0,
    }
}

fn entry_u64(value: &Value, name: &str) -> Option<u64> {
    match value.as_map().ok().and_then(|map| map.get(name)) {
        Some(Value::Int(number)) => u64::try_from(*number).ok(),
        _ => None,
    }
}

fn entry_bool(value: &Value, name: &str) -> bool {
    matches!(
        value.as_map().ok().and_then(|map| map.get(name)),
        Some(Value::Bool(true))
    )
}

fn entry_strings(value: &Value, name: &str) -> Vec<String> {
    value
        .as_map()
        .ok()
        .and_then(|map| map.get(name))
        .and_then(|list| list.as_list().ok())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().ok().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
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
    plan_with(
        registry,
        providers,
        pipeline,
        source,
        &PlanContext {
            stdout,
            adapters: None,
            executables: None,
            context: &[],
            limits: MaterializationLimits::default(),
            functions: None,
        },
    )
}

/// The plan for `pipeline` in `context`: with the adapter registry and `PATH` resolution
/// available, every external stage also reports what the registry answered (spec v0.3 §1.23).
#[must_use]
pub fn plan_with(
    registry: &CommandRegistry,
    providers: Option<&ProviderRegistry>,
    pipeline: &Pipeline,
    source: &str,
    context: &PlanContext<'_>,
) -> ExecutionPlan {
    let stdout = context.stdout;
    let mut stages: Vec<StagePlan> = Vec::new();
    let mut ordinal = 1;

    let lists =
        std::iter::once(&pipeline.head).chain(pipeline.tail.iter().map(|chained| &chained.list));
    for list in lists {
        let mut upstream: Option<IoType> = None;
        let first = stages.len();
        for stage in &list.stages {
            let planned = match function_stage(context, stage, source, ordinal, upstream.as_ref()) {
                Some(planned) => planned,
                None => plan_stage(
                    registry,
                    providers,
                    context.context,
                    stage,
                    source,
                    ordinal,
                    upstream.as_ref(),
                    context.limits,
                ),
            };
            upstream = Some(IoType::from_text(&planned.output));
            stages.push(planned);
            ordinal += 1;
        }
        plan_demands(&mut stages[first..], list, stdout);
        plan_adaptations(&mut stages[first..], list, source, context);
        rethread(
            &mut stages[first..],
            list,
            registry,
            providers,
            context.context,
            source,
            context.limits,
        );
    }

    ExecutionPlan {
        subject: source.to_owned(),
        source: source.to_owned(),
        stages,
        aliases: Vec::new(),
        environment: Vec::new(),
        context: None,
        notes: Vec::new(),
        change_plan: false,
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
        if list.stages.get(index).is_some_and(is_adapt) {
            stages[index].demand = Some((
                OutputDemand::Structured { schema: None },
                format!("`{}` requires structure", ono_adapter::ADAPT),
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
                    Resolution::Native { .. } | Resolution::Function { .. } => {
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

/// Threads an adapted stage's schema into the stages after it (spec v0.3 §1.61): once the
/// registry has answered, an adapted program's output is `stream<schema>` rather than bytes,
/// and every later native stage is planned again over that type.
fn rethread(
    stages: &mut [StagePlan],
    list: &StageList,
    registry: &CommandRegistry,
    providers: Option<&ProviderRegistry>,
    frames: &[ContextFrame],
    source: &str,
    limits: MaterializationLimits,
) {
    let mut upstream: Option<IoType> = None;
    let mut changed = false;
    for (index, planned) in stages.iter_mut().enumerate() {
        if let Some(schema) = planned.adapted_schema() {
            let schema = schema.to_owned();
            planned.output = format!("stream<{schema}>");
            planned.element_schema = Some(schema);
            changed = true;
        } else if changed
            && matches!(planned.resolution, Resolution::Native { .. })
            && let Some(stage) = list.stages.get(index)
        {
            let ordinal = planned.ordinal;
            let keep = (planned.demand.clone(), planned.adaptation.clone());
            *planned = plan_stage(
                registry,
                providers,
                frames,
                stage,
                source,
                ordinal,
                upstream.as_ref(),
                limits,
            );
            planned.demand = keep.0;
            planned.adaptation = keep.1;
        }
        upstream = Some(IoType::from_text(&planned.output));
    }
}

/// Asks the adapter registry about every external stage that has a demand (spec v0.3 §1.6).
///
/// Nothing here runs the subject: the registry may run a declared version probe of a different
/// program, which ADR-0056 allows `explain` because a guessed version could contradict the run.
fn plan_adaptations(
    stages: &mut [StagePlan],
    list: &StageList,
    source: &str,
    context: &PlanContext<'_>,
) {
    let (Some(adapters), Some(executables)) = (context.adapters, context.executables) else {
        return;
    };
    for (index, planned) in stages.iter_mut().enumerate() {
        let Resolution::External { head } = &planned.resolution else {
            continue;
        };
        if planned.raw {
            continue;
        }
        let Some((demand, _)) = &planned.demand else {
            continue;
        };
        let Some(stage) = list.stages.get(index) else {
            continue;
        };
        let Some(path) = executables(head) else {
            continue;
        };
        let mut argv = vec![head.clone()];
        let literal = literal_arguments(stage, source);
        argv.extend(if is_adapt(stage) {
            literal.into_iter().skip(1).collect::<Vec<String>>()
        } else {
            literal
        });
        let negotiation = adapters.negotiate(&path, &argv, demand);
        let (argv, candidates) = match &negotiation {
            Negotiation::StructuredSupported {
                plan,
                candidates,
                selection,
            }
            | Negotiation::StructuredSupportedWithLimits {
                plan,
                candidates,
                selection,
                ..
            } => (
                Some(plan.argv().to_vec()),
                Some((candidates.clone(), selection.clone())),
            ),
            _ => (None, None),
        };
        planned.adaptation = Some(Adaptation {
            state: negotiation.describe(demand),
            argv,
            candidates,
            negotiation,
        });
    }
}

/// The stage's arguments as the words the program would see, as far as the source can say
/// without evaluating anything: a word is itself, an option is its spelling, a value is its
/// source text.
#[must_use]
pub fn literal_arguments(stage: &Stage, source: &str) -> Vec<String> {
    stage
        .arguments
        .iter()
        .map(|argument| match argument {
            Argument::Word(word) => word.text.clone(),
            Argument::Option(option) => match &option.value {
                Some(value) => format!("--{}={}", option.name, value.span().of(source)),
                None => format!("--{}", option.name),
            },
            other => other.span().of(source).to_owned(),
        })
        .collect()
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

/// The plan of a stage whose head names a user function, when the shell said it does.
///
/// Step 2 of the resolution order, before the registry: a function named like a native command
/// shadows it (ADR-0011, ADR-0070). `fn:` forces the step; `ono:` and `exec:` skip it.
fn function_stage(
    context: &PlanContext<'_>,
    stage: &Stage,
    source: &str,
    ordinal: usize,
    upstream: Option<&IoType>,
) -> Option<StagePlan> {
    let lookup = context.functions?;
    let ono_parser::StageHead::Command(name) = &stage.head else {
        return None;
    };
    if !matches!(name.namespace.as_deref(), None | Some("fn")) {
        return None;
    }
    let function = lookup(&name.name)?;
    let (streams, note) = match upstream {
        None => match &function.at_head {
            Ok(()) => (
                true,
                "its body streams into the stages after the call (v0.4.1 §26.2)".to_owned(),
            ),
            Err(reason) => (
                false,
                format!(
                    "its result is collected before the stages after the call run, so its input \
                     must be finite: {reason} (v0.4.1 §26.2)"
                ),
            ),
        },
        Some(_) => match &function.with_input {
            Ok(()) => (
                true,
                "its body reads the stream in front of the call and streams into the stages \
                 after it (ADR-0951)"
                    .to_owned(),
            ),
            Err(reason) => (
                false,
                format!("it cannot read the stream in front of it: {reason} (ADR-0951)"),
            ),
        },
    };
    Some(StagePlan {
        ordinal,
        source: stage.span.of(source).trim().to_owned(),
        resolution: Resolution::Function {
            name: name.name.clone(),
        },
        origin: Origin::Core,
        provider: None,
        capability: None,
        input: upstream.map_or_else(|| "null".to_owned(), |io| io.text().to_owned()),
        output: "stream<any>".to_owned(),
        element_schema: None,
        streaming: streams,
        privilege: None,
        risk: None,
        fields: read_fields(&stage.arguments),
        notes: vec![format!("declared at {}; {note}", function.declared)],
        narrowed: None,
        demand: None,
        declared_input: Some("stream<any>".to_owned()),
        raw: false,
        adaptation: None,
        execution: None,
        budget: None,
        path: None,
        operation: None,
        remote_adaptation: None,
    })
}

fn plan_stage(
    registry: &CommandRegistry,
    providers: Option<&ProviderRegistry>,
    frames: &[ContextFrame],
    stage: &Stage,
    source: &str,
    ordinal: usize,
    upstream: Option<&IoType>,
    limits: MaterializationLimits,
) -> StagePlan {
    let text = stage.span.of(source).trim().to_owned();
    let carried = upstream.and_then(IoType::element_schema);
    let mut fields = read_fields(&stage.arguments);

    let Some(head) = stage.head.name() else {
        return StagePlan {
            ordinal,
            source: text,
            resolution: Resolution::Value,
            origin: Origin::Core,
            provider: None,
            capability: None,
            input: "null".to_owned(),
            output: upstream.map_or_else(|| "any".to_owned(), |io| io.text().to_owned()),
            element_schema: carried.map(str::to_owned),
            streaming: false,
            privilege: None,
            risk: None,
            fields,
            narrowed: None,
            notes: vec!["the stage's head is a value, not a command".to_owned()],
            demand: None,
            declared_input: None,
            raw: false,
            adaptation: None,
            execution: None,
            budget: None,
            path: None,
            operation: None,
            remote_adaptation: None,
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
            origin: Origin::Core,
            provider: None,
            capability: None,
            input: upstream.map_or_else(|| "bytes".to_owned(), |io| io.text().to_owned()),
            output: "bytes".to_owned(),
            element_schema: None,
            streaming: true,
            privilege: None,
            risk: None,
            fields,
            narrowed: None,
            notes: vec![
                "the program on PATH, run with no argv rewrite, no decoder and no renderer \
                 (spec v0.3 §1.17, ADR-0054)"
                    .to_owned(),
            ],
            demand: None,
            declared_input: None,
            raw: true,
            adaptation: None,
            execution: None,
            budget: None,
            path: None,
            operation: None,
            remote_adaptation: None,
        };
    }

    if is_adapt(stage) {
        let program = adapt_program(stage).unwrap_or(ono_adapter::ADAPT);
        return StagePlan {
            ordinal,
            source: text,
            resolution: Resolution::External {
                head: program.to_owned(),
            },
            origin: Origin::Core,
            provider: None,
            capability: None,
            input: upstream.map_or_else(|| "bytes".to_owned(), |io| io.text().to_owned()),
            output: "stream<any>".to_owned(),
            element_schema: None,
            streaming: true,
            privilege: None,
            risk: None,
            fields,
            narrowed: None,
            notes: vec![
                "forced adaptation: the program's output must become values, or the stage fails \
                 (spec v0.3 §1.18, ADR-0064)"
                    .to_owned(),
            ],
            demand: None,
            declared_input: None,
            raw: false,
            adaptation: None,
            execution: None,
            budget: None,
            path: None,
            operation: None,
            remote_adaptation: None,
        };
    }

    let resolution = registry.resolve(head, &stage.arguments);
    let Ok(resolved) = resolution else {
        // A verb the registry knows, refused only for its target word, is reported as that
        // refusal: `trace group root` is `trace` with a target it has no command for, and the
        // executor answers `resolve.target_not_found` rather than searching `PATH` (ADR-0217).
        let refusal = resolution
            .err()
            .filter(|error| error.code() == ono_core::ErrorCode::ResolveTargetNotFound);
        return StagePlan {
            ordinal,
            source: text,
            resolution: Resolution::External {
                head: head.to_owned(),
            },
            origin: Origin::Core,
            provider: None,
            capability: None,
            input: upstream.map_or_else(|| "bytes".to_owned(), |io| io.text().to_owned()),
            output: "bytes".to_owned(),
            element_schema: None,
            streaming: true,
            privilege: None,
            risk: None,
            fields,
            narrowed: None,
            notes: vec![match &refusal {
                Some(error) => error.message().to_owned(),
                None => "resolved after the registry: a user function, an alias, or an \
                         executable on PATH (ADR-0011)"
                    .to_owned(),
            }],
            demand: None,
            declared_input: None,
            raw: false,
            adaptation: None,
            execution: None,
            budget: None,
            path: None,
            operation: None,
            remote_adaptation: None,
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
    let mut narrowed = None;
    // v0.4.1 §22.4: the class of this invocation, which an option can change — `measure` folds in
    // constant state, `measure --median` holds the distribution (ADR-0953).
    let mut execution = contract.execution();
    match contract.bind(resolved.arguments) {
        // The fields a stage reads are the ones its first selector names: `sort memory desc`
        // reads `memory`, and `desc` is the direction rather than a field.
        Ok(bound) => {
            execution = contract.execution_for(&bound);
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
            // Everything a frame contributes can be written out by hand, and `explain` is where
            // it is written (spec §14.5, ADR-0023). The plan reports the same narrowing the
            // command table performs, from the same function (ADR-0225).
            if let Some(registered) = providers
                && let Ok(Some(filled)) =
                    crate::narrow::narrow(contract, registered, frames, &bound)
            {
                narrowed = crate::narrow::spelling(contract, &bound, &filled);
            }
        }
        Err(error) => notes.push(format!("arguments do not bind: {}", error.message())),
    }

    let capability = contract
        .provider_capability()
        .and_then(|id| registry.capability(id));
    // The provider that would answer, which is the first one that can: two providers claim
    // `package` — one per package database — and naming the one this machine does not have would
    // be a plan for a different machine (spec §27, ADR-0422). Where none can answer, the plan
    // still names what would, because that is what the reader is asking about.
    let provider = match (providers, contract.target()) {
        (Some(registered), Some(target)) => registered
            .provider_for(target)
            .ok()
            .or_else(|| registered.for_target(target).first().copied())
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
        origin: contract.origin().clone(),
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
        narrowed,
        demand: None,
        declared_input: Some(contract.input().text().to_owned()),
        raw: false,
        adaptation: None,
        // v0.4.1 §22.4: what the stage will hold, and what it is allowed to hold, derived from
        // the classification the contract declares rather than restated here (ADR-0460).
        execution,
        budget: execution
            .filter(|class| class.may_materialize())
            .map(|_| (limits.max_items(), limits.max_bytes())),
        path: None,
        operation: None,
        remote_adaptation: None,
    }
}

/// Whether `stage` is `raw <program> …`, the bypass of spec v0.3 §1.17.
///
/// The keyword is a bare, unqualified head: `exec:raw` is a program called `raw`.
#[must_use]
pub fn is_raw(stage: &Stage) -> bool {
    is_keyword(stage, ono_adapter::RAW)
}

/// Whether `stage` is `adapt <program> …`, the forced adaptation of spec v0.3 §1.18.
#[must_use]
pub fn is_adapt(stage: &Stage) -> bool {
    is_keyword(stage, ono_adapter::ADAPT)
}

fn is_keyword(stage: &Stage, keyword: &str) -> bool {
    matches!(&stage.head, ono_parser::StageHead::Command(name)
        if name.namespace.is_none() && name.name == keyword)
}

/// The program word behind `raw`, when the stage is a `raw` stage and names one.
#[must_use]
pub fn raw_program(stage: &Stage) -> Option<&str> {
    is_raw(stage).then(|| keyword_program(stage)).flatten()
}

/// The program word behind `adapt`, when the stage is an `adapt` stage and names one.
#[must_use]
pub fn adapt_program(stage: &Stage) -> Option<&str> {
    is_adapt(stage).then(|| keyword_program(stage)).flatten()
}

fn keyword_program(stage: &Stage) -> Option<&str> {
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

/// Renders a byte ceiling the way v0.4.1 §22.4's example plan writes it: `128 MiB`.
///
/// Appendix A: "Limits MUST be expressed internally in integer base units and rendered in
/// human-readable units separately." This is the rendering half.
fn human_bytes(bytes: u64) -> String {
    const UNITS: [(&str, u64); 4] = [
        ("GiB", 1 << 30),
        ("MiB", 1 << 20),
        ("KiB", 1 << 10),
        ("B", 1),
    ];
    for (name, scale) in UNITS {
        if bytes >= scale {
            let whole = bytes / scale;
            let tenths = (bytes % scale) * 10 / scale;
            return if tenths == 0 {
                format!("{whole} {name}")
            } else {
                format!("{whole}.{tenths} {name}")
            };
        }
    }
    "0 B".to_owned()
}
