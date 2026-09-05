//! KUANG/11 at the shell boundary (spec §31): discovery, loading, and contributed commands.
//!
//! The libraries do the hard part — the supervisor negotiates before spawn, brokers every
//! capability call and audits every decision (ADR-0040, ADR-0041). This module is the shell's
//! integration step the supervisor deliberately leaves out: finding installed packages, keeping
//! loaded ones on the session, and routing a `<package>:command` head into an invocation.

use std::path::PathBuf;

use ono_command::CommandContract;
use ono_core::{ErrorCode, ExitStatus};
use ono_kuang_protocol::KuangError;
use ono_kuang_supervisor::{LoadConfig, LoadedPlugin, Supervisor};
use ono_value::{ErrorValue, Value};

pub use crate::kuang_host::Installed;

use crate::eval::{Eval, Flow};
use crate::session::Session;

/// The directories `ONO_PLUGIN_PATH` names, or the user's plugin directory.
#[must_use]
pub fn plugin_path(session: &Session) -> Vec<PathBuf> {
    if let Some(path) = session.env_var("ONO_PLUGIN_PATH") {
        return std::env::split_paths(path).collect();
    }
    session
        .home()
        .map(|home| home.join(".config/ono/plugins"))
        .into_iter()
        .collect()
}

/// Every package installed under the plugin path, in directory order, and the directories
/// whose manifest does not validate (ADR-0051).
pub fn installed(session: &mut Session) -> (Vec<Installed>, Vec<ErrorValue>) {
    session.publish_host();
    session.with_kuang(|host| host.installed())
}

/// A supervisor error as the shell reports it: the K11 code itself (ADR-0108), with the
/// message and help it carried.
#[must_use]
pub fn error_value(error: &KuangError) -> ErrorValue {
    let mut value = match ErrorCode::from_code(error.code().code()) {
        Some(code) => ErrorValue::new(code, error.message()),
        // A code this build's taxonomy does not carry stays visible in the message.
        None => ErrorValue::new(
            ErrorCode::ProviderUnsupported,
            format!("{}: {}", error.code().name(), error.message()),
        ),
    };
    if let Some(help) = error.help() {
        value = value.with_help(help);
    }
    // v0.4.1 §53.2 and §54.1: the detail is what a script branches on, and a refusal that names
    // the boundary that decided it has to name it somewhere other than the sentence. The
    // supervisor puts the control, the execution tier and the platform's own reason in the
    // metadata; dropping them here would leave string matching as the only way to read them.
    for (key, detail) in error.metadata() {
        let detail = match detail {
            serde_json::Value::String(text) => Value::string(text),
            serde_json::Value::Bool(flag) => Value::Bool(*flag),
            serde_json::Value::Number(number) => number
                .as_i64()
                .map_or(Value::Null, |number| Value::Int(i128::from(number))),
            // Nested detail is rendered as the text it is; the shell's error metadata is a flat
            // map of facts, not a second value tree.
            other => Value::string(&other.to_string()),
        };
        value = value.with_metadata(key, detail);
    }
    value
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
    /// Whether the load reports itself. `load plugin` is an operator asking for a load and is
    /// told what it got; the lazy load behind a contributed command (spec §31.68) is not, because
    /// the operator asked for the command and its answer is the only thing on stdout.
    pub silent: bool,
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
    session.publish_host();
    let Some(package) = session.with_kuang(|host| host.installed_package(id)) else {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("no installed package answers to `{id}`"),
            )
            .with_help("`get plugin` lists the installed set (spec §31.8)"),
        ));
    };

    // `load` is a transition from `enabled` (lifecycle.v1): a package the operator disabled
    // stays inert until `set plugin --enabled true`.
    let management = session.with_kuang(|host| host.management(id));
    if !management.enabled {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::SafetyPolicyDenied,
                format!("`{id}` is disabled and is not loaded"),
            )
            .with_help(format!(
                "`set plugin {id} --enabled true` makes it eligible again (spec §31.3)"
            )),
        ));
    }

    // Integrity, signature and publisher trust are re-checked against what is on disk now, not
    // against what was on disk at install: a file changed afterwards must not load
    // (`lifecycle.v1.yaml` verification rules, ADR-0312 §4). No package code has run yet.
    let resolved = crate::kuang_host::Resolved {
        source: crate::kuang_host::source_of(&package, &management),
        package: Ok(package.clone()),
    };
    if let Some(failure) = session
        .with_kuang(|host| host.verify(&resolved))
        .map_err(Flow::Failed)?
        .blocking
        .into_iter()
        .next()
    {
        return Err(Flow::Failed(failure));
    }

    // The policy is default-deny; what the user said on the command line is the only grant
    // (spec §31.18). A grant takes the scope the manifest asked for, never a wider one, and it
    // is recorded on the host so `get capability` and `revoke capability` see it.
    let mut granted = Vec::new();
    for word in &options.grants {
        let capability: ono_kuang_protocol::Capability = word.parse().map_err(|_| {
            Flow::Failed(
                ErrorValue::new(
                    ErrorCode::ResolveTargetNotFound,
                    format!("`{word}` is not a capability"),
                )
                .with_help("`help capabilities` lists them"),
            )
        })?;
        let request = declared_request(&package.manifest, capability);
        granted.push((
            capability,
            request.and_then(|(request, _)| request.scope.clone()),
            request.map(|(_, class)| class),
        ));
    }
    let policy = session.with_kuang(|host| {
        for (capability, scope, class) in granted {
            if !host
                .standing_grants(id)
                .any(|grant| grant.capability == capability)
            {
                host.grant(id, capability, scope, class, "session", "session", None);
            }
        }
        host.policy_for(id)
    });

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
            if !options.silent {
                println!("loaded {id} (adapters): {}", listed.join("; "));
            }
            return Ok(ExitStatus::SUCCESS);
        }
        if !options.silent {
            println!("adapters of {id}: {}", listed.join("; "));
        }
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
    // v0.2 §31.7's `<from>-><to>` shapes, read before the manifest is handed to the loader:
    // v0.4 §36.1 makes them the package's spatial contribution, and §35.5 makes holding the
    // capability the condition for any of it reaching a map.
    let shapes: Vec<String> = package
        .manifest
        .contributions
        .as_ref()
        .and_then(|contributions| contributions.relations.clone())
        .unwrap_or_default();
    let mut config = LoadConfig::new(entry, package.manifest);
    config.policy = policy;
    // The instance runs in its own directory under the state root, not in the user's (spec
    // §31.10, §31.31, ADR-0283).
    config.private_dir = session.with_kuang(|host| host.private_dir(id));
    // What `models.list` and `models.infer` reach: the operator's catalogue (ADR-0566).
    config.models = session.with_kuang(|host| host.model_broker());
    // What the object, relation, history, process and secret domains reach: the session's
    // providers as they stand now, and its history file (ADR-0568).
    let history =
        crate::config::state_dir(session).map(|directory| directory.join("history.jsonl"));
    let registry = session.providers().clone();
    config.host = std::sync::Arc::new(crate::kuang_services::ShellHost::new(registry, history));
    // What `context.get` answers with: the session's published context (ADR-0567).
    config.context = std::sync::Arc::new(crate::kuang_host::SharedContext(std::sync::Arc::clone(
        session.tables(),
    )));
    config.views = std::sync::Arc::new(crate::kuang_views::ShellViews::new(std::sync::Arc::clone(
        session.theme(),
    )));
    let (runtime, _) = session.pipeline_context().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the runtime",
        ))
    })?;
    let loaded = runtime
        .block_on(Supervisor::load(config))
        .map_err(|error| Flow::Failed(error_value(&error)))?;

    if !options.silent {
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
    }
    session.with_kuang(|host| host.record_host_event(id, "plugin.load", "lifecycle.load", true));
    // Re-loading replaces (spec §31.72, ADR-0110 §3): the instance that was running is shut
    // down once the new one is kept, so a reload is never a moment without the package.
    if let Some(previous) = session.add_plugin(id.to_owned(), loaded)
        && let Some(runtime) = session.runtime_handle()
    {
        runtime.block_on(
            previous
                .plugin
                .shutdown(ono_kuang_protocol::ShutdownReason::Upgrade),
        );
    }
    // §35.5: the capability filter runs before the merge, and this is before. A package denied
    // `relation.write` contributes no relation at all, so no map has one of its edges to drop.
    crate::spatial::contributions::forget(id);
    if let Some(plugin) = session.plugin(id) {
        crate::spatial::contributions::adopt(id, &plugin, &shapes);
    }
    Ok(ExitStatus::SUCCESS)
}

/// A KUANG/11 management command the evaluator runs itself, seeding the rest of the pipeline
/// with what it produced (ADR-0108 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// `verify plugin <id | reference>` (spec §31.36).
    VerifyPlugin,
    /// `install plugin <reference> [--confirm]` (spec §31.9).
    InstallPlugin,
    /// `grant capability <capability> --plugin <id>` (spec §31.18).
    GrantCapability,
    /// `ask assistant <id> <request>` (spec §31.42).
    AskAssistant,
}

/// What a management command produced: the values for the pipeline after it, and the failure
/// that makes the run fail once they are shown.
#[derive(Debug)]
pub struct Produced {
    /// The records the rest of the pipeline is seeded with.
    pub values: Vec<Value>,
    /// A failure to report after the values, which fails the run.
    pub failure: Option<ErrorValue>,
}

/// Whether `stage` is a management command this module runs (spec §31.3's `<verb> plugin`).
#[must_use]
pub fn claims(stage: &ono_parser::Stage) -> Option<Request> {
    let ono_parser::StageHead::Command(name) = &stage.head else {
        return None;
    };
    if name.namespace.is_some() {
        return None;
    }
    let target = stage
        .arguments
        .first()
        .and_then(ono_parser::Argument::as_word)?;
    match (name.name.as_str(), target) {
        ("verify", "plugin") => Some(Request::VerifyPlugin),
        ("install", "plugin") => Some(Request::InstallPlugin),
        ("grant", "capability") => Some(Request::GrantCapability),
        ("ask", "assistant") => Some(Request::AskAssistant),
        _ => None,
    }
}

/// The piped form, `get plugin | verify plugin` (ADR-0118): the packages arrive as
/// `ono.plugin/1` records and each is handled as `<verb> plugin <id>` would be. `ask assistant`
/// declares `input: null | any`, so whatever arrives is its context and the assistant is still
/// named on the stage (spec §7.1); `install plugin` and `grant capability` declare no input.
///
/// # Errors
///
/// The structured refusal of the command, or a type error for a command with no stream input
/// or for a piped value that is not an `ono.plugin/1` record.
pub fn run_piped(
    session: &mut Session,
    request: Request,
    words: &[String],
    targets: &[Value],
) -> Eval<Produced> {
    match request {
        Request::VerifyPlugin => {
            let ids = crate::remote::piped_names("verify plugin", "ono.plugin", "id", targets)
                .map_err(Flow::Failed)?;
            let mut produced = Produced {
                values: Vec::new(),
                failure: None,
            };
            for id in ids {
                let one = run(session, request, &["plugin".to_owned(), id])?;
                produced.values.extend(one.values);
                produced.failure = produced.failure.or(one.failure);
            }
            Ok(produced)
        }
        Request::AskAssistant => run(session, request, words),
        Request::InstallPlugin => Err(Flow::Failed(crate::remote::no_stream_input(
            "install plugin",
            "plugin",
        ))),
        Request::GrantCapability => Err(Flow::Failed(crate::remote::no_stream_input(
            "grant capability",
            "capability",
        ))),
    }
}

/// The piped form of `load plugin` (ADR-0118): `get plugin | load plugin [--grant …]` loads
/// every package that arrived, with the stage's options applying to each.
///
/// # Errors
///
/// The first load that is refused, after the packages before it were loaded; a type error when
/// the stage also names a package, or a piped value is not an `ono.plugin/1` record.
pub fn load_piped(session: &mut Session, words: &[String], targets: &[Value]) -> Eval<ExitStatus> {
    let (named, options) = LoadOptions::from_words(words);
    if let Some(named) = named {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!(
                    "`load plugin` takes its packages from the pipe or by name, not both (`{named}`)"
                ),
            )
            .with_help("`get plugin | load plugin` or `load plugin <id>` (spec §31.8)"),
        ));
    }
    let ids = crate::remote::piped_names("load plugin", "ono.plugin", "id", targets)
        .map_err(Flow::Failed)?;
    let mut status = ExitStatus::SUCCESS;
    for id in ids {
        status = load_plugin_with(session, &id, &options)?;
    }
    Ok(status)
}

/// The autonomy levels of spec §31.48, as code and name.
///
/// The list is deliberately closed and deliberately stops at `L4`: §31.48 rules out an
/// unrestricted "root autonomous" level in the normal product model.
const AUTONOMY_LEVELS: &[(&str, &str)] = &[
    ("L0", "explain-only"),
    ("L1", "observe"),
    ("L2", "propose"),
    ("L3", "act-confirmed"),
    ("L4", "delegated-scope"),
];

/// Runs a management command over its words (the target word included).
///
/// # Errors
///
/// The structured refusal of the command.
pub fn run(session: &mut Session, request: Request, words: &[String]) -> Eval<Produced> {
    let arguments: Vec<&str> = words
        .iter()
        .skip(1)
        .map(String::as_str)
        .filter(|word| !word.starts_with("--"))
        .collect();
    let flag = |name: &str| words.iter().any(|word| word == name);
    let option = |name: &str| {
        words
            .iter()
            .position(|word| word == name)
            .and_then(|index| words.get(index + 1))
            .map(String::as_str)
    };
    match request {
        // Spec §7.1: the assistant is selected explicitly, and no assistant package is loaded
        // in this build, so whatever was named is not found (ADR-0111 §3).
        Request::AskAssistant => {
            // Spec §31.48: a package may declare which autonomy modes it supports, but the
            // levels themselves are Ono's, and there is to be no unrestricted one. A word
            // outside the vocabulary is refused here rather than carried into a turn, because a
            // policy nothing can enforce is exactly the invisible unlimited delegation §31.48
            // rules out (ADR-0233).
            if let Some(level) = option("--autonomy")
                && !AUTONOMY_LEVELS
                    .iter()
                    .any(|(code, name)| level.eq_ignore_ascii_case(code) || *name == level)
            {
                return Err(Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        format!("`{level}` is not an autonomy level this shell defines"),
                    )
                    .with_help(format!(
                        "spec §31.48 gives {}; Ono controls the policy, whatever a package \
                         supports",
                        AUTONOMY_LEVELS
                            .iter()
                            .map(|(code, name)| format!("{code} {name}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                ));
            }
            let Some(assistant) = arguments.first() else {
                return Err(Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::ResolveTargetNotFound,
                        "`ask assistant` needs the assistant to ask",
                    )
                    .with_help("`get assistant` lists the loaded assistants (spec §31.42)"),
                ));
            };
            Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::ResolveTargetNotFound,
                    format!("no loaded assistant answers to `{assistant}`"),
                )
                .with_help("`get assistant` lists the loaded assistants (spec §31.42)"),
            ))
        }
        Request::GrantCapability => {
            let Some(capability) = arguments.first() else {
                return Err(Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::ResolveTargetNotFound,
                        "`grant capability` needs the capability to grant",
                    )
                    .with_help("`get capability` lists them (spec §31.16)"),
                ));
            };
            let Some(plugin) = option("--plugin") else {
                return Err(Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::ResolveTargetNotFound,
                        "`grant capability` needs `--plugin <id>`: a grant is made to one package",
                    )
                    .with_help("spec §31.18: never to a publisher or a class of packages"),
                ));
            };
            // `--scope` is repeatable: one key per occurrence, so a grant can bound two keys of
            // the same capability without a nested literal on the command line.
            let scopes: Vec<&str> = words
                .iter()
                .enumerate()
                .filter(|(_, word)| *word == "--scope")
                .filter_map(|(index, _)| words.get(index + 1))
                .map(String::as_str)
                .collect();
            grant_capability(session, capability, plugin, &scopes, option("--duration"))
        }
        Request::InstallPlugin => {
            let Some(reference) = arguments.first() else {
                return Err(Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::ResolveTargetNotFound,
                        "`install plugin` needs the package reference to install",
                    )
                    .with_help("`path:<directory>` (spec §31.9)"),
                ));
            };
            install_plugin(session, reference, flag("--confirm"))
        }
        Request::VerifyPlugin => {
            let Some(reference) = arguments.first() else {
                return Err(Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::ResolveTargetNotFound,
                        "`verify plugin` needs the package to verify",
                    )
                    .with_help("an installed id, or `path:<directory>` (spec §31.36)"),
                ));
            };
            session.publish_host();
            let verification = session
                .with_kuang(|host| {
                    host.resolve(reference)
                        .and_then(|resolved| host.verify(&resolved))
                })
                .map_err(Flow::Failed)?;
            Ok(Produced {
                values: vec![verification.record.into_value()],
                failure: verification.blocking.into_iter().next(),
            })
        }
    }
}

/// Runs `install plugin <reference>`: verify, plan, confirm, place (spec §31.9).
fn install_plugin(session: &mut Session, reference: &str, confirmed: bool) -> Eval<Produced> {
    use crate::kuang_host::{action, action_result, install_plan};
    let started = std::time::Instant::now();
    session.publish_host();
    let resolved = session
        .with_kuang(|host| host.resolve(reference))
        .map_err(Flow::Failed)?;
    // Verification comes first: a blocking check never produces a prompt offering to continue
    // (lifecycle.v1, ADR-0015 rule 4).
    let verification = session
        .with_kuang(|host| host.verify(&resolved))
        .map_err(Flow::Failed)?;
    if let Some(failure) = verification.blocking.into_iter().next() {
        return Err(Flow::Failed(failure));
    }
    let package = resolved.package.map_err(Flow::Failed)?;
    let id = package.manifest.package.id.clone();
    let version = package.manifest.package.version.clone();
    let (destination, already) = session
        .with_kuang(|host| {
            host.install_destination(&package)
                .map(|destination| (destination, host.is_installed(&id, &version)))
        })
        .map_err(Flow::Failed)?;
    if already {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::IoAlreadyExists,
                format!("`{id}` {version} is already installed"),
            )
            .with_help(
                "`remove plugin` it first; a package version is never silently replaced \
                 (spec §31.35)",
            ),
        ));
    }

    // The plan comes before any mutation (spec §31.9), and a script never waits for its
    // prompt (spec §17.4): without `--confirm` the answer is a refusal that carries the plan.
    let plan = install_plan(&package, &resolved.source, &destination);
    if !confirmed {
        if session.is_interactive() && !prompt_for_install(&plan) {
            return Err(Flow::Failed(ErrorValue::new(
                ErrorCode::SafetyConfirmationRequired,
                format!("installing `{id}` was not confirmed"),
            )));
        }
        if !session.is_interactive() {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::SafetyConfirmationRequired,
                    format!(
                        "installing `{id}` {version} from {} needs the install plan confirmed",
                        resolved.source
                    ),
                )
                .with_help(
                    "nothing was written. Write `--confirm` to accept the plan \
                     non-interactively (spec §17.4, §31.9)",
                )
                .with_metadata("plan", plan),
            ));
        }
    }

    let outcome = session.with_kuang(|host| {
        let action = action("install", &id, &version);
        match host.install(&package, &resolved.source) {
            Ok(_) => ono_provider_api::ActionOutcome::succeeded(&action, true),
            Err(error) => ono_provider_api::ActionOutcome::failed(&action, error),
        }
    });
    let failed = !outcome.is_success();
    let failure = if failed {
        outcome.error().cloned()
    } else {
        None
    };
    Ok(Produced {
        values: vec![action_result(outcome, "ono.plugin.install", started)],
        failure,
    })
}

/// Runs `grant capability <capability> --plugin <id>` (spec §31.18): a standing session grant,
/// in the scope the manifest asked for, effective at the package's next call.
/// The grant durations this broker can hold itself to (spec §31.18).
///
/// §31.18 lists six durations a grant SHOULD support. Three of them — `once`, `this command` and
/// `this view` — are bounded by an event the broker cannot observe from here: nothing tells the
/// host that a use, a command or a view has ended, so a grant minted with one of those words
/// would behave exactly like a session grant while claiming to be narrower. That is the defect
/// this option exists to remove, so those words are refused by name rather than recorded
/// (ADR-0264). `link-session` waits on the same thing for a link.
const ENFORCEABLE_DURATIONS: &[&str] = &["session", "always"];

/// Reads `--duration`: a §31.18 word, or a span that makes the grant a lease (spec §31.49).
///
/// Answers the `duration` word the record carries and the instant it stops working.
fn grant_duration(
    written: Option<&str>,
    granted_at: jiff::Timestamp,
) -> Result<(&'static str, Option<jiff::Timestamp>), ErrorValue> {
    let Some(written) = written else {
        return Ok(("session", None));
    };
    if let Some(word) = ENFORCEABLE_DURATIONS.iter().find(|word| **word == written) {
        return Ok((word, None));
    }
    let span = ono_value::Duration::parse(written).map_err(|_| unenforceable_duration(written))?;
    if span.is_negative() || span == ono_value::Duration::ZERO {
        return Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("`{written}` is not a window a grant can stand in"),
        )
        .with_help("a lease expires after its span, so the span must be positive (spec §31.49)"));
    }
    let nanos = i64::try_from(span.nanoseconds()).map_err(|_| unenforceable_duration(written))?;
    let expires_at = granted_at
        .checked_add(jiff::Span::new().nanoseconds(nanos))
        .map_err(|_| unenforceable_duration(written))?;
    // A lease lives inside the session like every duration but `always`, and `expires_at` is
    // what makes it narrower — the record shows both (`capability-grant.v1`).
    Ok(("session", Some(expires_at)))
}

fn unenforceable_duration(written: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!("`{written}` is not a duration `grant capability` can hold itself to"),
    )
    .with_help(format!(
        "`--duration {}`, or a span such as `1h` for a lease that expires (spec §31.18, §31.49).          `once`, `command`, `view` and `link-session` need a boundary the broker cannot yet          observe and are refused rather than recorded",
        ENFORCEABLE_DURATIONS.join("`/`")
    ))
}

/// Reads the repeated `--scope key=value[,value]` words into the grant's scope (spec §31.16).
///
/// A key the capability does not declare is invalid rather than ignored, and a capability that
/// declares no scope at all cannot be scoped: §31.16 forbids offering a scope that cannot be
/// enforced as if it were a security boundary.
fn grant_scope(
    capability: ono_kuang_protocol::Capability,
    written: &[&str],
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, ErrorValue> {
    if written.is_empty() {
        return Ok(None);
    }
    let declared = capability.scope_keys();
    if declared.is_empty() {
        return Err(ErrorValue::new(
            ErrorCode::TypeUnknownField,
            format!("`{}` declares no scope keys", capability.id()),
        )
        .with_help(
            "spec §31.16: a scope that cannot be enforced is never offered — grant it unscoped",
        ));
    }
    let mut scope = serde_json::Map::new();
    for entry in written {
        let Some((key, values)) = entry.split_once('=') else {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`--scope {entry}` names no key"),
            )
            .with_help("`--scope <key>=<value>[,<value>]`, repeated once per key (spec §31.16)"));
        };
        if !declared.iter().any(|declared| declared.name == key) {
            return Err(ErrorValue::new(
                ErrorCode::TypeUnknownField,
                format!("`{}` has no scope key `{key}`", capability.id()),
            )
            .with_help(format!(
                "spec §31.16: `{}` declares {}",
                capability.id(),
                declared
                    .iter()
                    .map(|declared| declared.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        scope.insert(
            key.to_owned(),
            serde_json::Value::Array(
                values
                    .split(',')
                    .filter(|value| !value.is_empty())
                    .map(|value| serde_json::Value::String(value.to_owned()))
                    .collect(),
            ),
        );
    }
    Ok(Some(scope))
}

fn grant_capability(
    session: &mut Session,
    capability: &str,
    plugin: &str,
    scopes: &[&str],
    duration: Option<&str>,
) -> Eval<Produced> {
    let Some(capability) = ono_kuang_protocol::Capability::from_id(capability) else {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("`{capability}` is not a capability the broker knows"),
            )
            .with_help("`get capability` lists `kuang_capabilities` of docs/contracts/capabilities.yaml (spec §31.16)"),
        ));
    };
    session.publish_host();
    let Some(package) = session.with_kuang(|host| host.installed_package(plugin)) else {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("no installed package answers to `{plugin}`"),
            )
            .with_help("`get plugin` lists the installed set (spec §31.8)"),
        ));
    };
    let request = declared_request(&package.manifest, capability);
    // §31.19's precedence is "system deny > user deny > scoped grant > plugin request": the
    // operator's scope outranks the manifest's, key by key. Keys the operator did not name keep
    // whatever the package asked for.
    let named = grant_scope(capability, scopes).map_err(Flow::Failed)?;
    let mut scope = request.and_then(|(request, _)| request.scope.clone());
    if let Some(named) = named {
        let merged = scope.get_or_insert_with(serde_json::Map::new);
        for (key, value) in named {
            merged.insert(key, value);
        }
    }
    let granted_at = jiff::Timestamp::now();
    let (duration, expires_at) = grant_duration(duration, granted_at).map_err(Flow::Failed)?;
    let purpose = request.and_then(|(request, _)| request.purpose.clone());
    let class = request.map(|(_, class)| class);
    let (grant, value, policy, instance) = session.with_kuang(|host| {
        let grant = host.grant(
            plugin, capability, scope, class, "prompt", duration, expires_at,
        );
        let value =
            crate::kuang_host::grant_value(&grant, purpose.as_deref(), host.instance(plugin));
        (grant, value, host.policy_for(plugin), host.plugin(plugin))
    });
    let _ = grant;
    let value = value.map_err(Flow::Failed)?;
    if let Some(instance) = instance
        && let Some(runtime) = session.runtime_handle()
    {
        runtime.block_on(instance.update_policy(policy));
    }
    Ok(Produced {
        values: vec![value],
        failure: None,
    })
}

/// How `manifest` declared `capability`, if it did.
fn declared_request(
    manifest: &ono_kuang_protocol::Manifest,
    capability: ono_kuang_protocol::Capability,
) -> Option<(
    &ono_kuang_protocol::CapabilityRequest,
    ono_kuang_protocol::DeclarationClass,
)> {
    use ono_kuang_protocol::DeclarationClass;
    manifest
        .required_capabilities
        .iter()
        .map(|request| (request, DeclarationClass::Required))
        .chain(
            manifest
                .optional_capabilities
                .iter()
                .map(|request| (request, DeclarationClass::Optional)),
        )
        .chain(
            manifest
                .runtime_requested_capabilities
                .iter()
                .map(|request| (request, DeclarationClass::RuntimeRequested)),
        )
        .find(|(request, _)| request.capability == capability)
}

/// Shows the install plan and asks (spec §31.9's `proceed? [y/N]`); only an explicit yes is one.
fn prompt_for_install(plan: &Value) -> bool {
    let rendered = ono_value::to_yaml_data(plan).unwrap_or_default();
    println!("INSTALL PLAN\n{rendered}");
    print!("proceed? [y/N] ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

/// Whether `namespace` names a loaded package, by full id or by its last segment.
#[must_use]
pub fn loaded_package(session: &Session, namespace: &str) -> Option<String> {
    session
        .plugin_ids()
        .into_iter()
        .find(|id| id == namespace || id.rsplit('.').next() == Some(namespace))
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
    let Some(id) = loaded_package(session, namespace) else {
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

    let arguments = json_arguments(words);

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
        let full = contributed_id(&plugin, command).ok_or_else(|| {
            Flow::Failed(
                ErrorValue::new(
                    ErrorCode::ResolveCommandNotFound,
                    format!("`{id}` contributes no command named `{command}`"),
                )
                .with_help("`get plugin` shows what a package contributes"),
            )
        })?;
        runtime.block_on(async {
            let invocation = plugin
                .invoke(&full, arguments)
                .await
                .map_err(|error| crate::kuang_host::wire_error_value(&error))?;
            Ok::<_, ErrorValue>(invocation.collect().await)
        })
    };
    let (events, _result) = outcome.map_err(Flow::Failed)?;

    let mut values = Vec::new();
    for event in events {
        match event {
            ono_kuang_supervisor::StreamEvent::Value(value) => values.push(value),
            ono_kuang_supervisor::StreamEvent::Failed(error) => {
                return Err(Flow::Failed(crate::kuang_host::wire_error_value(&error)));
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

/// `--name value` and `--name=value` as the JSON arguments of the plugin protocol.
///
/// Shared by [`invoke`] and [`query`] because a command and a target are spelled the same way at
/// a prompt. They were not shared at first, and the target route passed an empty map: every
/// contributed target answered as though it had been asked with no arguments, which is not a
/// visible failure but a permanently unfiltered one.
fn json_arguments(words: &[std::ffi::OsString]) -> serde_json::Map<String, serde_json::Value> {
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
    arguments
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

// --- lazy invocation of a declared contribution (spec §31.68, ADR-0282) -----------------------

/// The registry entry a bare stage head names, when it names a contributed command.
///
/// `None` for every core command and for a head that is not a registry entry at all, so the
/// evaluator's ordinary path is unchanged for everything Ono ships.
#[must_use]
pub fn contributed_command(stage: &ono_parser::Stage) -> Option<&'static CommandContract> {
    let ono_parser::StageHead::Command(name) = &stage.head else {
        return None;
    };
    if name.namespace.is_some() {
        return None;
    }
    let registry = crate::eval::native::registry().ok()?;
    let target = stage
        .arguments
        .first()
        .and_then(ono_parser::Argument::as_word);
    let contract = registry
        .find(&name.name, target)
        .or_else(|| registry.find(&name.name, None))?;
    (!contract.origin().is_core()).then_some(contract)
}

/// The registry entry `<package>:<command>` names, when the package declared it (spec §31.66).
#[must_use]
pub fn contributed_by_namespace(
    namespace: &str,
    command: &str,
) -> Option<&'static CommandContract> {
    let registry = crate::eval::native::registry().ok()?;
    registry.commands().iter().find(|contract| {
        let Some(package) = contract.origin().package() else {
            return false;
        };
        let matches_package = package == namespace || package.rsplit('.').next() == Some(namespace);
        let short = contract.id().rsplit('.').next().unwrap_or_default();
        matches_package && (short == command || contract.id() == command)
    })
}

/// Runs a contributed command by its registry entry, loading its package first if the shell has
/// not loaded it yet (spec §31.68).
///
/// The declaration in the manifest is what let the command be resolved without any of the
/// package's code running; invoking it is the moment that changes, and it changes with the same
/// policy negotiation `load plugin` performs.
///
/// # Errors
///
/// The load's own refusal — a denied required capability, a disabled package, a runtime that
/// will not start — or the package's refusal of the invocation.
pub fn invoke_contributed(
    session: &mut Session,
    contract: &CommandContract,
    words: &[std::ffi::OsString],
) -> Eval<Vec<Value>> {
    let package = contract.origin().package().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            format!("`{}` names no package to run it", contract.spelling()),
        ))
    })?;
    if loaded_package(session, package).is_none() {
        load_plugin_with(
            session,
            package,
            &LoadOptions {
                silent: true,
                ..LoadOptions::default()
            },
        )?;
    }
    // Two kinds of contributed entry, routed differently, and the id says which (spec §31.23).
    // A command is invoked and answers whatever it declared; a target is *queried* through the
    // provider path, so that its records carry the schema the target declared and the provenance
    // the host stamps rather than whatever a command chose to emit.
    if let Some(target) = contract
        .id()
        .strip_prefix(package)
        .and_then(|rest| rest.strip_prefix(crate::plugin_registry::TARGET_INFIX))
    {
        let target = target.to_owned();
        return query(session, package, &target, words);
    }
    let command = contract.id().rsplit('.').next().unwrap_or(contract.id());
    invoke(session, package, command, words)
}

/// Answers a contributed target by querying the package that contributes it (spec §31.23).
///
/// The provider half of [`invoke`]. It reaches `provider.query` rather than `command.invoke`, so
/// the records that come back are the ones the package's provider handler emitted — validated
/// against the contributed schema and provenance-stamped `plugin:<package.id>` by the host
/// (spec §31.80), which is what makes a contributed target a noun rather than a command wearing
/// a target's spelling.
pub fn query(
    session: &mut Session,
    package: &str,
    target: &str,
    words: &[std::ffi::OsString],
) -> Eval<Vec<Value>> {
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
        let plugin = session.plugin(package).ok_or_else(|| {
            Flow::Failed(ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                format!("`{package}` is no longer loaded"),
            ))
        })?;
        if !plugin
            .targets()
            .iter()
            .any(|registered| registered.contribution.name == target)
        {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::ResolveTargetNotFound,
                    format!("`{package}` contributes no target named `{target}`"),
                )
                .with_help(
                    "the package declared it on disk but does not answer for it; the two must \
                     agree (spec §31.23)",
                ),
            ));
        }
        runtime.block_on(async {
            let invocation = plugin
                .query(target, json_arguments(words))
                .await
                .map_err(|error| crate::kuang_host::wire_error_value(&error))?;
            Ok::<_, ErrorValue>(invocation.collect().await)
        })
    };
    let (events, _result) = outcome.map_err(Flow::Failed)?;

    let mut values = Vec::new();
    for event in events {
        match event {
            ono_kuang_supervisor::StreamEvent::Value(value) => values.push(value),
            ono_kuang_supervisor::StreamEvent::Failed(error) => {
                return Err(Flow::Failed(crate::kuang_host::wire_error_value(&error)));
            }
        }
    }
    Ok(values)
}
