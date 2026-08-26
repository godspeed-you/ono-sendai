//! The commands that describe the shell rather than the system: `help`, `explain`, `type`,
//! `inspect`, `get command` and `find command`.
//!
//! All six answer from the registry and from the values already in hand, so none of them touches
//! a provider and none of them runs anything. That is what makes `explain` safe to type in front
//! of a destructive pipeline (spec §15.3, §42) and `type` safe to type in front of an expensive
//! one (spec §15.2).

use std::sync::{Arc, OnceLock};

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, ValueStream};
use ono_value::{
    ErrorValue, FieldAccess, FieldDef, FieldType, MapValue, Provenance, RecordValue, Schema,
    SchemaId, Value,
};

use crate::contract::CommandContract;
use crate::invoke::{CommandImpl, Invocation, Outcome};
use crate::registry::CommandRegistry;

/// Which meta command an implementation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Help,
    Explain,
    Type,
    Inspect,
    GetCommand,
    FindCommand,
}

/// One meta command, holding the registry it describes.
#[derive(Debug)]
pub(crate) struct MetaCommand {
    id: String,
    kind: Kind,
    registry: &'static CommandRegistry,
}

impl MetaCommand {
    pub(crate) fn new(id: &str, kind: Kind, registry: &'static CommandRegistry) -> Self {
        Self {
            id: id.to_owned(),
            kind,
            registry,
        }
    }
}

impl CommandImpl for MetaCommand {
    fn id(&self) -> &str {
        &self.id
    }

    fn invoke(&self, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        match self.kind {
            Kind::Help => {
                let topic = ctx
                    .arguments()
                    .selector("topic")
                    .and_then(|value| value.as_str().ok())
                    .unwrap_or_default()
                    .to_owned();
                let page = crate::help(self.registry, Some(ctx.providers()), &topic)?;
                Ok(values([page.to_value()]))
            }
            Kind::Explain => {
                let subject = ctx.arguments().require_selector("subject")?.as_str()?;
                let parsed = ono_parser::parse(subject);
                let pipeline = parsed
                    .program()
                    .statements
                    .first()
                    .and_then(ono_parser::Statement::as_pipeline)
                    .ok_or_else(|| not_a_pipeline(subject))?;
                let plan = crate::plan(self.registry, Some(ctx.providers()), pipeline, subject);
                Ok(values([plan.to_value()]))
            }
            Kind::Type => self.describe_type(ctx),
            Kind::Inspect => self.inspect(ctx),
            Kind::GetCommand => Ok(values(self.commands(ctx))),
            Kind::FindCommand => Ok(values(self.found(ctx))),
        }
    }
}

impl MetaCommand {
    /// `type` — the schema of what a pipeline produces, without producing it.
    fn describe_type(&self, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        if let Some(subject) = subject_text(ctx.arguments().selector("subject")) {
            let subject = subject.as_str();
            let parsed = ono_parser::parse(subject);
            let pipeline = parsed
                .program()
                .statements
                .first()
                .and_then(ono_parser::Statement::as_pipeline)
                .ok_or_else(|| not_a_pipeline(subject))?;
            let plan = crate::plan(self.registry, Some(ctx.providers()), pipeline, subject);
            let last = plan
                .stages()
                .last()
                .ok_or_else(|| not_a_pipeline(subject))?;
            let schema = last
                .element_schema()
                .and_then(|id| id.parse::<SchemaId>().ok())
                .and_then(|id| ono_value::builtin_schemas().get(&id));
            return Ok(values([type_of_declaration(
                subject,
                last.output(),
                schema.as_deref(),
            )]));
        }

        // No subject: the type of what is flowing through, read from the first value alone. A
        // stream is not consumed to answer a question about its shape.
        let input = ctx.take_input().ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                "`type` needs a subject or a value, and neither was given",
            )
            .with_help("write `type get process`, or pipe a value into it")
        })?;
        Ok(Outcome::Values(input.stage(
            Boundedness::Bounded,
            move |mut input, sink| async move {
                let described = match input.next_value(&sink).await {
                    Some(value) => type_of_value(&value),
                    None => type_of_value(&Value::Null),
                };
                let _ = sink.send(described).await;
            },
        )))
    }

    /// `inspect` — every field, its access, and where the record came from (spec §15.2, §25.2).
    fn inspect(&self, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        if let Some(subject) = ctx.arguments().selector("subject") {
            return Ok(values([inspection(subject)]));
        }
        let input = ctx.take_input().ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                "`inspect` needs a value, and none was given",
            )
            .with_help("pipe one in, or name it as in `inspect @1`")
        })?;
        Ok(Outcome::Values(input.stage(
            Boundedness::Bounded,
            move |mut input, sink| async move {
                while let Some(value) = input.next_value(&sink).await {
                    if sink.send(inspection(&value)).await.is_err() {
                        return;
                    }
                }
            },
        )))
    }

    /// `get command` — the registry as objects (spec §15.4).
    fn commands(&self, ctx: &Invocation<'_>) -> Vec<Value> {
        let arguments = ctx.arguments();
        let name = arguments
            .selector("name")
            .and_then(|value| value.as_str().ok());
        let verb = arguments
            .option("verb")
            .and_then(|value| value.as_str().ok());
        let target = arguments
            .option("target")
            .and_then(|value| value.as_str().ok());
        let stability = arguments
            .option("stability")
            .and_then(|value| value.as_str().ok());

        self.registry
            .commands()
            .iter()
            .filter(|command| {
                name.is_none_or(|name| command.spelling() == name || command.id() == name)
            })
            .filter(|command| verb.is_none_or(|verb| command.verb() == verb))
            .filter(|command| target.is_none_or(|target| command.target() == Some(target)))
            .filter(|command| stability.is_none_or(|level| command.stability().as_str() == level))
            .map(command_record)
            .collect()
    }

    /// `find command` — the same objects, discovered by what they do (spec §15.4).
    ///
    /// The search is over everything the registry says about a command, its selectors and options
    /// included, because "the command that lists listening sockets" is how someone actually looks
    /// for `get socket --listening`. Results are ranked by how many of the query's words they
    /// answer, so the closest match is first rather than buried.
    fn found(&self, ctx: &Invocation<'_>) -> Vec<Value> {
        let query = ctx
            .arguments()
            .selector("query")
            .and_then(|value| value.as_str().ok())
            .unwrap_or_default()
            .to_lowercase();
        let words: Vec<&str> = query.split_whitespace().collect();
        if words.is_empty() {
            return self
                .registry
                .commands()
                .iter()
                .map(command_record)
                .collect();
        }

        let mut ranked: Vec<(usize, &CommandContract)> = self
            .registry
            .commands()
            .iter()
            .map(|command| (matched_words(command, &words), command))
            .filter(|(matched, _)| *matched > 0)
            .collect();
        ranked.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.id().cmp(right.1.id()))
        });
        ranked
            .into_iter()
            .map(|(_, command)| command_record(command))
            .collect()
    }
}

/// How many of `words` the registry's description of `command` answers.
fn matched_words(command: &CommandContract, words: &[&str]) -> usize {
    let mut haystack = format!(
        "{} {} {} {}",
        command.id(),
        command.spelling(),
        command.summary(),
        command.note().unwrap_or_default()
    );
    for parameter in command.selectors().iter().chain(command.options()) {
        haystack.push(' ');
        haystack.push_str(parameter.name());
        haystack.push(' ');
        haystack.push_str(parameter.doc());
    }
    let haystack = haystack.to_lowercase();
    words
        .iter()
        .filter(|word| {
            // A plural in the question and a singular in the documentation are the same word to
            // anyone but a substring match.
            haystack.contains(**word)
                || word
                    .strip_suffix('s')
                    .is_some_and(|stem| !stem.is_empty() && haystack.contains(stem))
        })
        .count()
}

fn values(items: impl IntoIterator<Item = Value>) -> Outcome {
    Outcome::Values(ValueStream::from_values(items))
}

fn not_a_pipeline(subject: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ParseSyntax,
        format!("`{subject}` is not a pipeline"),
    )
    .with_help("quote a whole pipeline, as in `explain \"get process | to json\"`")
}

/// The schema of `ono.command/1`: the registry's own objects.
///
/// It lives here rather than in `docs/spec/schemas/` because it *is* the registry, described in
/// the registry's own vocabulary; a hand-written copy elsewhere would be one more thing to drift.
fn command_schema() -> Result<Arc<Schema>, ErrorValue> {
    static SCHEMA: OnceLock<Result<Arc<Schema>, ErrorValue>> = OnceLock::new();
    SCHEMA
        .get_or_init(|| {
            Schema::builder(SchemaId::new("ono.command", 1), "Command")
                .doc("One command of the Ono-Sendai registry (spec §27).")
                .field(FieldDef::new("id", FieldType::String).required())
                .field(FieldDef::new("verb", FieldType::String).required())
                .field(FieldDef::new("target", FieldType::String).nullable())
                .field(FieldDef::new("spelling", FieldType::String).required())
                .field(FieldDef::new("summary", FieldType::String).required())
                .field(FieldDef::new("stability", FieldType::String).required())
                .field(FieldDef::new("argument_mode", FieldType::String).required())
                .field(FieldDef::new("input", FieldType::String).required())
                .field(FieldDef::new("output", FieldType::String).required())
                .field(FieldDef::new("capability", FieldType::String).nullable())
                .field(FieldDef::new("privilege", FieldType::String).required())
                .field(FieldDef::new("streaming", FieldType::Bool).required())
                .field(FieldDef::new("phase", FieldType::String).required())
                .field(FieldDef::new("examples", FieldType::list(FieldType::String)).nullable())
                .identity(["id"])
                .default_view(["spelling", "summary", "stability"])
                .build()
                .map(Arc::new)
        })
        .clone()
}

fn command_record(contract: &CommandContract) -> Value {
    let schema = match command_schema() {
        Ok(schema) => schema,
        Err(error) => return error.into_value(),
    };
    let provenance =
        Provenance::local("ono.registry", schema.id().clone()).from_source("docs/spec/commands/");
    let built = RecordValue::builder(schema, provenance)
        .set("id", Value::string(contract.id()))
        .and_then(|record| record.set("verb", Value::string(contract.verb())))
        .and_then(|record| {
            record.set(
                "target",
                contract.target().map_or(Value::Null, Value::string),
            )
        })
        .and_then(|record| record.set("spelling", Value::string(&contract.spelling())))
        .and_then(|record| record.set("summary", Value::string(contract.summary())))
        .and_then(|record| record.set("stability", Value::string(contract.stability().as_str())))
        .and_then(|record| {
            record.set(
                "argument_mode",
                Value::string(contract.argument_mode().as_str()),
            )
        })
        .and_then(|record| record.set("input", Value::string(contract.input().text())))
        .and_then(|record| record.set("output", Value::string(contract.output().text())))
        .and_then(|record| {
            record.set(
                "capability",
                contract
                    .provider_capability()
                    .map_or(Value::Null, Value::string),
            )
        })
        .and_then(|record| record.set("privilege", Value::string(contract.privilege().as_str())))
        .and_then(|record| record.set("streaming", Value::Bool(contract.is_streaming())))
        .and_then(|record| record.set("phase", Value::string(&contract.phase().to_string())))
        .and_then(|record| {
            record.set(
                "examples",
                Value::list(contract.examples().iter().map(|line| Value::string(line))),
            )
        });
    match built {
        Ok(record) => record.build().into_value(),
        Err(error) => error.into_value(),
    }
}

/// The subject as one line of source, however many words it was written as.
///
/// The contract's own example is `type get socket` — no quotes — so the selector is repeatable
/// and a multi-word subject is the pipeline those words spell, exactly as if it had been quoted.
fn subject_text(selector: Option<&Value>) -> Option<String> {
    match selector? {
        Value::List(words) => {
            let words: Vec<&str> = words.iter().filter_map(|word| word.as_str().ok()).collect();
            (!words.is_empty()).then(|| words.join(" "))
        }
        word => word.as_str().ok().map(str::to_owned),
    }
}

/// What `type` reports about a value that is already in hand.
fn type_of_value(value: &Value) -> Value {
    let mut described = MapValue::new();
    described.insert("type".into(), Value::string(value.type_name()));
    match value {
        Value::Record(record) => {
            described.insert(
                "schema".into(),
                Value::string(&record.schema_id().to_string()),
            );
            described.insert("fields".into(), field_list(record.schema()));
        }
        Value::Error(error) => {
            described.insert("code".into(), Value::string(error.code().name()));
        }
        _ => {
            described.insert("schema".into(), Value::Null);
            described.insert("fields".into(), Value::Null);
        }
    }
    Value::Map(Arc::new(described))
}

/// What `type` reports about a pipeline it did not run.
fn type_of_declaration(subject: &str, output: &str, schema: Option<&Schema>) -> Value {
    let mut described = MapValue::new();
    described.insert("subject".into(), Value::string(subject));
    described.insert("type".into(), Value::string(output));
    described.insert(
        "schema".into(),
        schema.map_or(Value::Null, |schema| {
            Value::string(&schema.id().to_string())
        }),
    );
    described.insert("fields".into(), schema.map_or(Value::Null, field_list));
    Value::Map(Arc::new(described))
}

fn field_list(schema: &Schema) -> Value {
    Value::list(schema.fields().iter().map(|field| {
        let mut described = MapValue::new();
        described.insert("name".into(), Value::string(field.name()));
        described.insert("type".into(), Value::string(&field.ty().name()));
        described.insert("nullable".into(), Value::Bool(!field.is_required()));
        described.insert("doc".into(), field.doc().map_or(Value::Null, Value::string));
        Value::Map(Arc::new(described))
    }))
}

/// The detailed view of one value: fields with their access, provenance, and — for an error — the
/// whole causal chain (spec §15.2, §16.2, §25.2).
fn inspection(value: &Value) -> Value {
    let mut described = MapValue::new();
    described.insert("type".into(), Value::string(value.type_name()));
    match value {
        Value::Record(record) => {
            described.insert(
                "schema".into(),
                Value::string(&record.schema_id().to_string()),
            );
            described.insert("identity".into(), Value::Map(Arc::new(record.identity())));
            described.insert("fields".into(), inspected_fields(record));
            described.insert("provenance".into(), provenance_map(record.provenance()));
        }
        Value::Error(error) => {
            described.insert("error".into(), error_map(error));
        }
        other => {
            described.insert("value".into(), other.clone());
        }
    }
    Value::Map(Arc::new(described))
}

fn inspected_fields(record: &RecordValue) -> Value {
    Value::list(record.schema().fields().iter().map(|field| {
        let mut described = MapValue::new();
        described.insert("name".into(), Value::string(field.name()));
        described.insert("type".into(), Value::string(&field.ty().name()));
        // The three absences of spec §10.5, reported as three different words rather than as one
        // empty cell.
        let (access, held) = match record.access(field.name()) {
            FieldAccess::Known(value) => ("known", value),
            FieldAccess::Unknown => ("unknown", Value::Null),
            FieldAccess::Absent => ("absent", Value::Null),
            FieldAccess::Failed(error) => ("failed", Value::Error(error)),
        };
        described.insert("access".into(), Value::string(access));
        described.insert("value".into(), held);
        Value::Map(Arc::new(described))
    }))
}

fn provenance_map(provenance: &Provenance) -> Value {
    let mut described = MapValue::new();
    described.insert("provider".into(), Value::string(provenance.provider()));
    described.insert(
        "observed".into(),
        provenance.observed().map_or(Value::Null, Value::Timestamp),
    );
    described.insert(
        "source".into(),
        provenance.source().map_or(Value::Null, Value::string),
    );
    described.insert("link".into(), Value::string(&provenance.link().to_string()));
    described.insert(
        "schema".into(),
        Value::string(&provenance.schema().to_string()),
    );
    described.insert(
        "confidence".into(),
        provenance.confidence().map_or(Value::Null, Value::Float),
    );
    Value::Map(Arc::new(described))
}

/// An error with the whole chain that produced it, which is the causal chain spec §16.2 asks
/// `inspect` to show.
fn error_map(error: &ErrorValue) -> Value {
    let mut described = MapValue::new();
    described.insert("code".into(), Value::string(error.code().code()));
    described.insert("name".into(), Value::string(error.code().name()));
    described.insert("kind".into(), Value::string(error.kind().as_str()));
    described.insert("message".into(), Value::string(error.message()));
    described.insert(
        "help".into(),
        error.help().map_or(Value::Null, Value::string),
    );
    described.insert(
        "target".into(),
        error
            .target()
            .map_or(Value::Null, ono_value::ValueRef::to_value),
    );
    described.insert(
        "retryable".into(),
        error.retryable().map_or(Value::Null, Value::Bool),
    );
    described.insert(
        "metadata".into(),
        Value::Map(Arc::new(error.metadata().clone())),
    );
    described.insert(
        "chain".into(),
        Value::list(error.chain().skip(1).map(|cause| {
            let mut link = MapValue::new();
            link.insert("code".into(), Value::string(cause.code().name()));
            link.insert("message".into(), Value::string(cause.message()));
            Value::Map(Arc::new(link))
        })),
    );
    Value::Map(Arc::new(described))
}
