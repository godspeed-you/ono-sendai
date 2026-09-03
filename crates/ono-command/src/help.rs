//! Help, generated from the registry (spec §15.2).
//!
//! Spec §15.2 lists what a help page contains at minimum — synopsis, description, selectors,
//! options, input type, output type, privileges and capabilities, examples, related commands,
//! provider source, stability — and says it *derives from metadata*. So no page here is written
//! by hand: everything is read out of `docs/spec/`, which is also why a command cannot be added
//! without its help being complete (spec §50).

use std::fmt::Write as _;
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_provider_api::ProviderRegistry;
use ono_value::{ErrorValue, MapValue, Value};

use crate::contract::{CommandContract, ParameterSpec, TargetSpec, VerbSpec};
use crate::registry::CommandRegistry;
use crate::suggest::closest;

/// The width the label column is padded to, so a page reads as a table without a table renderer.
const LABEL: usize = 13;

/// A help page: a command, a verb, a target, or a browsing topic.
#[derive(Debug, Clone, PartialEq)]
pub enum HelpPage {
    /// One command, with everything spec §15.2 requires.
    Command(Box<CommandHelp>),
    /// One verb and the commands that use it.
    Verb(VerbHelp),
    /// One target and the commands that address it.
    Target(TargetHelp),
    /// A browsing topic: the overview, the verbs, the targets, the capabilities.
    Topic(TopicHelp),
}

impl HelpPage {
    /// The page's title, as a user would name it.
    #[must_use]
    pub fn title(&self) -> &str {
        match self {
            HelpPage::Command(page) => &page.spelling,
            HelpPage::Verb(page) => &page.verb,
            HelpPage::Target(page) => &page.name,
            HelpPage::Topic(page) => &page.name,
        }
    }

    /// The page as plain text, for a terminal.
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            HelpPage::Command(page) => page.render(),
            HelpPage::Verb(page) => page.render(),
            HelpPage::Target(page) => page.render(),
            HelpPage::Topic(page) => page.render(),
        }
    }

    /// The page as structured data, so a script reads the fields rather than the rendering.
    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            HelpPage::Command(page) => page.to_value(),
            HelpPage::Verb(page) => page.to_value(),
            HelpPage::Target(page) => page.to_value(),
            HelpPage::Topic(page) => page.to_value(),
        }
    }
}

/// One selector or option, as help presents it.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterHelp {
    /// How it is written: `pid` for a selector, `--tree` for an option.
    pub written: String,
    /// The declared type.
    pub declared_type: String,
    /// What it is for.
    pub doc: String,
    /// The default, where the registry declares one.
    pub default: Option<String>,
}

impl ParameterHelp {
    fn of(spec: &ParameterSpec, option: bool) -> Self {
        Self {
            written: if option {
                format!("--{}", spec.name())
            } else {
                spec.name().to_owned()
            },
            declared_type: spec.declared_type().name(),
            doc: spec.doc().to_owned(),
            default: spec.default_text().map(str::to_owned),
        }
    }

    fn render(&self, into: &mut String) {
        let default = match &self.default {
            Some(default) => format!(" (default {default})"),
            None => String::new(),
        };
        let _ = writeln!(
            into,
            "  {:<width$} {}{default}\n  {:<width$}   {}",
            self.written,
            self.declared_type,
            "",
            self.doc,
            width = LABEL
        );
    }

    fn to_value(&self) -> Value {
        let mut map = MapValue::default();
        map.insert("name".into(), Value::string(&self.written));
        map.insert("type".into(), Value::string(&self.declared_type));
        map.insert("doc".into(), Value::string(&self.doc));
        map.insert(
            "default".into(),
            self.default.as_deref().map_or(Value::Null, Value::string),
        );
        Value::Map(Arc::new(map))
    }
}

/// A command's help page: everything spec §15.2 requires, and nothing invented.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandHelp {
    /// The stable command id.
    pub id: String,
    /// How the command is written, such as `get process`.
    pub spelling: String,
    /// The one-line summary.
    pub summary: String,
    /// The longer description, where the registry has one.
    pub description: Option<String>,
    /// The synopsis: the command with its selectors and options.
    pub synopsis: String,
    /// The positional selectors.
    pub selectors: Vec<ParameterHelp>,
    /// The `--named` options.
    pub options: Vec<ParameterHelp>,
    /// What the command accepts through the pipeline.
    pub input: String,
    /// What it emits.
    pub output: String,
    /// Whether it produces output incrementally.
    pub streaming: bool,
    /// What the user needs before it runs.
    pub privilege: String,
    /// The provider capability it needs, and what that capability is.
    pub capability: Option<String>,
    /// The one-line summary of that capability.
    pub capability_summary: Option<String>,
    /// The risk of that capability: read, observe, mutate, destructive.
    pub risk: Option<String>,
    /// The providers registered for the command's target, where any are.
    pub providers: Vec<String>,
    /// How much of a compatibility promise the command is.
    pub stability: String,
    /// Where the command came from: `core`, or the package that contributed it (spec §31.64).
    pub origin: String,
    /// The spec §37 phase that delivers it.
    pub phase: String,
    /// The documented examples.
    pub examples: Vec<String>,
    /// Commands a reader is likely to want next.
    pub related: Vec<String>,
}

impl CommandHelp {
    /// The page as plain text.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = String::new();
        let _ = writeln!(text, "{}  —  {}", self.spelling, self.summary);
        let _ = writeln!(text);
        let _ = writeln!(text, "SYNOPSIS\n  {}", self.synopsis);
        if let Some(description) = &self.description {
            let _ = writeln!(text, "\nDESCRIPTION\n  {description}");
        }
        if !self.selectors.is_empty() {
            let _ = writeln!(text, "\nSELECTORS");
            for selector in &self.selectors {
                selector.render(&mut text);
            }
        }
        if !self.options.is_empty() {
            let _ = writeln!(text, "\nOPTIONS");
            for option in &self.options {
                option.render(&mut text);
            }
        }
        let _ = writeln!(text, "\nTYPES");
        row(&mut text, "input", &self.input);
        row(&mut text, "output", &self.output);
        row(
            &mut text,
            "streaming",
            if self.streaming { "yes" } else { "no" },
        );
        let _ = writeln!(text, "\nSAFETY");
        row(&mut text, "privilege", &self.privilege);
        if let Some(capability) = &self.capability {
            let summary = self.capability_summary.as_deref().unwrap_or_default();
            row(&mut text, "capability", &format!("{capability}  {summary}"));
        }
        if let Some(risk) = &self.risk {
            row(&mut text, "risk", risk);
        }
        if self.providers.is_empty() {
            row(&mut text, "provider", "none registered");
        } else {
            row(&mut text, "provider", &self.providers.join(", "));
        }
        let _ = writeln!(text, "\nCONTRACT");
        row(&mut text, "id", &self.id);
        row(&mut text, "origin", &self.origin);
        row(&mut text, "stability", &self.stability);
        row(&mut text, "phase", &self.phase);
        let _ = writeln!(text, "\nEXAMPLES");
        for example in &self.examples {
            let _ = writeln!(text, "  {example}");
        }
        if !self.related.is_empty() {
            let _ = writeln!(text, "\nRELATED\n  {}", self.related.join("  "));
        }
        text
    }

    /// The page as structured data.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut map = MapValue::default();
        map.insert("id".into(), Value::string(&self.id));
        map.insert("command".into(), Value::string(&self.spelling));
        map.insert("summary".into(), Value::string(&self.summary));
        map.insert("synopsis".into(), Value::string(&self.synopsis));
        map.insert(
            "description".into(),
            self.description
                .as_deref()
                .map_or(Value::Null, Value::string),
        );
        map.insert(
            "selectors".into(),
            Value::list(self.selectors.iter().map(ParameterHelp::to_value)),
        );
        map.insert(
            "options".into(),
            Value::list(self.options.iter().map(ParameterHelp::to_value)),
        );
        map.insert("input".into(), Value::string(&self.input));
        map.insert("output".into(), Value::string(&self.output));
        map.insert("streaming".into(), Value::Bool(self.streaming));
        map.insert("privilege".into(), Value::string(&self.privilege));
        map.insert(
            "capability".into(),
            self.capability
                .as_deref()
                .map_or(Value::Null, Value::string),
        );
        map.insert(
            "risk".into(),
            self.risk.as_deref().map_or(Value::Null, Value::string),
        );
        map.insert(
            "providers".into(),
            Value::list(self.providers.iter().map(|id| Value::string(id))),
        );
        map.insert("stability".into(), Value::string(&self.stability));
        map.insert("origin".into(), Value::string(&self.origin));
        map.insert("phase".into(), Value::string(&self.phase));
        map.insert(
            "examples".into(),
            Value::list(self.examples.iter().map(|example| Value::string(example))),
        );
        map.insert(
            "related".into(),
            Value::list(self.related.iter().map(|command| Value::string(command))),
        );
        Value::Map(Arc::new(map))
    }
}

/// A verb's help page: what the verb means, and the commands that use it.
#[derive(Debug, Clone, PartialEq)]
pub struct VerbHelp {
    /// The verb id.
    pub id: String,
    /// The word the user types.
    pub verb: String,
    /// One line of semantics, from spec §7.1.
    pub semantics: String,
    /// Where the verb sits in a pipeline.
    pub pipeline_role: String,
    /// Whether it changes state outside the shell.
    pub mutating: bool,
    /// The commands that use it: spelling and summary.
    pub commands: Vec<(String, String)>,
}

impl VerbHelp {
    /// The page as plain text.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = String::new();
        let _ = writeln!(text, "{}  —  {}", self.verb, self.semantics);
        let _ = writeln!(text);
        row(&mut text, "role", &self.pipeline_role);
        row(
            &mut text,
            "mutating",
            if self.mutating { "yes" } else { "no" },
        );
        let _ = writeln!(text, "\nCOMMANDS");
        for (spelling, summary) in &self.commands {
            let _ = writeln!(text, "  {spelling:<24} {summary}");
        }
        text
    }

    /// The page as structured data.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut map = MapValue::default();
        map.insert("id".into(), Value::string(&self.id));
        map.insert("verb".into(), Value::string(&self.verb));
        map.insert("semantics".into(), Value::string(&self.semantics));
        map.insert("pipeline_role".into(), Value::string(&self.pipeline_role));
        map.insert("mutating".into(), Value::Bool(self.mutating));
        map.insert("commands".into(), entries(&self.commands));
        Value::Map(Arc::new(map))
    }
}

/// A target's help page: what the target denotes, and the commands that address it.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetHelp {
    /// The target id.
    pub id: String,
    /// The word the user types after a verb.
    pub name: String,
    /// `system`, `development` or `infrastructure`.
    pub category: String,
    /// What the target denotes.
    pub summary: String,
    /// The canonical schema a producer of this target emits.
    pub schema: Option<String>,
    /// The spec §37 phase that delivers it.
    pub phase: String,
    /// The commands that address it: spelling and summary.
    pub commands: Vec<(String, String)>,
}

impl TargetHelp {
    /// The page as plain text.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = String::new();
        let _ = writeln!(text, "{}  —  {}", self.name, self.summary);
        let _ = writeln!(text);
        row(&mut text, "category", &self.category);
        row(
            &mut text,
            "schema",
            self.schema.as_deref().unwrap_or("not yet defined"),
        );
        row(&mut text, "phase", &self.phase);
        let _ = writeln!(text, "\nCOMMANDS");
        for (spelling, summary) in &self.commands {
            let _ = writeln!(text, "  {spelling:<24} {summary}");
        }
        text
    }

    /// The page as structured data.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut map = MapValue::default();
        map.insert("id".into(), Value::string(&self.id));
        map.insert("target".into(), Value::string(&self.name));
        map.insert("category".into(), Value::string(&self.category));
        map.insert("summary".into(), Value::string(&self.summary));
        map.insert(
            "schema".into(),
            self.schema.as_deref().map_or(Value::Null, Value::string),
        );
        map.insert("phase".into(), Value::string(&self.phase));
        map.insert("commands".into(), entries(&self.commands));
        Value::Map(Arc::new(map))
    }
}

/// A browsing topic: a list of names with one line each.
#[derive(Debug, Clone, PartialEq)]
pub struct TopicHelp {
    /// The topic's name, as `help <name>` spells it.
    pub name: String,
    /// One line saying what the topic lists.
    pub summary: String,
    /// The entries: a name and a line about it.
    pub entries: Vec<(String, String)>,
    /// Where to go next.
    pub see_also: Vec<String>,
}

impl TopicHelp {
    /// The page as plain text.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = String::new();
        let _ = writeln!(text, "{}  —  {}", self.name, self.summary);
        let _ = writeln!(text);
        for (name, line) in &self.entries {
            let _ = writeln!(text, "  {name:<24} {line}");
        }
        if !self.see_also.is_empty() {
            let _ = writeln!(text, "\nSEE ALSO");
            for line in &self.see_also {
                let _ = writeln!(text, "  {line}");
            }
        }
        text
    }

    /// The page as structured data.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut map = MapValue::default();
        map.insert("topic".into(), Value::string(&self.name));
        map.insert("summary".into(), Value::string(&self.summary));
        map.insert("entries".into(), entries(&self.entries));
        Value::Map(Arc::new(map))
    }
}

fn row(into: &mut String, label: &str, value: &str) {
    let _ = writeln!(into, "  {label:<LABEL$} {value}");
}

fn entries(pairs: &[(String, String)]) -> Value {
    Value::list(pairs.iter().map(|(name, line)| {
        let mut map = MapValue::default();
        map.insert("name".into(), Value::string(name));
        map.insert("summary".into(), Value::string(line));
        Value::Map(Arc::new(map))
    }))
}

/// The help page for `topic`.
///
/// `topic` is a command id (`ono.process.get`), a command spelling (`get process`), a verb
/// (`get`), a target (`process`), a browsing topic (`verbs`, `targets`, `capabilities`,
/// `commands`) or empty for the overview. `providers` fills in the provider source spec §15.2
/// asks for; without it the page reports the required capability alone.
///
/// # Errors
///
/// `resolve.command_not_found` when nothing in the registry answers to `topic`, carrying the near
/// miss spec §15.4 asks for.
///
/// ```
/// let registry = ono_command::CommandRegistry::embedded()?;
/// let page = ono_command::help(registry, None, "get process")?;
/// assert!(page.render().contains("stream<ono.process/1>"));
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
pub fn help(
    registry: &CommandRegistry,
    providers: Option<&ProviderRegistry>,
    topic: &str,
) -> Result<HelpPage, ErrorValue> {
    let topic = topic.trim();
    if topic.is_empty() {
        return Ok(HelpPage::Topic(overview(registry)));
    }
    if let Some(page) = builtin_topic(registry, topic) {
        return Ok(HelpPage::Topic(page));
    }
    if let Some(command) = registry.get(topic) {
        return Ok(HelpPage::Command(Box::new(command_help(
            registry, providers, command,
        ))));
    }

    let words: Vec<&str> = topic.split_whitespace().collect();
    match words.as_slice() {
        [verb, target] => registry
            .find(verb, Some(target))
            .map(|command| HelpPage::Command(Box::new(command_help(registry, providers, command))))
            .ok_or_else(|| not_found(registry, topic)),
        [name] => {
            if let Some(command) = registry.find(name, None) {
                return Ok(HelpPage::Command(Box::new(command_help(
                    registry, providers, command,
                ))));
            }
            if let Some(verb) = registry.verb(name) {
                return Ok(HelpPage::Verb(verb_help(registry, verb)));
            }
            if let Some(target) = registry.target(name) {
                return Ok(HelpPage::Target(target_help(registry, target)));
            }
            Err(not_found(registry, topic))
        }
        _ => Err(not_found(registry, topic)),
    }
}

fn command_help(
    registry: &CommandRegistry,
    providers: Option<&ProviderRegistry>,
    command: &CommandContract,
) -> CommandHelp {
    let capability = command
        .provider_capability()
        .and_then(|id| registry.capability(id));
    let provider_ids = match (providers, command.target()) {
        (Some(registered), Some(target)) => registered
            .for_target(target)
            .iter()
            .map(|provider| provider.id().to_owned())
            .collect(),
        _ => Vec::new(),
    };
    let mut related: Vec<String> = registry
        .by_target(command.target().unwrap_or_default())
        .into_iter()
        .chain(registry.by_verb(command.verb()))
        .filter(|other| other.id() != command.id())
        .map(CommandContract::spelling)
        .collect();
    related.sort();
    related.dedup();
    related.truncate(8);

    CommandHelp {
        id: command.id().to_owned(),
        spelling: command.spelling(),
        summary: command.summary().to_owned(),
        description: command.note().map(|note| note.trim().to_owned()),
        synopsis: synopsis(command),
        selectors: command
            .selectors()
            .iter()
            .map(|spec| ParameterHelp::of(spec, false))
            .collect(),
        options: command
            .options()
            .iter()
            .map(|spec| ParameterHelp::of(spec, true))
            .collect(),
        input: command.input().text().to_owned(),
        output: command.output().text().to_owned(),
        streaming: command.is_streaming(),
        privilege: command.privilege().as_str().to_owned(),
        capability: command.provider_capability().map(str::to_owned),
        capability_summary: capability.map(|entry| entry.summary().to_owned()),
        risk: capability.map(|entry| entry.risk().as_str().to_owned()),
        providers: provider_ids,
        stability: command.stability().as_str().to_owned(),
        origin: command.origin().to_string(),
        phase: command.phase().to_string(),
        examples: command.examples().to_vec(),
        related,
    }
}

fn synopsis(command: &CommandContract) -> String {
    let mut synopsis = command.spelling();
    for selector in command.selectors() {
        synopsis.push_str(&format!(" [{}]", selector.name()));
    }
    for option in command.options() {
        if option.declared_type().is_flag() {
            synopsis.push_str(&format!(" [--{}]", option.name()));
        } else {
            synopsis.push_str(&format!(
                " [--{} <{}>]",
                option.name(),
                option.declared_type().name()
            ));
        }
    }
    synopsis
}

fn verb_help(registry: &CommandRegistry, verb: &VerbSpec) -> VerbHelp {
    VerbHelp {
        id: verb.id().to_owned(),
        verb: verb.verb().to_owned(),
        semantics: verb.semantics().to_owned(),
        pipeline_role: verb.pipeline_role().to_owned(),
        mutating: verb.is_mutating(),
        commands: summaries(registry.by_verb(verb.verb())),
    }
}

fn target_help(registry: &CommandRegistry, target: &TargetSpec) -> TargetHelp {
    TargetHelp {
        id: target.id().to_owned(),
        name: target.name().to_owned(),
        category: target.category().to_owned(),
        summary: target.summary().to_owned(),
        schema: target.schema().map(str::to_owned),
        phase: target.phase().to_owned(),
        commands: summaries(registry.by_target(target.name())),
    }
}

fn summaries(commands: Vec<&CommandContract>) -> Vec<(String, String)> {
    commands
        .into_iter()
        .map(|command| (command.spelling(), command.summary().to_owned()))
        .collect()
}

fn overview(registry: &CommandRegistry) -> TopicHelp {
    TopicHelp {
        name: "help".to_owned(),
        summary: format!(
            "{} commands, {} verbs and {} targets — the shell's whole public surface",
            registry.len(),
            registry.verbs().len(),
            registry.targets().len()
        ),
        entries: registry
            .verbs()
            .iter()
            .map(|verb| (verb.verb().to_owned(), verb.semantics().to_owned()))
            .collect(),
        see_also: vec![
            "help verbs             every verb and what it means".to_owned(),
            "help targets           every target and what it denotes".to_owned(),
            "help capabilities      what a provider must be allowed to do".to_owned(),
            "help commands          every stable command".to_owned(),
            "help get process       one command in full".to_owned(),
            "help raw               the escape hatch that bypasses adaptation".to_owned(),
            "help adapt             force a program's output into values".to_owned(),
            "help spatial           moving through the system as a space".to_owned(),
            "help plugin-trust      what a KUANG/11 plugin can and cannot reach".to_owned(),
            "help here              what the place you are standing in offers".to_owned(),
        ],
    }
}

/// The browsing topics `help <topic>` answers for, each with the line the landing page shows.
///
/// This is the only enumeration of them: `builtin_topic`'s match is what answers, and a match
/// arm cannot be listed. Completion needs the list (spec §15.1) and so does anything that wants
/// to say what `help` knows about, so the two are kept beside each other and `spec-check`'s
/// example checks keep them honest.
#[must_use]
pub fn topics() -> &'static [(&'static str, &'static str)] {
    &[
        ("verbs", "every verb and what it means"),
        ("targets", "every target and what it denotes"),
        ("capabilities", "what a provider must be allowed to do"),
        ("commands", "every stable command"),
        ("raw", "the escape hatch that bypasses adaptation"),
        ("adapt", "force a program's output into values"),
        (
            "plugin-trust",
            "what a KUANG/11 plugin can and cannot reach",
        ),
        ("spatial", "moving through the system as a space"),
        ("here", "what the place you are standing in offers"),
    ]
}

fn builtin_topic(registry: &CommandRegistry, topic: &str) -> Option<TopicHelp> {
    let page = match topic {
        "verbs" => TopicHelp {
            name: "verbs".to_owned(),
            summary: "The curated verb vocabulary of spec §7.1.".to_owned(),
            entries: registry
                .verbs()
                .iter()
                .map(|verb| (verb.verb().to_owned(), verb.semantics().to_owned()))
                .collect(),
            see_also: vec!["help <verb>            one verb and its commands".to_owned()],
        },
        "targets" => TopicHelp {
            name: "targets".to_owned(),
            summary: "The objects this shell can be asked about, spec §8.".to_owned(),
            entries: registry
                .targets()
                .iter()
                .map(|target| (target.name().to_owned(), target.summary().to_owned()))
                .collect(),
            see_also: vec!["help <target>          one target and its commands".to_owned()],
        },
        "capabilities" => TopicHelp {
            name: "capabilities".to_owned(),
            summary: "What a provider must be able to do for a command to work.".to_owned(),
            entries: registry
                .capabilities()
                .iter()
                .map(|capability| {
                    (
                        capability.id().to_owned(),
                        format!(
                            "{} [{} / {}]",
                            capability.summary(),
                            capability.risk(),
                            capability.elevation()
                        ),
                    )
                })
                .collect(),
            see_also: Vec::new(),
        },
        "commands" => TopicHelp {
            name: "commands".to_owned(),
            summary: "Every command whose id and semantics are a compatibility promise.".to_owned(),
            entries: summaries(registry.with_stability(crate::contract::Stability::Stable)),
            see_also: vec!["help <command>         one command in full".to_owned()],
        },
        "raw" => TopicHelp {
            name: "raw".to_owned(),
            summary: "Run a program with nothing between it and the terminal (spec v0.3 §1.17)."
                .to_owned(),
            entries: vec![
                (
                    "raw <program> [arguments]".to_owned(),
                    "the program on PATH, as typed: no argv rewrite, no decoder, no Ono \
                     renderer, stdout and stderr as ordinary streams, its own exit status. The \
                     guaranteed escape hatch when an adapter would otherwise adapt the command."
                        .to_owned(),
                ),
                (
                    "exec:<program> [arguments]".to_owned(),
                    "the program rather than a native command of the same name; adaptation \
                     still follows what the pipeline demands (ADR-0011, ADR-0054)."
                        .to_owned(),
                ),
            ],
            see_also: vec![
                "explain raw <program>  the plan, with adaptation shown as bypassed".to_owned(),
            ],
        },
        "adapt" => TopicHelp {
            name: "adapt".to_owned(),
            summary: "Force a program's output into values, or fail (spec v0.3 §1.18).".to_owned(),
            entries: vec![
                (
                    "adapt <program> [arguments]".to_owned(),
                    "the program through its adapter whatever the consumer: the records it \
                     produces, or `adapter.required_for_structured_pipeline` when no adapter \
                     answers — never text. `adapt curl <url> | inspect` shows an HTTP exchange \
                     as an object."
                        .to_owned(),
                ),
                (
                    "raw <program> [arguments]".to_owned(),
                    "the opposite: no adapter, bytes as the program wrote them (ADR-0054)."
                        .to_owned(),
                ),
            ],
            see_also: vec![
                "explain adapt <program>  the plan, the adapter chosen, the demand forced"
                    .to_owned(),
            ],
        },
        // v0.4.1 §15.1 fixes three concepts the documentation has to keep apart, §15.2 requires
        // the native trust statement to be stated, and §17.3 forbids calling a tier "sandboxed"
        // without saying which boundary is meant. This page is where `help` says it (ADR-0447).
        "plugin-trust" => TopicHelp {
            name: "plugin-trust".to_owned(),
            summary: "What a KUANG/11 plugin can and cannot reach (spec v0.4.1 §15).".to_owned(),
            entries: vec![
                (
                    "capability mediation".to_owned(),
                    "Ono decides which operations the plugin protocol may ask Ono to perform.                      `get capability` is the table, default-deny; a call outside a grant is                      refused and audited."
                        .to_owned(),
                ),
                (
                    "process confinement".to_owned(),
                    "resource ceilings, no-new-privileges, session separation, descriptor and                      environment hygiene, a private working directory — installed before the                      plugin's first instruction, and each one able to refuse the launch.                      `inspect plugin <id>` shows which are in force."
                        .to_owned(),
                ),
                (
                    "kernel isolation".to_owned(),
                    "kernel policy preventing direct filesystem or network access outside an                      allowlist. The native tier does NOT provide this."
                        .to_owned(),
                ),
                (
                    "what that means".to_owned(),
                    "A native KUANG/11 plugin executes as a process of the Ono user. Ono limits                      its brokered capabilities and applies process confinement, but native                      execution is not a complete filesystem or network sandbox. Install native                      plugins only from sources you are willing to run as your user account."
                        .to_owned(),
                ),
                (
                    "brokered vs. direct".to_owned(),
                    "a denied capability means `brokered capability: denied`; it does not mean                      the process cannot make the equivalent syscall itself. That is                      `native direct OS access: not isolated by this execution tier`."
                        .to_owned(),
                ),
            ],
            see_also: vec![
                "inspect plugin <id>    the execution tier and the controls in force".to_owned(),
                "get capability         the broker's table, default-deny".to_owned(),
            ],
        },
        // v0.4 §38.1: the overview a user reaches for before they know a verb to ask about. The
        // eleven lines are the ones that section lists, in its order; each verb's full page is
        // `help <verb>`, generated from `docs/spec/commands/spatial.yaml` like every other.
        "spatial" => TopicHelp {
            name: "spatial".to_owned(),
            summary: "Moving through the system as a space (spec v0.4 §6, §38.1).".to_owned(),
            entries: vec![
                (
                    "look".to_owned(),
                    "see where you are and what is nearby".to_owned(),
                ),
                ("map".to_owned(), "see topology".to_owned()),
                (
                    "enter".to_owned(),
                    "move into a visible child or object".to_owned(),
                ),
                ("follow".to_owned(), "traverse a relationship".to_owned()),
                (
                    "jump".to_owned(),
                    "move directly to another known place".to_owned(),
                ),
                ("back".to_owned(), "return along your trail".to_owned()),
                ("up".to_owned(), "go to the canonical parent".to_owned()),
                ("home".to_owned(), "return to the system root".to_owned()),
                ("near".to_owned(), "query neighbouring objects".to_owned()),
                ("find place".to_owned(), "search known places".to_owned()),
                ("trail".to_owned(), "inspect where you moved".to_owned()),
                (
                    "pin / unpin".to_owned(),
                    "keep a place as a landmark of your own, across sessions".to_owned(),
                ),
            ],
            see_also: vec![
                "help look              one spatial command in full".to_owned(),
                "help find place        the spatial search beside `find file`".to_owned(),
            ],
        },
        _ => return None,
    };
    Some(page)
}

fn not_found(registry: &CommandRegistry, topic: &str) -> ErrorValue {
    let error = ErrorValue::new(
        ErrorCode::ResolveCommandNotFound,
        format!("`{topic}` is not a command, verb, target or topic"),
    );
    let candidates: Vec<&str> = registry
        .verbs()
        .iter()
        .map(VerbSpec::verb)
        .chain(registry.targets().iter().map(TargetSpec::name))
        .chain(registry.commands().iter().map(CommandContract::id))
        .collect();
    match closest(topic, candidates) {
        Some(near) => error.with_help(format!("did you mean `{near}`?")),
        None => error.with_help("`help` lists the verbs, targets and topics"),
    }
}
