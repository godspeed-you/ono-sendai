//! `to`, `from` and `format`: the explicit boundary between values and bytes (spec §12.3, §12.4).
//!
//! Spec §12.3 forbids the shell from formatting a structured stream on its way to a byte sink,
//! because the format it chose would silently become the contract. These three commands are the
//! way across, and being written out is the whole point of them.
//!
//! Each is a stage over its input rather than a collection in the implementation: `to json` has to
//! see the whole stream before it can write an array, but it waits for it *inside* the pipeline,
//! where backpressure and cancellation already apply (ADR-0013).

use std::sync::Arc;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, StreamSink};
use ono_render::{Cell, Column, Layout, Renderer, Table, View};
use ono_value::{ErrorValue, Value};

use crate::invoke::{CommandImpl, Invocation, Outcome};

/// The width a redirected rendering is laid out at.
///
/// Spec §50 requires behaviour to be deterministic when output is redirected, and a width read
/// from the terminal is the opposite of that. A pipe gets 80 columns, every time.
const REDIRECTED_WIDTH: usize = 80;

/// Which side of the boundary a command is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    /// `to <format>` — values out as text or bytes.
    Serialize,
    /// `from <format>` — text or bytes in as values.
    Deserialize,
    /// `format <renderer>` — values into a rendering meant for a person (spec §13).
    Render,
}

/// One conversion command.
#[derive(Debug)]
pub(crate) struct ConversionCommand {
    id: String,
    direction: Direction,
}

impl ConversionCommand {
    pub(crate) fn new(id: &str, direction: Direction) -> Self {
        Self {
            id: id.to_owned(),
            direction,
        }
    }
}

impl CommandImpl for ConversionCommand {
    fn id(&self) -> &str {
        &self.id
    }

    fn invoke(&self, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        let spelling = ctx.contract().spelling();
        let arguments = ctx.arguments();
        let selector = match self.direction {
            Direction::Render => "renderer",
            _ => "format",
        };
        let name = arguments.require_selector(selector)?.as_str()?.to_owned();
        let options = Options {
            pretty: arguments.flag("pretty"),
            human: arguments.flag("human"),
            headers: arguments.option("headers").cloned(),
            // `to text --field path` is the bridge spec §29.1 writes for feeding an ordinary
            // Unix tool. `docs/spec/commands/data.yaml` does not declare the option yet, so
            // binding rejects it before this is reached; the projection is implemented here so
            // that declaring it is the only change the contract needs.
            field: arguments
                .option("field")
                .and_then(|value| value.as_str().ok())
                .map(str::to_owned),
            columns: arguments.option("columns").cloned(),
            max_rows: arguments
                .option("max-rows")
                .and_then(|value| value.as_int().ok())
                .and_then(|rows| usize::try_from(rows).ok()),
        };

        // Fail on an unknown format before consuming anything: naming a format that does not
        // exist is a typo, and a typo must cost nothing (ADR-0013).
        self.check(&name, &spelling)?;

        let input = ctx.take_input().ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{spelling} {name}` needs a value, and nothing was piped into it"),
            )
        })?;
        let direction = self.direction;
        let output = input.stage(Boundedness::Bounded, move |mut input, sink| async move {
            let mut values = Vec::new();
            while let Some(value) = input.next_value(&sink).await {
                values.push(value);
            }
            let produced = match direction {
                Direction::Serialize => serialize(&name, &values, &options),
                Direction::Deserialize => deserialize(&name, &values, &options),
                Direction::Render => render(&name, &values, &options).map(|text| vec![text]),
            };
            emit(&sink, produced).await;
        });
        Ok(Outcome::Values(output))
    }
}

impl ConversionCommand {
    fn check(&self, name: &str, spelling: &str) -> Result<(), ErrorValue> {
        let known: &[&str] = match self.direction {
            Direction::Serialize => &["json", "yaml", "csv", "text", "bytes"],
            Direction::Deserialize => &["json", "yaml", "csv"],
            Direction::Render => &["table", "list", "tree", "raw", "hex"],
        };
        if known.contains(&name) {
            return Ok(());
        }
        let error = ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("`{spelling}` has no `{name}`"),
        );
        Err(match crate::suggest::closest(name, known.iter().copied()) {
            Some(near) => error.with_help(format!("did you mean `{spelling} {near}`?")),
            None => error.with_help(format!("`{spelling}` knows {}", known.join(", "))),
        })
    }
}

/// The options the three conversions read, gathered once so the stage owns them.
#[derive(Debug, Clone)]
struct Options {
    pretty: bool,
    human: bool,
    headers: Option<Value>,
    field: Option<String>,
    columns: Option<Value>,
    max_rows: Option<usize>,
}

async fn emit(sink: &StreamSink, produced: Result<Vec<Value>, ErrorValue>) {
    match produced {
        Ok(values) => {
            for value in values {
                if sink.send(value).await.is_err() {
                    return;
                }
            }
        }
        Err(error) => {
            let _ = sink.fail(error).await;
        }
    }
}

/// `to <format>`: one document for the whole stream.
///
/// A pipeline stage always receives a stream, so the document is always the stream's shape — a
/// JSON array, a CSV with one header, one text line per value. A single-valued stream is not
/// special-cased into a bare object: a script whose output shape depended on how many rows the
/// machine happened to have would be a script that breaks on a quiet day.
///
/// Every format here writes the **data**, in the shape spec §33.5 prints: a record is a plain
/// object of its fields, a byte size is its number of bytes, and no Ono envelope reaches the
/// wire. Spec §12.3 sends this document to a process that has never heard of Ono, so a schema id
/// or a provenance block in it would be noise the reader cannot use — provenance stays reachable
/// through `inspect` (spec §10.7). The tagged codec of ADR-0016 item 6 keeps its own job: round
/// trips inside the system, where a value must come back as the value it was.
fn serialize(format: &str, values: &[Value], options: &Options) -> Result<Vec<Value>, ErrorValue> {
    let rendered: Vec<Value> = if options.human {
        values.iter().map(humanise).collect()
    } else {
        values.to_vec()
    };
    let text = match format {
        "json" => {
            let json = ono_value::to_json_data(&Value::list(rendered));
            if options.pretty {
                serde_json::to_string_pretty(&json).map_err(json_failed)?
            } else {
                serde_json::to_string(&json).map_err(json_failed)?
            }
        }
        "yaml" => ono_value::to_yaml_data(&Value::list(rendered))?,
        "csv" => ono_value::to_csv(&rendered)?,
        "text" => ono_value::to_text(&rendered, options.field.as_deref())?,
        "bytes" => {
            return Ok(vec![Value::Bytes(ono_value::to_bytes(&Value::list(
                rendered,
            ))?)]);
        }
        other => return Err(unknown_format("to", other)),
    };
    Ok(vec![Value::string(&text)])
}

/// `from <format>`: an explicit representation becomes values again.
fn deserialize(
    format: &str,
    values: &[Value],
    options: &Options,
) -> Result<Vec<Value>, ErrorValue> {
    let text = text_of(values)?;
    let schemas = ono_value::builtin_schemas();
    let decoded = match format {
        "json" => ono_value::from_json_str(&text, schemas)?,
        "yaml" => ono_value::from_yaml(&text, schemas)?,
        "csv" => {
            if matches!(options.headers, Some(Value::Bool(false))) {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    "this CSV codec reads a header row and names the fields from it",
                )
                .with_help(
                    "a headerless document has no field names to give the values; add a header \
                     row, or read it with `to text` and name the columns yourself",
                ));
            }
            ono_value::from_csv(&text)?
        }
        other => return Err(unknown_format("from", other)),
    };
    // A document that holds a sequence becomes a stream of its items, which is what makes
    // `from json | where ...` work; anything else is one value.
    Ok(match decoded {
        Value::List(items) => items.iter().cloned().collect(),
        single => vec![single],
    })
}

/// `format <renderer>`: a rendering, made explicit so it can be piped (spec §12.3, §13).
fn render(renderer: &str, values: &[Value], options: &Options) -> Result<Value, ErrorValue> {
    let view = match renderer {
        "table" => View::Table,
        "list" => View::List,
        "tree" => View::Tree,
        "raw" => View::Raw,
        "hex" => View::Hex,
        other => return Err(unknown_format("format", other)),
    };
    let mut layout = Layout::new(REDIRECTED_WIDTH);
    if let Some(rows) = options.max_rows {
        layout = layout.max_rows(rows);
    }
    let painter = Renderer::new();
    let lines = match (view, columns_of(options)) {
        (View::Table, Some(columns)) => layout.render(&chosen_columns(&painter, values, &columns)),
        _ => layout.render_view(&painter, values, view),
    };
    Ok(Value::string(&lines.join("\n")))
}

/// The columns `--columns` names, where it names any.
fn columns_of(options: &Options) -> Option<Vec<String>> {
    match options.columns.as_ref()? {
        Value::List(items) => Some(
            items
                .iter()
                .filter_map(|item| ono_value::canonical_text(item).ok())
                .collect(),
        ),
        other => ono_value::canonical_text(other).ok().map(|one| vec![one]),
    }
}

/// A table over exactly the columns the user asked for (spec §13.3).
///
/// A field the value does not have renders as the failure of reading it, not as a blank cell:
/// spec §10.5 keeps "absent" and "unknown" apart, and a renderer is the last place they should be
/// allowed to merge.
fn chosen_columns(renderer: &Renderer, values: &[Value], columns: &[String]) -> Table {
    let mut table = Table::new(
        columns
            .iter()
            .map(|column| Column::new(column.to_uppercase()))
            .collect(),
    );
    for value in values {
        let cells = columns
            .iter()
            .map(
                |column| match value.follow(&[ono_value::FieldStep::required(column)]) {
                    Ok(field) => renderer.cell(&field),
                    Err(error) => Cell::new(error.code().name()),
                },
            )
            .collect();
        table.push_row(cells);
    }
    table
}

/// The display form of a semantic scalar, for `--human` (spec §33.5).
fn humanise(value: &Value) -> Value {
    let renderer = Renderer::new();
    match value {
        Value::ByteSize(_) | Value::Duration(_) | Value::Timestamp(_) | Value::Percent(_) => {
            Value::string(renderer.cell(value).text())
        }
        Value::List(items) => Value::list(items.iter().map(humanise)),
        // A record's fields are where the byte sizes and durations actually live; the display
        // forms replace them in a plain map of the same fields, declared ones first, exactly
        // as the record would have been written (spec §33.5).
        Value::Record(record) => {
            let mut humanised = ono_value::MapValue::new();
            for field in record.schema().fields() {
                let value = record.get(field.name()).cloned().unwrap_or(Value::Null);
                humanised.insert(field.name().into(), humanise(&value));
            }
            for (key, item) in record.extra().iter() {
                humanised.insert(key.into(), humanise(item));
            }
            Value::Map(Arc::new(humanised))
        }
        Value::Map(map) => {
            let mut humanised = ono_value::MapValue::new();
            for (key, item) in map.iter() {
                humanised.insert(key.into(), humanise(item));
            }
            Value::Map(Arc::new(humanised))
        }
        other => other.clone(),
    }
}

/// The text a `from` stage was given: one document, however many chunks carried it.
fn text_of(values: &[Value]) -> Result<String, ErrorValue> {
    let mut text = String::new();
    for value in values {
        match value {
            Value::String(chunk) => text.push_str(chunk),
            Value::Bytes(raw) => text.push_str(std::str::from_utf8(raw).map_err(|_| {
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    "these bytes are not valid text, so they are not a document to parse",
                )
                .with_help("spec §12.2 keeps undecodable bytes; decode them explicitly first")
            })?),
            other => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!(
                        "`from` parses text, and a {} is not text",
                        other.type_name()
                    ),
                )
                .with_help("a value that is already structured needs no `from`"));
            }
        }
    }
    Ok(text)
}

fn unknown_format(verb: &str, format: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!("`{verb}` has no `{format}`"),
    )
}

fn json_failed(error: serde_json::Error) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        "the value could not be serialized as JSON",
    )
    .with_help(error.to_string())
}
