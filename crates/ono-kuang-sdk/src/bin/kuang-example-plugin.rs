//! The example plugin the conformance suite runs (spec §31.73, §31.74).
//!
//! The default mode is an honest plugin built on the SDK: it contributes commands and a
//! provider target, streams typed values under credit, observes cancellation, and reaches the
//! host API only through brokered calls.
//!
//! The `--misbehave=…` modes bypass the SDK on purpose and speak the wire directly, because a
//! misbehaving package would: `flood` emits beyond its granted credit, `garbage` and
//! `huge-frame` break the framing, `bad-hello` claims an identity the manifest does not carry.
//! The conformance suite asserts that each one ends in quarantine, not in a wedged shell.

#![allow(
    clippy::expect_used,
    reason = "a conformance fixture binary states its preconditions loudly; nothing here is \
              reachable from user input"
)]

use std::io::Write;

use ono_kuang_protocol::{
    CommandContribution, ContributionSet, EmitParams, Envelope, FrameLimits, Hello, InitResult,
    InvokeParams, InvokeResult, InvokeStatus, PACKAGE_FORMAT, SchemaContribution,
    SchemaFieldContribution, TargetContribution, method,
};
use ono_kuang_sdk::{Ctx, Outcome, Plugin};
use ono_value::{Provenance, RecordValue, Value};
use serde_json::json;

const PACKAGE: &str = "dev.example.echo";
const VERSION: &str = "0.1.0";
const ITEM_SCHEMA: &str = "dev.example.echo.item/1";

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--misbehave=flood") => misbehave(Mode::Flood),
        Some("--misbehave=garbage") => misbehave(Mode::Garbage),
        Some("--misbehave=huge-frame") => misbehave(Mode::HugeFrame),
        Some("--misbehave=bad-hello") => misbehave(Mode::BadHello),
        _ => honest().run(),
    }
}

fn item_schema_contribution() -> SchemaContribution {
    SchemaContribution {
        id: ITEM_SCHEMA.to_owned(),
        name: "EchoItem".to_owned(),
        summary: "One echoed item.".to_owned(),
        identity: vec!["seq".to_owned()],
        fields: vec![
            SchemaFieldContribution {
                name: "seq".to_owned(),
                field_type: "int".to_owned(),
                required: true,
                nullable: false,
            },
            SchemaFieldContribution {
                name: "label".to_owned(),
                field_type: "string".to_owned(),
                required: true,
                nullable: false,
            },
        ],
    }
}

fn command(
    id_suffix: &str,
    summary: &str,
    output: &str,
    capabilities: &[&str],
) -> CommandContribution {
    CommandContribution {
        id: format!("{PACKAGE}.command.{id_suffix}"),
        verb: "get".to_owned(),
        target: "echo-item".to_owned(),
        summary: summary.to_owned(),
        input: None,
        output: output.to_owned(),
        capabilities: capabilities.iter().map(|c| (*c).to_owned()).collect(),
        argument_mode: "expression".to_owned(),
        risk: None,
        examples: vec![format!("get echo-item | {id_suffix}")],
    }
}

fn item_record(seq: i64, label: &str) -> Value {
    let schema = item_schema_contribution()
        .to_schema()
        .expect("the fixture schema is valid");
    let schema_id = schema.id().clone();
    let record = RecordValue::builder(
        std::sync::Arc::new(schema),
        Provenance::local("plugin-self", schema_id),
    )
    .set("seq", Value::Int(i128::from(seq)))
    .and_then(|builder| builder.set("label", Value::String(label.into())))
    .expect("the fixture fields exist")
    .build();
    Value::Record(std::sync::Arc::new(record))
}

fn int_argument(ctx: &Ctx<'_>, name: &str, default: i64) -> i64 {
    ctx.arguments()
        .get(name)
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(default)
}

fn honest() -> Plugin {
    Plugin::new(PACKAGE, VERSION)
        .contribute_schema(item_schema_contribution())
        .contribute_target(TargetContribution {
            name: "echo-item".to_owned(),
            schema: ITEM_SCHEMA.to_owned(),
            summary: "Items the example package provides.".to_owned(),
            identity_doc: "Two observations are the same item when their `seq` matches.".to_owned(),
        })
        .contribute_command(command(
            "emit",
            "Emit counted integers.",
            "stream<int>",
            &[],
        ))
        .contribute_command(command(
            "count-forever",
            "Emit integers until cancelled.",
            "stream<int>",
            &[],
        ))
        .contribute_command(command(
            "read-file",
            "Report a file's size through the brokered filesystem.",
            "stream<int>",
            &["filesystem.read"],
        ))
        .contribute_command(command(
            "clock",
            "Tell the host's time.",
            "stream<string>",
            &["clock.read"],
        ))
        .contribute_command(command(
            "state-write",
            "Write a value into the package's persistent store.",
            "stream<string>",
            &["state.persist"],
        ))
        .contribute_command(command(
            "wrong-schema",
            "Emit a record outside the declared schema (a defect, on purpose).",
            &format!("stream<{ITEM_SCHEMA}>"),
            &[],
        ))
        .contribute_command(command(
            "sneaky-clock",
            "Call clock.now without declaring the capability (a defect, on purpose).",
            "stream<string>",
            &[],
        ))
        .contribute_command(command(
            "request-capability",
            "Make a runtime capability request.",
            "stream<string>",
            &[],
        ))
        .optional_feature("tell-time", "clock.read")
        .command(&format!("{PACKAGE}.command.emit"), |ctx| {
            let count = int_argument(ctx, "count", 3);
            for n in 1..=count {
                match ctx.emit(&Value::Int(i128::from(n))) {
                    Ok(()) => {}
                    Err(ono_kuang_sdk::EmitError::Cancelled) => return Outcome::Cancelled,
                    Err(ono_kuang_sdk::EmitError::Refused(error)) => return Outcome::Failed(error),
                    Err(ono_kuang_sdk::EmitError::Transport) => return Outcome::Cancelled,
                }
            }
            Outcome::Completed
        })
        .command(&format!("{PACKAGE}.command.count-forever"), |ctx| {
            let mut n: i128 = 0;
            loop {
                n += 1;
                match ctx.emit(&Value::Int(n)) {
                    Ok(()) => {}
                    Err(ono_kuang_sdk::EmitError::Cancelled) => return Outcome::Cancelled,
                    Err(ono_kuang_sdk::EmitError::Refused(error)) => return Outcome::Failed(error),
                    Err(ono_kuang_sdk::EmitError::Transport) => return Outcome::Cancelled,
                }
            }
        })
        .command(&format!("{PACKAGE}.command.read-file"), |ctx| {
            let path = ctx
                .arguments()
                .get("path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            match ctx.host_call(method::FILESYSTEM_READ, json!({"path": path})) {
                Ok(result) => {
                    let bytes = result
                        .get("content")
                        .and_then(|content| content.get("$bytes"))
                        .and_then(serde_json::Value::as_str)
                        .map_or(0, |hex| hex.len() / 2);
                    let _ = ctx.emit(&Value::Int(bytes as i128));
                    Outcome::Completed
                }
                Err(error) => Outcome::Failed(error),
            }
        })
        .command(&format!("{PACKAGE}.command.clock"), |ctx| {
            match ctx.clock_now() {
                Ok(now) => {
                    let _ = ctx.emit(&Value::String(now.into()));
                    Outcome::Completed
                }
                Err(error) => Outcome::Failed(error),
            }
        })
        .command(&format!("{PACKAGE}.command.state-write"), |ctx| {
            let key = ctx
                .arguments()
                .get("key")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("k")
                .to_owned();
            let size = int_argument(ctx, "size", 8).unsigned_abs() as usize;
            let value = "x".repeat(size);
            match ctx.host_call(
                method::STATE_SET,
                json!({"key": key, "class": "persistent", "value": value}),
            ) {
                Ok(_) => {
                    let _ = ctx.emit(&Value::String("stored".into()));
                    Outcome::Completed
                }
                Err(error) => Outcome::Failed(error),
            }
        })
        .command(&format!("{PACKAGE}.command.wrong-schema"), |ctx| {
            // Declared output is stream<dev.example.echo.item/1>; this emits a bare int.
            match ctx.emit(&Value::Int(42)) {
                Ok(()) => Outcome::Completed,
                Err(ono_kuang_sdk::EmitError::Refused(error)) => Outcome::Failed(error),
                Err(_) => Outcome::Cancelled,
            }
        })
        .command(
            &format!("{PACKAGE}.command.sneaky-clock"),
            |ctx| match ctx.clock_now() {
                Ok(now) => {
                    let _ = ctx.emit(&Value::String(now.into()));
                    Outcome::Completed
                }
                Err(error) => Outcome::Failed(error),
            },
        )
        .command(&format!("{PACKAGE}.command.request-capability"), |ctx| {
            let action = ctx
                .arguments()
                .get("action_context")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            match ctx.host_call(
                method::CAPABILITIES_REQUEST,
                json!({
                    "capability": "process.signal",
                    "purpose": "restart the unit the operator just selected",
                    "action_context": action,
                }),
            ) {
                Ok(lease) => {
                    let expires = lease
                        .get("expires_at")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    let _ = ctx.emit(&Value::String(format!("lease:{expires}").into()));
                    Outcome::Completed
                }
                Err(error) => Outcome::Failed(error),
            }
        })
        .provider("echo-item", |ctx| {
            for (seq, label) in [(1, "alpha"), (2, "beta"), (3, "gamma")] {
                if ctx.emit(&item_record(seq, label)).is_err() {
                    return Outcome::Cancelled;
                }
            }
            Outcome::Completed
        })
}

// --- the misbehaving paths, spoken raw on purpose ---------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Flood,
    Garbage,
    HugeFrame,
    BadHello,
}

fn misbehave(mode: Mode) {
    let limits = FrameLimits::default();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    let package = if mode == Mode::BadHello {
        "ono.evil"
    } else {
        PACKAGE
    };
    let hello = Envelope::Hello(Hello {
        format: PACKAGE_FORMAT.to_owned(),
        package: package.to_owned(),
        version: VERSION.to_owned(),
        kuang_api: ">=11.1 <12".to_owned(),
        contributions: ContributionSet {
            commands: vec![command("flood", "Emit beyond credit.", "stream<int>", &[])],
            targets: Vec::new(),
            schemas: Vec::new(),
        },
    });
    if ono_kuang_protocol::write_frame(&mut writer, &hello, limits).is_err() {
        return;
    }
    loop {
        let Ok(Some(envelope)) = ono_kuang_protocol::read_frame(&mut reader, limits) else {
            return;
        };
        let Envelope::Request {
            seq,
            method: method_name,
            params,
        } = envelope
        else {
            continue;
        };
        match method_name.as_str() {
            method::LIFECYCLE_INIT => {
                let result = InitResult {
                    ready: true,
                    disabled_features: Vec::new(),
                    error: None,
                };
                let response = Envelope::Response {
                    seq,
                    result: serde_json::to_value(result).ok(),
                    error: None,
                };
                if ono_kuang_protocol::write_frame(&mut writer, &response, limits).is_err() {
                    return;
                }
            }
            method::COMMAND_INVOKE => {
                let Ok(invoke) = serde_json::from_value::<InvokeParams>(params) else {
                    return;
                };
                match mode {
                    Mode::Flood => {
                        // Three values beyond the granted credit, in one emission.
                        let beyond = invoke.credit + 3;
                        let values = (0..beyond).map(|n| json!(i64::from(n))).collect();
                        let request = Envelope::Request {
                            seq: 1000,
                            method: method::STREAMS_EMIT.to_owned(),
                            params: serde_json::to_value(EmitParams {
                                handle: invoke.output,
                                values,
                            })
                            .unwrap_or(serde_json::Value::Null),
                        };
                        let _ = ono_kuang_protocol::write_frame(&mut writer, &request, limits);
                    }
                    Mode::Garbage => {
                        // A well-formed length declaring a payload that is not an envelope.
                        let _ = writer.write_all(&4u32.to_be_bytes());
                        let _ = writer.write_all(b"abcd");
                        let _ = writer.flush();
                    }
                    Mode::HugeFrame => {
                        // A declaration beyond the ceiling; the payload never follows.
                        let _ = writer.write_all(&(limits.max_frame + 1).to_be_bytes());
                        let _ = writer.flush();
                    }
                    Mode::BadHello => {
                        let response = Envelope::Response {
                            seq,
                            result: serde_json::to_value(InvokeResult {
                                status: InvokeStatus::Completed,
                                error: None,
                            })
                            .ok(),
                            error: None,
                        };
                        let _ = ono_kuang_protocol::write_frame(&mut writer, &response, limits);
                    }
                }
            }
            _ => {
                let response = Envelope::Response {
                    seq,
                    result: Some(serde_json::Value::Null),
                    error: None,
                };
                if ono_kuang_protocol::write_frame(&mut writer, &response, limits).is_err() {
                    return;
                }
            }
        }
    }
}
