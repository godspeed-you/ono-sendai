//! The command registry: the contract files of `docs/spec/commands/`, available at runtime.
//!
//! The files are embedded at compile time. Spec §34 budgets a cold start of under 100 ms with a
//! target of 50 ms, which a dozen YAML reads would spend before the prompt appears; and a shell
//! whose command set depends on files being installed correctly is a shell that breaks when they
//! are not. The registry is therefore part of the binary, parsed once on first use.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use ono_core::ErrorCode;
use ono_parser::Argument;
use ono_value::ErrorValue;

use crate::contract::{
    CapabilitySpec, CommandContract, RawCapabilityFile, RawFamily, RawTargetFile, RawVerbFile,
    Stability, TargetSpec, VerbSpec,
};
use crate::suggest::closest;

/// The command families of `docs/spec/commands/`, embedded verbatim.
const COMMAND_FILES: &[&str] = &[
    include_str!("../../../docs/spec/commands/container.yaml"),
    include_str!("../../../docs/spec/commands/data.yaml"),
    include_str!("../../../docs/spec/commands/file.yaml"),
    include_str!("../../../docs/spec/commands/identity.yaml"),
    include_str!("../../../docs/spec/commands/kuang.yaml"),
    include_str!("../../../docs/spec/commands/meta.yaml"),
    include_str!("../../../docs/spec/commands/network.yaml"),
    include_str!("../../../docs/spec/commands/package.yaml"),
    include_str!("../../../docs/spec/commands/process.yaml"),
    include_str!("../../../docs/spec/commands/remote.yaml"),
    include_str!("../../../docs/spec/commands/service.yaml"),
    include_str!("../../../docs/spec/commands/spatial.yaml"),
    include_str!("../../../docs/spec/commands/storage.yaml"),
];

const VERB_FILE: &str = include_str!("../../../docs/spec/verbs.yaml");
const TARGET_FILE: &str = include_str!("../../../docs/spec/targets.yaml");
const CAPABILITY_FILE: &str = include_str!("../../../docs/spec/capabilities.yaml");

static EMBEDDED: OnceLock<Result<CommandRegistry, String>> = OnceLock::new();

/// A command resolved from a stage head, together with the arguments that remain after the
/// target word has been consumed.
#[derive(Debug, Clone, Copy)]
pub struct Resolved<'r, 'a> {
    /// The command the head named. It borrows the registry, not the stage, so a caller holding a
    /// `&'static CommandRegistry` keeps a `&'static CommandContract`.
    pub contract: &'r CommandContract,
    /// The arguments still to be bound: everything after the verb and, where the command takes
    /// one, after the target word.
    pub arguments: &'a [Argument],
}

/// Every public command contract the shell knows, with the verb, target and capability
/// registries the contracts refer to.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandRegistry {
    commands: Vec<CommandContract>,
    by_id: BTreeMap<String, usize>,
    by_spelling: BTreeMap<(String, Option<String>), usize>,
    verbs: Vec<VerbSpec>,
    targets: Vec<TargetSpec>,
    capabilities: Vec<CapabilitySpec>,
}

impl CommandRegistry {
    /// The registry compiled into this binary.
    ///
    /// Parsed once, on first use. The result is memoised, so the second caller pays nothing.
    ///
    /// # Errors
    ///
    /// Returns a structured error when an embedded contract file does not typecheck against the
    /// vocabulary of ADR-0012 — a build defect rather than a runtime condition, reported rather
    /// than papered over with an empty registry.
    ///
    /// ```
    /// let registry = ono_command::CommandRegistry::embedded()?;
    /// let command = registry.find("get", Some("process")).expect("`get process` is declared");
    /// assert_eq!(command.id(), "ono.process.get");
    /// # Ok::<(), ono_value::ErrorValue>(())
    /// ```
    pub fn embedded() -> Result<&'static Self, ErrorValue> {
        match EMBEDDED.get_or_init(|| Self::load().map_err(|error| error.message().to_owned())) {
            Ok(registry) => Ok(registry),
            Err(message) => Err(ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                message.clone(),
            )
            .with_help(
                "the command contracts are compiled into the binary; this is a build defect",
            )),
        }
    }

    /// Parses the embedded contract files into a fresh registry.
    ///
    /// # Errors
    ///
    /// Returns a structured error when a file is not valid YAML or an entry is not a valid
    /// contract.
    pub fn load() -> Result<Self, ErrorValue> {
        let mut commands = Vec::new();
        for source in COMMAND_FILES {
            let family: RawFamily = serde_yaml_ng::from_str(source).map_err(yaml_error)?;
            for raw in family.commands {
                commands.push(raw.into_contract(&family.family)?);
            }
        }
        let verbs: RawVerbFile = serde_yaml_ng::from_str(VERB_FILE).map_err(yaml_error)?;
        let targets: RawTargetFile = serde_yaml_ng::from_str(TARGET_FILE).map_err(yaml_error)?;
        let capabilities: RawCapabilityFile =
            serde_yaml_ng::from_str(CAPABILITY_FILE).map_err(yaml_error)?;

        let mut by_id = BTreeMap::new();
        let mut by_spelling = BTreeMap::new();
        for (index, command) in commands.iter().enumerate() {
            if by_id.insert(command.id().to_owned(), index).is_some() {
                return Err(ErrorValue::new(
                    ErrorCode::ResolveAmbiguous,
                    format!(
                        "`{}` is declared twice in docs/spec/commands/",
                        command.id()
                    ),
                ));
            }
            let spelling = (
                command.verb().to_owned(),
                command.target().map(str::to_owned),
            );
            if by_spelling.insert(spelling, index).is_some() {
                return Err(ErrorValue::new(
                    ErrorCode::ResolveAmbiguous,
                    format!(
                        "`{}` is claimed by more than one command; the core never allows two \
                         natives to claim one name (spec §40.1)",
                        command.spelling()
                    ),
                ));
            }
        }

        Ok(Self {
            commands,
            by_id,
            by_spelling,
            verbs: verbs.verbs,
            targets: targets.targets,
            capabilities: capabilities
                .provider_capabilities
                .into_iter()
                .map(crate::contract::RawCapability::into_spec)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    /// This registry with `contributions` added, and the contributions it refused.
    ///
    /// The registries KUANG/11 contributes into are the shell's own (spec §31.64): a contributed
    /// command is a command, and the same `get command` finds it. Two rules keep that from
    /// meaning "and is therefore trusted", both from
    /// `docs/spec/kuang/contributions.v1.yaml` → `registration_checks`:
    ///
    /// - **`no-core-shadow`** — a contribution whose spelling a core command already holds is
    ///   refused. Ono's own vocabulary cannot be replaced from a package directory.
    /// - **conflict resolution (spec §31.65)** — when two packages claim one spelling, neither
    ///   takes it. Both entries stay in the registry under their own ids, so `get command` and
    ///   `help` still find them and `<package>:<command>` still runs them; what is refused is the
    ///   bare spelling, because install order is not a resolution policy.
    ///
    /// A refusal is returned, never swallowed: nothing here shadows anything silently.
    #[must_use]
    pub fn extended(&self, contributions: Vec<CommandContract>) -> (Self, Vec<ErrorValue>) {
        let mut extended = self.clone();
        let mut refusals = Vec::new();
        let mut accepted: Vec<CommandContract> = Vec::new();
        for contribution in contributions {
            if let Some(existing) = self.get(contribution.id()) {
                refusals.push(shadow_error(&contribution, existing.origin()));
                continue;
            }
            if let Some(clash) = accepted
                .iter()
                .find(|other| other.id() == contribution.id())
            {
                refusals.push(shadow_error(&contribution, clash.origin()));
                continue;
            }
            accepted.push(contribution);
        }

        // A spelling every package but one wants is a spelling no package gets (spec §31.65).
        let mut contested: BTreeMap<(String, Option<String>), usize> = BTreeMap::new();
        for contribution in &accepted {
            *contested
                .entry((
                    contribution.verb().to_owned(),
                    contribution.target().map(str::to_owned),
                ))
                .or_default() += 1;
        }

        for contribution in accepted {
            let spelling = (
                contribution.verb().to_owned(),
                contribution.target().map(str::to_owned),
            );
            let index = extended.commands.len();
            let bare = if let Some(existing) = self.by_spelling.get(&spelling) {
                refusals.push(shadow_error(
                    &contribution,
                    extended.commands[*existing].origin(),
                ));
                continue;
            } else if contested.get(&spelling).copied().unwrap_or_default() > 1 {
                refusals.push(contested_error(&contribution));
                false
            } else {
                true
            };
            extended.by_id.insert(contribution.id().to_owned(), index);
            if bare {
                extended.by_spelling.insert(spelling, index);
            }
            extended.commands.push(contribution);
        }
        (extended, refusals)
    }

    /// Every command, in the order the contract files declare them.
    #[must_use]
    pub fn commands(&self) -> &[CommandContract] {
        &self.commands
    }

    /// How many commands the registry holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether the registry holds no commands at all, which only a broken build produces.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// A command by its stable id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&CommandContract> {
        self.by_id.get(id).map(|index| &self.commands[*index])
    }

    /// A command by the way it is written: a verb, and the target word where it takes one.
    #[must_use]
    pub fn find(&self, verb: &str, target: Option<&str>) -> Option<&CommandContract> {
        self.by_spelling
            .get(&(verb.to_owned(), target.map(str::to_owned)))
            .map(|index| &self.commands[*index])
    }

    /// Every command of one verb, in declaration order.
    #[must_use]
    pub fn by_verb(&self, verb: &str) -> Vec<&CommandContract> {
        self.commands
            .iter()
            .filter(|command| command.verb() == verb)
            .collect()
    }

    /// Every command of one target, in declaration order.
    #[must_use]
    pub fn by_target(&self, target: &str) -> Vec<&CommandContract> {
        self.commands
            .iter()
            .filter(|command| command.target() == Some(target))
            .collect()
    }

    /// Every command of one stability level.
    #[must_use]
    pub fn with_stability(&self, stability: Stability) -> Vec<&CommandContract> {
        self.commands
            .iter()
            .filter(|command| command.stability() == stability)
            .collect()
    }

    /// The target words a verb can be followed by, sorted, without repeats.
    #[must_use]
    pub fn targets_for_verb(&self, verb: &str) -> Vec<&str> {
        let mut targets: Vec<&str> = self
            .commands
            .iter()
            .filter(|command| command.verb() == verb)
            .filter_map(CommandContract::target)
            .collect();
        targets.sort_unstable();
        targets.dedup();
        targets
    }

    /// The verb registry of `docs/spec/verbs.yaml`.
    #[must_use]
    pub fn verbs(&self) -> &[VerbSpec] {
        &self.verbs
    }

    /// One verb by the word the user types.
    #[must_use]
    pub fn verb(&self, verb: &str) -> Option<&VerbSpec> {
        self.verbs.iter().find(|entry| entry.verb() == verb)
    }

    /// The target registry of `docs/spec/targets.yaml`.
    #[must_use]
    pub fn targets(&self) -> &[TargetSpec] {
        &self.targets
    }

    /// One target by name.
    #[must_use]
    pub fn target(&self, name: &str) -> Option<&TargetSpec> {
        self.targets.iter().find(|entry| entry.name() == name)
    }

    /// The provider capability registry of `docs/spec/capabilities.yaml`.
    #[must_use]
    pub fn capabilities(&self) -> &[CapabilitySpec] {
        &self.capabilities
    }

    /// One provider capability by id.
    #[must_use]
    pub fn capability(&self, id: &str) -> Option<&CapabilitySpec> {
        self.capabilities.iter().find(|entry| entry.id() == id)
    }

    /// Resolves a stage head against the registry, consuming the target word when the command
    /// takes one.
    ///
    /// This is step 4 of ADR-0011's resolution order. A head that names no native command is
    /// reported rather than guessed at, so the caller can go on to look for an executable on
    /// `PATH`.
    ///
    /// # Errors
    ///
    /// `resolve.target_not_found` when the verb is known but the target word is not, and
    /// `resolve.command_not_found` when nothing in the registry answers to the head. Both carry
    /// the near misses spec §15.4 asks for.
    pub fn resolve<'r, 'a>(
        &'r self,
        head: &str,
        arguments: &'a [Argument],
    ) -> Result<Resolved<'r, 'a>, ErrorValue> {
        let first_word = arguments.first().and_then(Argument::as_word);
        if let Some(word) = first_word
            && let Some(contract) = self.find(head, Some(word))
        {
            return Ok(Resolved {
                contract,
                arguments: &arguments[1..],
            });
        }
        if let Some(contract) = self.find(head, None) {
            return Ok(Resolved {
                contract,
                arguments,
            });
        }

        let targets = self.targets_for_verb(head);
        if !targets.is_empty() {
            let given = first_word.unwrap_or("");
            let mut error = ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("`{head}` has no target `{given}`"),
            );
            if let Some(near) = closest(given, targets.iter().copied()) {
                error = error.with_help(format!("did you mean `{head} {near}`?"));
            } else {
                error = error.with_help(format!("`help {head}` lists its targets"));
            }
            return Err(error);
        }

        let mut error = ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            format!("no native command answers to `{head}`"),
        );
        if let Some(near) = closest(head, self.verbs.iter().map(VerbSpec::verb)) {
            error = error.with_help(format!("did you mean `{near}`?"));
        }
        Err(error)
    }

    /// The stable commands nothing has registered an implementation for.
    ///
    /// Spec §27.2 requires CI to verify that every stable registry command has an
    /// implementation; this is the question it asks. `is_bound` reports whether an id has one, so
    /// the check can be run against a command table or against any other list of ids.
    #[must_use]
    pub fn unbound_stable_ids(&self, is_bound: impl Fn(&str) -> bool) -> Vec<&str> {
        self.commands
            .iter()
            .filter(|command| command.stability() == Stability::Stable)
            .map(CommandContract::id)
            .filter(|id| !is_bound(id))
            .collect()
    }
}

fn yaml_error(error: serde_yaml_ng::Error) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderSchemaViolation,
        format!("an embedded contract file is not valid YAML: {error}"),
    )
}

/// A contribution that would have taken a name something else already holds (spec §31.65).
fn shadow_error(contribution: &CommandContract, held_by: &crate::contract::Origin) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::KuangPackageInvalid,
        format!(
            "{} contributes `{}`, which would shadow the one {held_by} already provides",
            contribution.origin(),
            contribution.spelling()
        ),
    )
    .with_help(
        "a contribution never replaces a name the shell already answers to (spec §31.65); `<package>:<command>` runs it under its own name",
    )
}

/// Two packages claiming one spelling, which neither of them then gets (spec §31.65).
fn contested_error(contribution: &CommandContract) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ResolveAmbiguous,
        format!(
            "`{}` is claimed by more than one package, so it names none of them",
            contribution.spelling()
        ),
    )
    .with_help(format!(
        "write `{}:{}` for this one (spec §31.66)",
        contribution.origin().package().unwrap_or_default(),
        contribution
            .id()
            .rsplit('.')
            .next()
            .unwrap_or(contribution.id())
    ))
}
