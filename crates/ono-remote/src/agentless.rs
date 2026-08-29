//! The agentless fallback of spec §21.3: a reduced provider set over plain POSIX commands.
//!
//! > If no Ono-Sendai agent exists remotely, the link MAY fall back to SSH and a limited
//! > provider set implemented through standard commands/procfs reads. Fallback MUST be visible
//! > because semantics and performance may differ. — spec §21.3
//!
//! Everything here is that sentence made structural. A link without an agent still mounts one
//! provider per target the agent would have served, so the shell above the registry does not
//! change: what changes is that most of those providers answer
//! [`Availability::Unavailable`] with the reason, and the two that can answer do it by running
//! a standard command on the far side and decoding its output through the v0.3 adaptation layer
//! (`ono-adapter`). A reduced link therefore *refuses* what it cannot see, and never returns the
//! empty list that would be indistinguishable from "there are none" (spec §35.3).
//!
//! # What a reduced link can answer
//!
//! Exactly what a first-party adapter pack already describes as a stable protocol, run with no
//! argument the user did not give:
//!
//! | target | command | adapter |
//! |---|---|---|
//! | `process` | `ps -e -o pid=,ppid=,…` | `org.ono.compat.procps/ps` |
//! | `filesystem` | `df --block-size=1 --output=…` | `org.ono.compat.coreutils/df` |
//!
//! The table is the whole claim. Adding a target to it means adding a strategy whose command and
//! decoder some adapter pack already stands behind — never a new hand-written text parser, which
//! spec §50 forbids and which the adaptation layer exists to replace.
//!
//! # Where the commands run
//!
//! [`FarSide`] is the one thing that differs between "a machine reached over ssh" and "this
//! machine". [`SshFarSide`] wraps each command in `ssh -o BatchMode=yes -T -- <host> <command>`,
//! which is spec §21.3's fallback verbatim; [`LocalFarSide`] runs it as a child of this process,
//! which is the same code path with the ssh hop removed and is what makes the fallback provable
//! without a network (the argument of ADR-0037 §2, applied again).

use std::collections::BTreeMap;
use std::sync::Arc;

use ono_adapter::{Adapter, Trace};
use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{
    Availability, Capability, ObjectRef, Provider, ProviderRegistry, Query, Risk, Selector,
};
use ono_value::{ErrorValue, Schema, Value};

use crate::retag::retag_value;
use crate::transport::SshTarget;

/// The provider id every mounted target of a reduced link carries.
///
/// One id, because there is one thing answering: the reduced set itself. Which command produced
/// a given record is not hidden by that — the adaptation layer records the executable, its
/// version and the exact invocation in the record's provenance (spec v0.3 §1.8).
pub const AGENTLESS_PROVIDER: &str = "remote.agentless";

/// How the reduced set runs a command on the far side of a link.
///
/// The trait exists so that the ssh hop is the only thing a test has to stand in for: everything
/// above it — which command runs, which adapter decodes it, what the records mean — is identical
/// whether the far side is a machine across a network or a child of this process.
pub trait FarSide: Send + Sync + std::fmt::Debug {
    /// Runs `argv` with `env` set on top of the far side's environment, and returns everything
    /// the command wrote to standard output.
    ///
    /// # Errors
    ///
    /// `remote.unreachable` (E0601) when the far side cannot be reached or the command cannot be
    /// started there, and `external.exit_nonzero` (E0801) when it ran and failed.
    fn run(&self, argv: &[String], env: &BTreeMap<String, String>) -> Result<Vec<u8>, ErrorValue>;
}

/// The far side reached by running each command through `ssh` (spec §21.3).
#[derive(Debug, Clone)]
pub struct SshFarSide {
    target: SshTarget,
}

impl SshFarSide {
    /// Commands run on `target`, one ssh invocation each.
    #[must_use]
    pub const fn new(target: SshTarget) -> Self {
        Self { target }
    }

    /// The exact command line this far side runs for `argv`, program first.
    ///
    /// `-o BatchMode=yes` for the reason ADR-0037 gives: a refusal is never a prompt. `-T`
    /// because the wire is a byte stream, not a terminal. Everything after the host is one
    /// argument, because ssh concatenates what follows and hands it to the account's login
    /// shell — so the words are quoted here, once, rather than trusted to survive a shell
    /// nobody chose.
    #[must_use]
    pub fn spelled(&self, argv: &[String], env: &BTreeMap<String, String>) -> Vec<String> {
        let mut words = vec![
            "ssh".to_owned(),
            "-o".to_owned(),
            "BatchMode=yes".to_owned(),
            "-T".to_owned(),
        ];
        if let Some(config) = self.target.config() {
            words.push("-F".to_owned());
            words.push(config.to_string_lossy().into_owned());
        }
        if let Some(port) = self.target.port() {
            words.push("-p".to_owned());
            words.push(port.to_string());
        }
        if let Some(user) = self.target.user() {
            words.push("-l".to_owned());
            words.push(user.to_owned());
        }
        words.push("--".to_owned());
        words.push(self.target.host().to_owned());
        words.push(remote_command_line(argv, env));
        words
    }
}

impl FarSide for SshFarSide {
    fn run(&self, argv: &[String], env: &BTreeMap<String, String>) -> Result<Vec<u8>, ErrorValue> {
        let spelled = self.spelled(argv, env);
        let mut command = std::process::Command::new(&spelled[0]);
        command.args(&spelled[1..]);
        capture(&mut command, "ssh", argv)
    }
}

/// The far side that is this very machine, each command run as a child process.
///
/// It is what `link host <name> --transport local --agentless` uses, and what the suites and the
/// acceptance container run: the same strategies, the same decoders, the same refusals, with the
/// ssh hop removed.
#[derive(Debug, Clone, Copy)]
pub struct LocalFarSide;

impl FarSide for LocalFarSide {
    fn run(&self, argv: &[String], env: &BTreeMap<String, String>) -> Result<Vec<u8>, ErrorValue> {
        let Some((program, arguments)) = argv.split_first() else {
            return Err(ErrorValue::new(
                ErrorCode::RemoteUnreachable,
                "an agentless strategy asked for an empty command",
            ));
        };
        let mut command = std::process::Command::new(program);
        command.args(arguments).envs(env);
        capture(&mut command, program, argv)
    }
}

/// Renders `env` and `argv` as one command line for a POSIX login shell.
///
/// Every word is single-quoted, so a filename, a `ps` format string or a host's own `IFS` cannot
/// turn one word into two on the far side.
fn remote_command_line(argv: &[String], env: &BTreeMap<String, String>) -> String {
    let mut line = String::new();
    for (name, value) in env {
        line.push_str(name);
        line.push('=');
        line.push_str(&quote(value));
        line.push(' ');
    }
    for (index, word) in argv.iter().enumerate() {
        if index > 0 {
            line.push(' ');
        }
        line.push_str(&quote(word));
    }
    line
}

/// One word, quoted so a POSIX shell reads it back unchanged.
fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// Runs `command` to completion and returns its standard output.
fn capture(
    command: &mut std::process::Command,
    program: &str,
    argv: &[String],
) -> Result<Vec<u8>, ErrorValue> {
    command
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    let output = command.output().map_err(|error| {
        ErrorValue::new(
            ErrorCode::RemoteUnreachable,
            format!("cannot run `{program}`: {error}"),
        )
    })?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    let code = output.status.code().unwrap_or(-1);
    let complaint = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    // ssh reserves 255 for its own failures and passes anything else through from the far side,
    // so the two are genuinely different diagnoses and are reported as such.
    let (code_of, detail) = if program == "ssh" && code == 255 {
        (
            ErrorCode::RemoteUnreachable,
            format!("ssh could not reach the far side: {complaint}"),
        )
    } else {
        (
            ErrorCode::ExternalExitNonzero,
            format!(
                "`{}` exited with status {code} on the far side: {complaint}",
                argv.join(" ")
            ),
        )
    };
    Err(ErrorValue::new(code_of, detail))
}

/// What the reduced set runs for one target, and which adapter reads what it writes.
#[derive(Debug)]
struct Strategy {
    /// The target this answers.
    target: &'static str,
    /// The adapter pack that owns the command's output contract.
    pack: &'static str,
    /// The adapter within that pack.
    adapter: &'static str,
    /// The declared invocation whose plan supplies the argv and the environment.
    invocation: &'static str,
}

/// The whole of what an agentless link can answer (spec §21.3's "limited provider set").
const STRATEGIES: &[Strategy] = &[
    Strategy {
        target: "process",
        pack: "org.ono.compat.procps",
        adapter: "ps",
        invocation: "every",
    },
    Strategy {
        target: "filesystem",
        pack: "org.ono.compat.coreutils",
        adapter: "df",
        invocation: "usage",
    },
];

/// The adapter a strategy decodes with, and the plan it runs.
fn planned(
    strategy: &Strategy,
) -> Option<(&'static Adapter, Vec<String>, BTreeMap<String, String>)> {
    let adapter = ono_adapter::first_party()
        .iter()
        .filter(|pack| pack.id() == strategy.pack)
        .flat_map(ono_adapter::AdapterPack::adapters)
        .find(|adapter| adapter.id() == strategy.adapter)?;
    let invocation = adapter
        .invocations()
        .iter()
        .find(|invocation| invocation.id() == strategy.invocation)?;
    Some((
        adapter,
        invocation.plan().argv().to_vec(),
        invocation.plan().env().clone(),
    ))
}

/// A link to a machine with no Ono agent on it (spec §21.3).
///
/// Opening one runs a single `uname -s -m` on the far side. That is the agentless answer to the
/// handshake of spec §21.2: it proves the far side can be reached and run a command, and it is
/// the only part of the handshake's list — remote OS and arch — that a machine without an agent
/// can be asked. Nothing else is negotiated, because there is nobody on the other end to
/// negotiate with, and claiming otherwise is what §21.3 means by "fallback MUST be visible".
#[derive(Debug)]
pub struct AgentlessLink {
    host: Arc<str>,
    system: Option<String>,
    providers: Vec<Arc<AgentlessProvider>>,
}

impl AgentlessLink {
    /// Opens a reduced link to `host` over `far_side`, mounting one provider per target in
    /// `agent_targets`.
    ///
    /// `agent_targets` is what the shell would have been able to ask an agent — the caller's own
    /// target vocabulary. Every one of them is mounted: the ones the reduced set has a strategy
    /// for answer, and the rest are [`Availability::Unavailable`] naming the mode, so a user
    /// asking for one is told what they lost rather than shown an empty table.
    ///
    /// # Errors
    ///
    /// `remote.unreachable` (E0601) when the far side does not answer `uname -s -m`.
    pub fn open(
        host: impl Into<String>,
        far_side: Arc<dyn FarSide>,
        agent_targets: &[&str],
    ) -> Result<Self, ErrorValue> {
        let host: Arc<str> = Arc::from(host.into());
        let system = far_side
            .run(
                &["uname".to_owned(), "-s".to_owned(), "-m".to_owned()],
                &BTreeMap::from([("LC_ALL".to_owned(), "C".to_owned())]),
            )
            .map_err(|error| {
                ErrorValue::new(
                    ErrorCode::RemoteUnreachable,
                    format!(
                        "{host} answers neither the Ono agent nor `uname`: {}",
                        error.message()
                    ),
                )
                .with_help(
                    "agentless mode (spec §21.3) needs a far side that can run standard commands",
                )
            })?;
        let system = String::from_utf8_lossy(&system).trim().to_owned();

        let mut providers = Vec::new();
        for target in agent_targets {
            providers.push(Arc::new(AgentlessProvider::new(
                Arc::clone(&host),
                target,
                Arc::clone(&far_side),
            )));
        }
        Ok(Self {
            host,
            system: (!system.is_empty()).then_some(system),
            providers,
        })
    }

    /// The host this link is to, as the user named it.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// What `uname -s -m` said the far side is, when it said anything.
    #[must_use]
    pub fn system(&self) -> Option<&str> {
        self.system.as_deref()
    }

    /// One mountable provider per target, the visibly unavailable ones included (spec §21.3).
    #[must_use]
    pub fn providers(&self) -> &[Arc<AgentlessProvider>] {
        &self.providers
    }

    /// The targets the reduced set can actually answer, in mount order.
    #[must_use]
    pub fn answered_targets(&self) -> Vec<String> {
        self.providers
            .iter()
            .filter(|provider| provider.availability() == Availability::Available)
            .map(|provider| provider.target.to_owned())
            .collect()
    }

    /// Registers every mounted provider — answering and refusing alike — into `registry`.
    pub fn register_into(&self, registry: &mut ProviderRegistry) {
        for provider in &self.providers {
            registry.register(Arc::clone(provider) as Arc<dyn Provider>);
        }
    }
}

/// One target of a reduced link, mounted as an ordinary [`Provider`].
#[derive(Debug)]
pub struct AgentlessProvider {
    host: Arc<str>,
    target: &'static str,
    targets: [&'static str; 1],
    far_side: Arc<dyn FarSide>,
    strategy: Option<&'static Strategy>,
}

impl AgentlessProvider {
    fn new(host: Arc<str>, target: &str, far_side: Arc<dyn FarSide>) -> Self {
        let strategy = STRATEGIES
            .iter()
            .find(|strategy| strategy.target == target)
            .filter(|strategy| planned(strategy).is_some());
        let target = crate::client::intern_target(target);
        Self {
            host,
            target,
            targets: [target],
            far_side,
            strategy,
        }
    }

    /// The records the strategy's command produces on the far side, decoded and re-tagged.
    fn read(&self) -> Result<Vec<Value>, ErrorValue> {
        let strategy = self.strategy.ok_or_else(|| self.refusal())?;
        let (adapter, argv, env) = planned(strategy).ok_or_else(|| self.refusal())?;
        let bytes = self.far_side.run(&argv, &env)?;
        let trace = Trace {
            executable: std::path::PathBuf::from(&argv[0]),
            // Nothing probed the far side's version of the tool: an agentless link runs one
            // command per query and does not spend a round trip asking, so the honest answer is
            // that it is unknown (spec §35.3).
            version: None,
            user_invocation: argv.clone(),
            actual_invocation: argv.clone(),
            host: Some(self.host.to_string()),
        };
        let decoded = ono_adapter::decode(adapter, &bytes, &trace, ono_value::builtin_schemas())?;
        Ok(decoded
            .into_iter()
            .map(|value| retag_value(value, &self.host))
            .collect())
    }

    /// Why this target cannot be answered without an agent.
    fn refusal(&self) -> ErrorValue {
        ErrorValue::new(ErrorCode::ProviderUnavailable, self.reason())
    }

    fn reason(&self) -> String {
        format!(
            "this link is agentless (spec §21.3): it reads the far side with standard commands, \
             and `{}` needs the Ono agent of spec §21.4",
            self.target
        )
    }
}

#[async_trait::async_trait]
impl Provider for AgentlessProvider {
    fn id(&self) -> &str {
        AGENTLESS_PROVIDER
    }

    fn targets(&self) -> &[&str] {
        &self.targets
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        let Some(strategy) = self.strategy else {
            return Vec::new();
        };
        let Some((adapter, _, _)) = planned(strategy) else {
            return Vec::new();
        };
        adapter
            .schema()
            .parse()
            .ok()
            .and_then(|id| ono_value::builtin_schemas().get(&id))
            .into_iter()
            .collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        // Everything a reduced link can do is a read: it runs one query command and decodes it.
        // Acting on a remote object needs the agent, which is what `availability` already says
        // for the targets that would carry an action.
        match self.strategy {
            Some(strategy) => vec![Capability::new(
                format!("{}.list", strategy.target),
                Risk::Read,
            )],
            None => Vec::new(),
        }
    }

    fn availability(&self) -> Availability {
        match self.strategy {
            Some(_) => Availability::Available,
            None => Availability::unavailable(self.reason()),
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let records = self.read()?;
        let query = query.clone();
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                for value in records {
                    let keep = match &value {
                        Value::Record(record) => query.matches(record),
                        _ => true,
                    };
                    if keep && sink.send(value).await.is_err() {
                        break;
                    }
                }
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let records = self.read()?;
        Ok(records
            .iter()
            .filter_map(|value| match value {
                Value::Record(record) if selector.matches(record) => ObjectRef::of(record),
                _ => None,
            })
            .collect())
    }
}
