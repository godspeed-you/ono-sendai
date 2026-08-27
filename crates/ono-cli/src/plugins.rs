//! KUANG/11 at the shell boundary (spec §31): discovery, loading, and contributed commands.
//!
//! The libraries do the hard part — the supervisor negotiates before spawn, brokers every
//! capability call and audits every decision (ADR-0040, ADR-0041). This module is the shell's
//! integration step the supervisor deliberately leaves out: finding installed packages, keeping
//! loaded ones on the session, and routing a `<package>:command` head into an invocation.

use std::path::PathBuf;

use ono_core::{ErrorCode, ExitStatus};
use ono_kuang_protocol::Manifest;
use ono_kuang_supervisor::{LoadConfig, LoadedPlugin, Supervisor};
use ono_value::{ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::session::Session;

/// One discovered package: its directory and its parsed manifest.
pub struct Installed {
    /// Where the package lives.
    pub directory: PathBuf,
    /// The validated manifest.
    pub manifest: Manifest,
}

/// The directories `ONO_PLUGIN_PATH` names, or the user's plugin directory.
fn plugin_path(session: &Session) -> Vec<PathBuf> {
    if let Some(path) = session.env_var("ONO_PLUGIN_PATH") {
        return std::env::split_paths(path).collect();
    }
    session
        .home()
        .map(|home| home.join(".config/ono/plugins"))
        .into_iter()
        .collect()
}

/// Every package installed under the plugin path, in directory order.
///
/// A directory whose manifest does not validate is reported as a failure entry rather than
/// silently skipped: an installed package that cannot load is a fact about this machine.
pub fn installed(session: &Session) -> (Vec<Installed>, Vec<ErrorValue>) {
    let mut found = Vec::new();
    let mut failures = Vec::new();
    for root in plugin_path(session) {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        let mut directories: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        directories.sort();
        for directory in directories {
            let manifest_path = directory.join("manifest.yaml");
            let Ok(text) = std::fs::read_to_string(&manifest_path) else {
                continue;
            };
            match Manifest::parse(&text) {
                Ok(manifest) => found.push(Installed {
                    directory,
                    manifest,
                }),
                Err(error) => failures.push(ErrorValue::new(
                    ErrorCode::ProviderSchemaViolation,
                    format!(
                        "{} holds a package that does not validate: {}",
                        directory.display(),
                        error
                    ),
                )),
            }
        }
    }
    (found, failures)
}

/// Runs `get plugin`: the installed set, with the session's runtime states over it (spec §31.8).
///
/// # Errors
///
/// None in practice; failures are reported per package.
pub fn get_plugin(session: &mut Session) -> Eval<ExitStatus> {
    let (packages, failures) = installed(session);
    for failure in &failures {
        eprintln!("ono: {failure}");
    }
    for package in packages {
        let id = package.manifest.package.id.clone();
        let state = session
            .plugin(&id)
            .map_or("installed", |loaded| loaded.state().as_str());
        println!("{id}  {}  {state}", package.manifest.package.version);
    }
    Ok(ExitStatus::SUCCESS)
}

/// Runs `load plugin <id>` (spec §31.10): negotiate, spawn, keep.
///
/// # Errors
///
/// The structured refusals of validation and negotiation, exactly as the supervisor raises
/// them — a denied required capability refuses before the binary ever starts.
pub fn load_plugin(session: &mut Session, id: &str) -> Eval<ExitStatus> {
    load_plugin_with(session, id, &LoadOptions::default())
}

/// What `load plugin` was told besides the package: explicit grants and allowances.
#[derive(Debug, Default, Clone)]
pub struct LoadOptions {
    /// Capabilities granted for this load, `--grant <capability>` (spec §31.18: explicit).
    pub grants: Vec<String>,
    /// Whether an experimental adapter pack may influence structured output
    /// (spec v0.3 §1.56), `--allow-experimental`.
    pub allow_experimental: bool,
}

impl LoadOptions {
    /// Reads the options out of the words after `load plugin`, answering the package id too.
    #[must_use]
    pub fn from_words(words: &[String]) -> (Option<String>, Self) {
        let mut options = Self::default();
        let mut id = None;
        let mut iter = words.iter().filter(|word| *word != "plugin");
        while let Some(word) = iter.next() {
            match word.as_str() {
                "--grant" => {
                    if let Some(capability) = iter.next() {
                        options.grants.push(capability.clone());
                    }
                }
                "--allow-experimental" => options.allow_experimental = true,
                other if other.starts_with("--") => {}
                other => {
                    if id.is_none() {
                        id = Some(other.to_owned());
                    }
                }
            }
        }
        (id, options)
    }
}

/// Runs `load plugin <id> [--grant <capability>] [--allow-experimental]`.
///
/// # Errors
///
/// The structured refusals of validation and negotiation.
pub fn load_plugin_with(
    session: &mut Session,
    id: &str,
    options: &LoadOptions,
) -> Eval<ExitStatus> {
    let (packages, _) = installed(session);
    let Some(package) = packages
        .into_iter()
        .find(|package| package.manifest.package.id == id)
    else {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("no installed package answers to `{id}`"),
            )
            .with_help("`get plugin` lists the installed set (spec §31.8)"),
        ));
    };

    // The policy is default-deny; what the user said on the command line is the only grant
    // (spec §31.18). A grant takes the scope the manifest asked for, never a wider one.
    let mut policy = ono_kuang_supervisor::Policy::deny_all();
    for granted in &options.grants {
        let capability: ono_kuang_protocol::Capability = granted.parse().map_err(|_| {
            Flow::Failed(
                ErrorValue::new(
                    ErrorCode::ResolveTargetNotFound,
                    format!("`{granted}` is not a capability"),
                )
                .with_help("`help capabilities` lists them"),
            )
        })?;
        let scope = package
            .manifest
            .required_capabilities
            .iter()
            .chain(&package.manifest.optional_capabilities)
            .find(|request| request.capability == capability)
            .and_then(|request| request.scope.clone());
        policy = policy.grant(capability, scope);
    }

    // A declarative adapter package (spec v0.3 §1.45): packs, no runtime — or packs beside one.
    let contributes_adapters = package
        .manifest
        .contributions
        .as_ref()
        .and_then(|contributions| contributions.adapters.as_ref())
        .is_some_and(|paths| !paths.is_empty());
    if contributes_adapters {
        let packs = ono_kuang_supervisor::validate_package(&package.directory, &package.manifest)
            .map_err(|error| {
            Flow::Failed(
                ErrorValue::new(error.code, error.message)
                    .with_metadata("adapter", ono_value::Value::string(id)),
            )
        })?;
        let granted = policy.grants_capability(ono_kuang_protocol::Capability::ProcessExec);
        let mut listed = Vec::new();
        for pack in packs {
            let ids: Vec<String> = pack
                .adapters()
                .iter()
                .map(ono_adapter::Adapter::full_id)
                .collect();
            let held = if !granted {
                Some("process.exec was not granted (spec v0.3 §1.22)".to_owned())
            } else if pack.tier() == ono_adapter::Tier::Experimental && !options.allow_experimental
            {
                Some(
                    "the pack is experimental; `--allow-experimental` lets it answer \
                     (spec v0.3 §1.56)"
                        .to_owned(),
                )
            } else {
                None
            };
            match &held {
                Some(reason) => {
                    listed.push(format!("{} [disabled: {reason}]", ids.join(" ")));
                    session.adapters().add_disabled_pack(pack, reason);
                }
                None => {
                    listed.push(ids.join(" "));
                    session.adapters().add_pack(pack);
                }
            }
        }
        if package.manifest.runtime.is_none() {
            println!("loaded {id} (adapters): {}", listed.join("; "));
            return Ok(ExitStatus::SUCCESS);
        }
        println!("adapters of {id}: {}", listed.join("; "));
    }

    let entry = package
        .manifest
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.entry.as_ref())
        .map(|entry| package.directory.join(entry))
        .ok_or_else(|| {
            Flow::Failed(ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                format!("`{id}` declares no runtime to load"),
            ))
        })?;
    let mut config = LoadConfig::new(entry, package.manifest);
    config.policy = policy;
    let (runtime, _) = session.pipeline_context().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the runtime",
        ))
    })?;
    let loaded = runtime
        .block_on(Supervisor::load(config))
        .map_err(|error| {
            // The K11 code family folds into ono_core::ErrorCode in its own increment (ADR-0040
            // §3); until then the code travels in the message, never silently dropped.
            Flow::Failed(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("{error}"),
            ))
        })?;

    println!(
        "loaded {id} ({}): {}",
        loaded.state().as_str(),
        if loaded.commands().is_empty() {
            "no commands contributed".to_owned()
        } else {
            loaded
                .commands()
                .iter()
                .map(|command| command.contribution.id.clone())
                .collect::<Vec<_>>()
                .join(" ")
        }
    );
    session.add_plugin(id.to_owned(), loaded);
    Ok(ExitStatus::SUCCESS)
}

/// Whether `namespace` names a loaded package, by full id or by its last segment.
#[must_use]
pub fn loaded_package<'a>(session: &'a Session, namespace: &str) -> Option<&'a str> {
    session
        .plugin_ids()
        .find(|id| *id == namespace || id.rsplit('.').next() == Some(namespace))
}

/// Runs a contributed command: `<package>:<command> --name value …` (spec §31.22, ADR-0011).
///
/// # Errors
///
/// The package's own structured refusals, and `resolve.command_not_found` for a command the
/// package does not contribute.
pub fn invoke(
    session: &mut Session,
    namespace: &str,
    command: &str,
    words: &[std::ffi::OsString],
) -> Eval<Vec<Value>> {
    let Some(id) = loaded_package(session, namespace).map(str::to_owned) else {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                format!("no loaded package answers to `{namespace}:`"),
            )
            .with_help(
                "`load plugin` brings a package's commands in (spec §31.10); `get plugin` lists \
                 what is installed",
            ),
        ));
    };

    // `--name value` and `--name=value` become the JSON arguments of the plugin protocol.
    let mut arguments = serde_json::Map::new();
    let mut pending: Option<String> = None;
    for word in words {
        let text = word.to_string_lossy();
        if let Some(rest) = text.strip_prefix("--") {
            if let Some(name) = pending.take() {
                arguments.insert(name, serde_json::Value::Bool(true));
            }
            if let Some((name, value)) = rest.split_once('=') {
                arguments.insert(name.to_owned(), json_word(value));
            } else {
                pending = Some(rest.to_owned());
            }
        } else if let Some(name) = pending.take() {
            arguments.insert(name, json_word(&text));
        }
    }
    if let Some(name) = pending.take() {
        arguments.insert(name, serde_json::Value::Bool(true));
    }

    session.pipeline_context().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the runtime",
        ))
    })?;
    let outcome = {
        let runtime = session.runtime_handle().ok_or_else(|| {
            Flow::Failed(ErrorValue::new(
                ErrorCode::IoPermissionDenied,
                "the operating system refused to start the runtime",
            ))
        })?;
        let plugin = session.plugin(&id).ok_or_else(|| {
            Flow::Failed(ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                format!("`{id}` is no longer loaded"),
            ))
        })?;
        let full = contributed_id(plugin, command).ok_or_else(|| {
            Flow::Failed(
                ErrorValue::new(
                    ErrorCode::ResolveCommandNotFound,
                    format!("`{id}` contributes no command named `{command}`"),
                )
                .with_help("`get plugin` shows what a package contributes"),
            )
        })?;
        runtime.block_on(async {
            let invocation = plugin.invoke(&full, arguments).await.map_err(|error| {
                ErrorValue::new(ErrorCode::ProviderUnsupported, format!("{error}"))
            })?;
            Ok::<_, ErrorValue>(invocation.collect().await)
        })
    };
    let (events, _result) = outcome.map_err(Flow::Failed)?;

    let mut values = Vec::new();
    for event in events {
        match event {
            ono_kuang_supervisor::StreamEvent::Value(value) => values.push(value),
            ono_kuang_supervisor::StreamEvent::Failed(error) => {
                return Err(Flow::Failed(ErrorValue::new(
                    ErrorCode::ProviderUnsupported,
                    format!("{error}"),
                )));
            }
        }
    }
    Ok(values)
}

/// The full contributed id for a short command name, such as `emit`.
fn contributed_id(plugin: &LoadedPlugin, command: &str) -> Option<String> {
    plugin.commands().iter().find_map(|registered| {
        let id = &registered.contribution.id;
        (id == command || id.rsplit('.').next() == Some(command)).then(|| id.clone())
    })
}

fn json_word(text: &str) -> serde_json::Value {
    if let Ok(number) = text.parse::<i64>() {
        return serde_json::Value::Number(number.into());
    }
    if let Ok(flag) = text.parse::<bool>() {
        return serde_json::Value::Bool(flag);
    }
    serde_json::Value::String(text.to_owned())
}
