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
    ///
    /// A silent load is that lazy load, so it loads only a package nobody has loaded by the time
    /// it holds the package's turn: a job and the shell that both reach an unloaded package start
    /// it once, and the one that waited runs in the instance the other kept (ADR-1026).
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
    // A short name resolves to the one installed package that carries it (K11P §10.4).
    let package = session
        .with_kuang(|host| host.resolve_installed(id))
        .map_err(Flow::Failed)?;
    let canonical = package.manifest.package.id.clone();
    let id = canonical.as_str();

    // One load of a package at a time, whichever evaluator asks: the shell and its jobs share the
    // host (ADR-0995), and two lazy loads racing each other kept the second instance and shut the
    // first down under the invocation running in it (ADR-1026).
    let turn = session.with_kuang(|host| host.load_turn(id));
    let _turn = await_turn(&turn)?;
    if options.silent && session.plugin(id).is_some() {
        return Ok(ExitStatus::SUCCESS);
    }

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
                host.grant(
                    id,
                    capability,
                    scope,
                    class,
                    "session",
                    "session",
                    None,
                    crate::kuang_host::GrantOrigin::default(),
                );
            }
        }
        // A bounded extension-local permission is included whichever way the package arrived
        // (K11P §7.1, ADR-0600 §2): a package placed by hand gets its automatic grants here.
        crate::kuang_permissions::ensure_automatic(host, &package);
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
    // Both halves of a shape are on disk here — the shape in the manifest, the schema ids in the
    // `contributions.targets` documents §31.68 reads without running anything — so a shape naming
    // a kind of place nobody contributes is refused before the runtime is spawned, rather than
    // being discovered when somebody types `follow` (ADR-0585).
    let declared_schemas = crate::plugin_registry::declared_target_schemas(&package);
    let declared_names = crate::plugin_registry::declared_target_names(&package);
    crate::spatial::contributions::check_shapes(id, &shapes, &declared_schemas)
        .map_err(Flow::Failed)?;
    // A target's declared parent is settled the same way, and before the runtime is spawned: the
    // parent is a schema on disk, and the shape that will carry the edge is in the manifest
    // beside it (ADR-0597).
    crate::spatial::contributions::check_parents(
        id,
        &crate::plugin_registry::declared_target_parents(&package),
        &shapes,
        &declared_schemas,
    )
    .map_err(Flow::Failed)?;
    // The instance is shared by the foreground and every job (ADR-0995), so whoever loads it
    // decides nothing about who may be asked: its own source asks nobody, and an invocation
    // carries the source of the evaluator that runs it (ADR-1023).
    let consent = std::sync::Arc::new(crate::kuang_permissions::ShellConsent::new(
        std::sync::Arc::clone(session.tables()),
        false,
        package.clone(),
    ));
    let mut config = LoadConfig::new(entry, package.manifest);
    config.policy = policy;
    // The stores the session's environment names, where `kuang-compile` typed in it writes
    // (ADR-0917).
    config.compiled = session.with_kuang(|host| host.compiled_stores());
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
    // Who answers a just-in-time request no invocation's own source answers: nobody
    // (K11P §14, §20.3, ADR-0603 §2, ADR-1023).
    config.consent = consent;
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
        // A target the handshake contributes and the document does not declare answers through
        // the provider path and is not a word: the registry's words come from disk (spec
        // §31.68), so it has no `get`, no help and no completion. Said here rather than
        // accepted silently (ADR-0598).
        let unspellable: Vec<&str> = loaded
            .targets()
            .iter()
            .map(|target| target.contribution.name.as_str())
            .filter(|name| !declared_names.iter().any(|declared| declared == name))
            .collect();
        if !unspellable.is_empty() {
            println!(
                "  answerable, not spellable: {} (declared at handshake; not in \
                 contributions.targets, so no `get` word)",
                unspellable.join(" ")
            );
        }
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
    // Mounting the package puts its targets into the provider registry and registers the kinds
    // of place they answer with (ADR-0583, ADR-0584) — and a shape naming one of those kinds can
    // only be resolved once they exist. This is the ordering ADR-0584 named, settled by doing the
    // two things in the order they depend on: the handshake has happened, the types are
    // registered, and only then are the shapes read from the manifest adopted against them. The
    // shapes were *checked* before any of it, against the declarations on disk.
    let _ = session.providers();
    // §35.5: the capability filter runs before the merge, and this is before. A package denied
    // `relation.write` contributes no relation at all, so no map has one of its edges to drop.
    crate::spatial::contributions::forget(id);
    if let Some(plugin) = session.plugin(id) {
        crate::spatial::contributions::adopt(id, &package.directory, &plugin, &shapes);
    }
    Ok(ExitStatus::SUCCESS)
}

/// Waits for the turn to load a package, for as long as the load holding it takes — which the
/// supervisor's handshake deadline bounds — and no longer than the line's own interrupt allows.
fn await_turn(turn: &std::sync::Mutex<()>) -> Eval<std::sync::MutexGuard<'_, ()>> {
    loop {
        match turn.try_lock() {
            Ok(held) => return Ok(held),
            // A load that panicked left nothing half-kept in the turn itself; the host's tables
            // are what it guards the order of.
            Err(std::sync::TryLockError::Poisoned(poisoned)) => return Ok(poisoned.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => {
                if crate::eval::interrupt_reached() {
                    return Err(crate::eval::interrupted_flow_now());
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }
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
    /// `set permission <plugin> …` (K11P §16.2).
    SetPermission,
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
        ("set", "permission") => Some(Request::SetPermission),
        ("ask", "assistant") => Some(Request::AskAssistant),
        _ => None,
    }
}

/// The piped form, `get plugin | verify plugin` (ADR-0118): the packages arrive as
/// `ono.plugin/2` records and each is handled as `<verb> plugin <id>` would be. `ask assistant`
/// declares `input: null | any`, so whatever arrives is its context and the assistant is still
/// named on the stage (spec §7.1); `install plugin` and `grant capability` declare no input.
///
/// # Errors
///
/// The structured refusal of the command, or a type error for a command with no stream input
/// or for a piped value that is not an `ono.plugin/2` record.
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
        Request::SetPermission => Err(Flow::Failed(crate::remote::no_stream_input(
            "set permission",
            "permission",
        ))),
    }
}

/// The piped form of `load plugin` (ADR-0118): `get plugin | load plugin [--grant …]` loads
/// every package that arrived, with the stage's options applying to each.
///
/// # Errors
///
/// The first load that is refused, after the packages before it were loaded; a type error when
/// the stage also names a package, or a piped value is not an `ono.plugin/2` record.
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
                        ErrorCode::PluginNotFound,
                        "`install plugin` needs the package to install",
                    )
                    .with_help(
                        "a short name a catalog lists, a canonical id, or `path:<directory>` \
                         (K11P §4.1)",
                    ),
                ));
            };
            let reference = anchored_reference(session, reference);
            let source = option("--source").map(|source| anchored_reference(session, source));
            crate::kuang_install::install(
                session,
                &reference,
                &crate::kuang_install::InstallOptions {
                    access: option("--access").map(str::to_owned),
                    confirm: flag("--confirm"),
                    source,
                },
            )
        }
        Request::SetPermission => crate::kuang_permissions::set_permission(
            session,
            &crate::kuang_permissions::SetPermission::from_words(words),
        ),
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
            let reference = anchored_reference(session, reference);
            let verification = session
                .with_kuang(|host| {
                    host.resolve(&reference)
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
    let package = session
        .with_kuang(|host| host.resolve_installed(plugin))
        .map_err(Flow::Failed)?;
    let canonical = package.manifest.package.id.clone();
    let plugin = canonical.as_str();
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
            plugin,
            capability,
            scope,
            class,
            "prompt",
            duration,
            expires_at,
            crate::kuang_host::GrantOrigin::default(),
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

/// Whether `namespace` names a loaded package, by full id or by its last segment.
#[must_use]
pub fn loaded_package(session: &Session, namespace: &str) -> Option<String> {
    session
        .plugin_ids()
        .into_iter()
        .find(|id| id == namespace || id.rsplit('.').next() == Some(namespace))
}

/// The values an invocation delivered, or the failure it ended with.
///
/// A handler can fail *without emitting anything* — no cluster reachable, no credential, no route
/// — and the host learns of that from the invocation's result rather than from a stream event.
/// Reading only the stream, such a refusal is indistinguishable from "there are none", which is
/// the confusion spec §35.3 and the provider truth model exist to prevent. So the result is read,
/// and a failed invocation is a failure even when the stream was empty.
fn delivered(
    events: Vec<ono_kuang_supervisor::StreamEvent>,
    result: &ono_kuang_protocol::InvokeResult,
) -> Eval<Vec<Value>> {
    let mut values = Vec::new();
    for event in events {
        match event {
            ono_kuang_supervisor::StreamEvent::Value(value) => values.push(value),
            ono_kuang_supervisor::StreamEvent::Failed(error) => {
                return Err(Flow::Failed(crate::kuang_host::wire_error_value(&error)));
            }
        }
    }
    if result.status == ono_kuang_protocol::InvokeStatus::Failed {
        let error = result.error.clone().unwrap_or_else(|| {
            ono_kuang_protocol::WireError::from_core(
                ErrorCode::ProviderUnavailable,
                "the package reported the invocation failed and named no reason",
            )
        });
        return Err(Flow::Failed(crate::kuang_host::wire_error_value(&error)));
    }
    Ok(values)
}

/// Reads an invocation to its end, or cancels it when the line running it is interrupted first.
///
/// The line's interrupt is the evaluator's: Ctrl-C in the foreground, and in a background job
/// the job's own cancellation — `kill %N`, Ctrl-C under `fg`, a leaving shell (ADR-0952,
/// ADR-0959). A job runs packages in the session's instance (ADR-0995), so the job's end can no
/// longer be the end of an instance of its own: what ends is the job's invocation, through the
/// cancellation spec §31.14 delivers to the package. The instance and every other invocation in
/// it go on. The package is given the shutdown grace period to answer the cancellation, so the
/// instance's state no longer counts the invocation once the line has unwound; one that does not
/// answer is left to the supervisor.
///
/// `None` when the line was interrupted.
async fn collect_unless_interrupted(
    mut invocation: ono_kuang_supervisor::RunningInvocation,
) -> Option<(
    Vec<ono_kuang_supervisor::StreamEvent>,
    ono_kuang_protocol::InvokeResult,
)> {
    let mut interrupted = std::pin::pin!(crate::eval::native::interrupted());
    let mut events = Vec::new();
    loop {
        tokio::select! {
            biased;
            () = &mut interrupted => {
                invocation.cancel().await;
                let _ = tokio::time::timeout(crate::shutdown::GRACE, invocation.finish()).await;
                return None;
            }
            event = invocation.next() => match event {
                Some(event) => events.push(event),
                None => break,
            },
        }
    }
    Some((events, invocation.finish().await))
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

    let declared = declaration(&contributed_command_id(&id, command));
    let arguments = declared_arguments(declared, words);

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
        // The qualified spelling is a second door to the same contribution, and it is held to
        // the same confirmation (ADR-1001). The declaration on disk and the running package's
        // hello are two declarations of one command, and the hello is the one the package's
        // broker enforces: it is held to the registry's floor whether or not the disk declares
        // the command too (ADR-1002), and the stronger of the two decides the confirmation, so a
        // package cannot say `read` in one and act on `destructive` in the other (ADR-1022).
        let hello = plugin
            .commands()
            .iter()
            .find(|registered| registered.contribution.id == full)
            .map(|registered| hello_contract(&id, &registered.contribution))
            .transpose()
            .map_err(Flow::Failed)?;
        refuse_unconfirmed(declaration(&full), words)?;
        refuse_unconfirmed(hello.as_ref(), words)?;
        let consent = evaluator_consent(session, &id);
        runtime.block_on(async {
            let invocation = plugin
                .invoke_with_consent(&full, arguments, consent)
                .await
                .map_err(|error| crate::kuang_host::wire_error_value(&error))?;
            Ok::<_, ErrorValue>(collect_unless_interrupted(invocation).await)
        })
    };
    let (events, result) = outcome
        .map_err(Flow::Failed)?
        .ok_or_else(crate::eval::interrupted_flow_now)?;
    delivered(events, &result)
}

/// Who answers the just-in-time questions of an invocation this evaluator runs: a person at the
/// terminal of an interactive foreground, nobody in a script or a background job (K11P §14,
/// §20.3, ADR-1023). The instance is the session's whoever loaded it (ADR-0995); who may be asked
/// is the evaluator's.
///
/// `None` when the package is no longer installed, which leaves the instance's own source — one
/// that asks nobody — to answer.
fn evaluator_consent(
    session: &Session,
    package: &str,
) -> Option<std::sync::Arc<dyn ono_kuang_supervisor::ConsentSource>> {
    let installed = session
        .with_kuang(|host| host.resolve_installed(package))
        .ok()?;
    Some(std::sync::Arc::new(
        crate::kuang_permissions::ShellConsent::new(
            std::sync::Arc::clone(session.tables()),
            session.is_interactive(),
            installed,
        ),
    ))
}

/// Refuses a run of a contribution whose confirmation policy `words` do not satisfy.
///
/// The decision and the refusal are the registry entry's, the same ones a core command's
/// `confirmation: always` gives (spec §17.4, `CommandContract::unconfirmed`): a contribution that
/// declared itself `destructive` needs `--confirm` on every run, at a terminal as in a script, and
/// without it nothing of the package runs (ADR-1001).
fn refuse_unconfirmed(
    contract: Option<&CommandContract>,
    words: &[std::ffi::OsString],
) -> Eval<()> {
    let confirmed =
        json_arguments(words, contract).0.get("confirm") == Some(&serde_json::Value::Bool(true));
    match contract.and_then(|contract| contract.unconfirmed(confirmed)) {
        Some(refusal) => Err(Flow::Failed(refusal)),
        None => Ok(()),
    }
}

/// The registry entry a command the running package declared in its hello would be, read for its
/// confirmation policy.
///
/// A hello the registry would refuse refuses the invocation with the registry's own reason: a
/// declaration that understates its risk below its capabilities' (ADR-1000) would otherwise reach
/// the package without the confirmation its capabilities call for (ADR-1002). The version is not
/// part of that decision, so it is left empty.
fn hello_contract(
    package: &str,
    contribution: &ono_kuang_protocol::CommandContribution,
) -> Result<CommandContract, ErrorValue> {
    hello_entry(package, contribution, false)
}

/// The registry entry a command the running package declared in its hello would be, with the
/// options it declared there: what the provider path invokes it with — their types, their
/// defaults, which of them is its dry run — as well as its confirmation policy (ADR-1024).
///
/// # Errors
///
/// The registry's own refusal of the declaration, as for [`hello_contract`].
pub(crate) fn hello_declaration(
    package: &str,
    contribution: &ono_kuang_protocol::CommandContribution,
) -> Result<CommandContract, ErrorValue> {
    hello_entry(package, contribution, true)
}

fn hello_entry(
    package: &str,
    contribution: &ono_kuang_protocol::CommandContribution,
    with_options: bool,
) -> Result<CommandContract, ErrorValue> {
    // A straight rename across the crate boundary, as the registry makes for a declaration on
    // disk: `ono-kuang-protocol` owns the wire's shape, `ono-command` the registry's.
    let options = if with_options {
        contribution
            .options
            .iter()
            .map(|parameter| ono_command::ContributedParameter {
                name: parameter.name.clone(),
                declared_type: parameter.declared_type.clone(),
                doc: parameter.doc.clone(),
                repeatable: parameter.repeatable,
                optional_value: parameter.optional_value,
                default: parameter.default_text(),
            })
            .collect()
    } else {
        Vec::new()
    };
    ono_command::ContributedCommand {
        id: contribution.id.clone(),
        verb: contribution.verb.clone(),
        target: contribution.target.clone(),
        summary: contribution.summary.clone(),
        input: contribution.input.clone(),
        output: contribution.output.clone(),
        capabilities: contribution.capabilities.clone(),
        argument_mode: contribution.argument_mode.clone(),
        selectors: Vec::new(),
        options,
        risk: contribution.risk.clone(),
        examples: Vec::new(),
        origin: ono_command::Origin::plugin(package, ""),
    }
    .into_contract()
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
/// The arguments an invocation carries, with everything the contribution declared applied.
///
/// Three things a declaration buys, and this is where two of them are spent (ADR-0587): a word
/// the user typed is coerced to the type the declaration gives it, so `--count 1` arrives as an
/// integer because it was declared one rather than because it happened to parse as one; and a
/// declared default is supplied when the argument is absent, so a package's safe default is the
/// shell's guarantee and not each handler's memory. The third — help and completion — is spent in
/// the registry entry itself.
///
/// A word the contribution did not declare still arrives. Closing the argument set would refuse
/// invocations that work today, and nothing about a package declaring *some* of its arguments
/// says it accepts no others (`docs/contracts/kuang/contributions.v1.yaml` → `parameter.rules`).
fn declared_arguments(
    declaration: Option<&CommandContract>,
    words: &[std::ffi::OsString],
) -> serde_json::Map<String, serde_json::Value> {
    // The bare words, in order, minus the ones that were an option's value, bind to the declared
    // selectors positionally, exactly as a core command's do (ADR-0009).
    let (mut arguments, positional) = json_arguments(words, declaration);
    let Some(declaration) = declaration else {
        return arguments;
    };
    for (spec, word) in declaration.selectors().iter().zip(&positional) {
        arguments
            .entry(spec.name().to_owned())
            .or_insert_with(|| json_typed(spec, word));
    }
    for spec in declaration.selectors().iter().chain(declaration.options()) {
        match arguments.get(spec.name()) {
            Some(serde_json::Value::String(text)) => {
                let coerced = json_typed(spec, text);
                arguments.insert(spec.name().to_owned(), coerced);
            }
            Some(_) => {}
            None => {
                if let Some(default) = spec.default_value() {
                    arguments.insert(spec.name().to_owned(), json_of(default));
                }
            }
        }
    }
    arguments
}

/// One written word as the type its declaration gives it, or as the text it was.
fn json_typed(spec: &ono_command::ParameterSpec, text: &str) -> serde_json::Value {
    spec.declared_type().coerce(text).map_or_else(
        |_| serde_json::Value::String(text.to_owned()),
        |value| json_of(&value),
    )
}

/// An Ono value as the tagged JSON the protocol carries (`ono_value::to_json`).
fn json_of(value: &Value) -> serde_json::Value {
    ono_value::to_json(value)
}

/// The named arguments `words` carry, and the bare words left for the selectors.
///
/// A flag — an option the declaration types `bool`, or the host's own `--confirm` where the
/// declaration says nothing of it — takes the next word only when that word is `true` or `false`,
/// and otherwise means `true` and leaves the word where it was: the core binder's rule (ADR-0009),
/// so `remove echo-record --confirm r1` confirms and removes `r1`, as a core `confirmation:
/// always` command does wherever `--confirm` is written. Any other option takes the next word.
fn json_arguments(
    words: &[std::ffi::OsString],
    declaration: Option<&CommandContract>,
) -> (serde_json::Map<String, serde_json::Value>, Vec<String>) {
    let is_flag = |name: &str| match declaration.and_then(|declared| declared.option(name)) {
        Some(spec) => spec.declared_type().is_flag(),
        None => name == "confirm",
    };
    let mut arguments = serde_json::Map::new();
    let mut positional = Vec::new();
    let mut pending: Option<String> = None;
    let mut pending_flag: Option<String> = None;
    for word in words {
        let text = word.to_string_lossy();
        if let Some(rest) = text.strip_prefix("--") {
            for name in [pending.take(), pending_flag.take()].into_iter().flatten() {
                arguments.insert(name, serde_json::Value::Bool(true));
            }
            if let Some((name, value)) = rest.split_once('=') {
                arguments.insert(name.to_owned(), json_word(value));
            } else if is_flag(rest) {
                pending_flag = Some(rest.to_owned());
            } else {
                pending = Some(rest.to_owned());
            }
            continue;
        }
        if let Some(name) = pending.take() {
            arguments.insert(name, json_word(&text));
            continue;
        }
        if let Some(name) = pending_flag.take() {
            if let Ok(explicit) = text.parse::<bool>() {
                arguments.insert(name, serde_json::Value::Bool(explicit));
                continue;
            }
            arguments.insert(name, serde_json::Value::Bool(true));
        }
        positional.push(text.into_owned());
    }
    for name in [pending, pending_flag].into_iter().flatten() {
        arguments.insert(name, serde_json::Value::Bool(true));
    }
    (arguments, positional)
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
    match registry
        .find(&name.name, target)
        .or_else(|| registry.find(&name.name, None))
    {
        Some(contract) => (!contract.origin().is_core()).then_some(contract),
        // `watch <contributed target>` has no entry, because nothing can answer it (ADR-1013).
        // Without one the word went on to whatever program called `watch` was on PATH, which
        // answered about a command named after the target. The shell answers itself instead.
        None if name.name == "watch" => {
            target.and_then(|target| unwatchable_entry(registry, target))
        }
        None => None,
    }
}

/// The entry `watch <target>` resolves to when a package contributes `target`: never registered,
/// so `help`, completion and `get command` do not offer a command that only refuses (spec §50),
/// and answered by [`invoke_contributed`] with the refusal of
/// [`crate::plugin_provider::unwatchable`] before any package is loaded.
///
/// Built once per target and kept, like the target names `intern_target` keeps, because the
/// dispatch hands contracts around as `&'static`; the total is bounded by the distinct targets
/// the installed packages declare.
fn unwatchable_entry(
    registry: &'static ono_command::CommandRegistry,
    target: &str,
) -> Option<&'static CommandContract> {
    static ENTRIES: std::sync::OnceLock<std::sync::Mutex<Vec<&'static CommandContract>>> =
        std::sync::OnceLock::new();
    let read = registry.find("get", Some(target))?;
    let package = read.origin().package()?;
    if read.id() != crate::plugin_registry::target_command_id(package, target) {
        return None;
    }
    let entries = ENTRIES.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    let mut entries = entries
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(existing) = entries
        .iter()
        .find(|entry| entry.id() == read.id() && entry.origin() == read.origin())
    {
        return Some(existing);
    }
    let entry = ono_command::ContributedCommand {
        id: read.id().to_owned(),
        verb: "watch".to_owned(),
        target: target.to_owned(),
        summary: format!("`{target}` cannot be watched (ADR-1013)"),
        input: None,
        output: "stream<record>".to_owned(),
        capabilities: Vec::new(),
        argument_mode: "words".to_owned(),
        selectors: Vec::new(),
        options: Vec::new(),
        risk: Some("read".to_owned()),
        examples: Vec::new(),
        origin: read.origin().clone(),
    }
    .into_contract()
    .ok()?;
    let entry: &'static CommandContract = Box::leak(Box::new(entry));
    entries.push(entry);
    Some(entry)
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
/// What a contributed entry answered with.
///
/// Two shapes: a contributed command's answer is read to its end, and a contributed target's is
/// a stream. Everything a package contributed used to be read to its end, which is wrong for a
/// watch — an answer that does not end never reached the prompt at all (ADR-0588) — and wrong for
/// a bounded target too, whose first value waited for its last (ADR-1011). A target's answer
/// becomes the stream the pipeline already knows how to run and, when its declaration says it
/// does not end, the live view already knows how to show (shell specification §18.2, §18.3).
pub enum Answered {
    /// A command's answer, read to its end.
    Values(Vec<Value>),
    /// A target's answer, passed on as it arrives. The stream's boundedness is the target's
    /// declared one, so it says whether the answer ends.
    Live(ono_pipeline::ValueStream),
}

/// Answers a contributed command or target.
///
/// # Errors
///
/// The structured error of whichever step refused: loading the package, resolving the
/// contribution, or the invocation itself.
pub fn invoke_contributed(
    session: &mut Session,
    contract: &CommandContract,
    words: &[std::ffi::OsString],
) -> Eval<Answered> {
    let package = contract.origin().package().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            format!("`{}` names no package to run it", contract.spelling()),
        ))
    })?;
    // A destructive contribution is confirmed before its package is even started: the refusal
    // is the host's, so nothing of the package's needs to run to give it (ADR-1001).
    refuse_unconfirmed(Some(contract), words)?;
    // Answered from the declaration, so nothing is started to say it (ADR-1013).
    if contract.verb() == "watch"
        && let Some(target) = contract.target()
    {
        return Err(Flow::Failed(crate::plugin_provider::unwatchable(
            package, target,
        )));
    }
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
    invoke(session, package, command, words).map(Answered::Values)
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
) -> Eval<Answered> {
    let arguments = declared_arguments(
        declaration(&crate::plugin_registry::target_command_id(package, target)),
        words,
    );
    session.pipeline_context().ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the runtime",
        ))
    })?;
    {
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
        let Some(registered) = plugin
            .targets()
            .iter()
            .find(|registered| registered.contribution.name == target)
        else {
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
        };
        // A target's answer is never collected before it is passed on. It becomes a stream with
        // the boundedness the declaration gave it: an unbounded one reaches the live view of shell
        // specification §18.3 rather than a table that would never be drawn (ADR-0588), and a
        // bounded one reaches the next stage value by value, as a core provider's does — so a
        // `take 1` finishes on the first value rather than after the last (ADR-1011). The stream
        // carries the invocation's failure as its last event (ADR-0992), so a failure after some
        // values fails the pipeline instead of reading as a shorter answer.
        let boundedness = if registered.contribution.answer.is_bounded() {
            ono_pipeline::Boundedness::Bounded
        } else {
            ono_pipeline::Boundedness::Unbounded
        };
        let target = crate::plugin_provider::intern_target(target);
        let consent = evaluator_consent(session, package);
        let _guard = runtime.enter();
        Ok(Answered::Live(crate::plugin_provider::stream_of(
            plugin,
            target,
            arguments,
            boundedness,
            None,
            consent,
        )))
    }
}

/// The registry entry for a contributed id, when the registry holds one.
///
/// The declaration is read from the package's own documents before anything runs (spec §31.68),
/// so it is available at the moment an invocation is being assembled — which is what lets the
/// host type an argument and supply a default rather than forwarding whatever words arrived.
pub(crate) fn declaration(id: &str) -> Option<&'static CommandContract> {
    crate::plugin_registry::registry().ok()?.get(id)
}

/// The id a package's own command is registered under.
fn contributed_command_id(package: &str, command: &str) -> String {
    format!("{package}.command.{command}")
}

/// A package reference as the session means it: an explicit local path — `./pkg`, `../pkg`,
/// `path:pkg` — is relative to the session's directory, which in a background job is the one the
/// job was started in, not wherever the foreground has moved the process (ADR-0957). It is
/// written back absolute, so the record of where the package came from names the directory that
/// was read. Every other reference, and every reference outside a job, is left as written.
fn anchored_reference(session: &Session, reference: &str) -> String {
    if session.anchoring_directory().is_none() {
        return reference.to_owned();
    }
    let (prefix, path) = match reference.strip_prefix("path:") {
        Some(path) => ("path:", path),
        None => match ono_kuang_protocol::PluginRef::parse(reference) {
            ono_kuang_protocol::PluginRef::Path(_) => ("", reference),
            _ => return reference.to_owned(),
        },
    };
    let path = std::path::Path::new(path);
    if !path.is_relative() || path.starts_with("~") {
        return reference.to_owned();
    }
    format!("{prefix}{}", session.anchored_path(path).display())
}
