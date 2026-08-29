//! Completion over what the providers can actually answer (spec §15.1).
//!
//! `ono_command::complete` offers what the registry knows: verbs, targets, options, and the
//! closed sets a contract declares. Everything else is a *value* — the users on this machine, the
//! services of this host — and the registry deliberately knows nothing about those.
//! [`ono_command::ValueCompleter`] is the seam the contracts left for them, and this is the
//! implementation the shell installs in it (ADR-0252).
//!
//! Three rules make it safe to consult a provider from inside the line editor:
//!
//! * **it never blocks the prompt.** The query runs on a thread of its own and the keystroke
//!   waits `BUDGET` for it. Spec §34 gives the first completion 50 ms end to end, so the part
//!   that leaves the process gets less than that; a provider slower than the budget contributes
//!   nothing to *this* keystroke instead of freezing the line.
//! * **what it read is kept.** A target's objects are cached for `FRESH`, so the second Tab and
//!   every one after it answers from memory in under a millisecond. The first Tab for a target
//!   pays the provider's cold cost and may run out of budget before the answer lands — it is then
//!   in the cache, and the next keystroke has it. Warming every target at startup would be the
//!   other way to hide that, and it is exactly the eager work spec §34 and case `027` forbid.
//! * **it never runs a command.** A snapshot of a target is the same read `get <target>` would
//!   perform and nothing else: no mutation, no candidate executed, no side effect (ADR-0245 T4).

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use ono_command::{Candidate, CommandContract, ParameterSpec, ValueCompleter};
use ono_provider_api::{ProviderRegistry, Query};

/// How long a keystroke waits for a provider before answering without it.
const BUDGET: Duration = Duration::from_millis(40);

/// How long what a provider said is good enough to complete from.
///
/// Long enough that a burst of Tabs costs one query, short enough that a user who has just
/// created an account does not have to restart the shell to complete it.
const FRESH: Duration = Duration::from_secs(5);

/// How many objects a completion is willing to read, and how many candidates it offers.
///
/// A host with forty thousand objects under the cursor must not turn one Tab into a full
/// enumeration; the bound is the *query's*, so the provider stops producing rather than the
/// completer stopping reading.
const CEILING: usize = 500;
const OFFERED: usize = 50;

/// Completes a value selector from the providers this build has.
#[derive(Debug, Clone)]
pub struct ProviderValues {
    environment: Vec<(String, String)>,
}

impl ProviderValues {
    /// A completer that asks the providers the given environment configures (ADR-0103, ADR-0112).
    ///
    /// Building the registry means constructing every provider, and two of them look for
    /// something on the machine. That happens here, on a thread of its own, so no keystroke pays
    /// for provider discovery inside a budget meant for the query.
    #[must_use]
    pub fn new(environment: Vec<(String, String)>) -> Self {
        let warm = environment.clone();
        std::thread::spawn(move || {
            let _ = providers(&warm);
            // The first provider read in a process is far more expensive than the rest: it pays
            // for the async runtime and for whatever the C library loads the first time an
            // account database is consulted. That cost is process-wide, not target-specific, so
            // paying it once here — on the cheapest purely local read there is — is what makes
            // the *first* Tab of any target as fast as the ones after it.
            let _ = read(&warm, "user", &["name".to_owned()]);
        });
        Self { environment }
    }
}

impl ValueCompleter for ProviderValues {
    fn complete(
        &self,
        command: &CommandContract,
        parameter: &ParameterSpec,
        prefix: &str,
    ) -> Vec<Candidate> {
        // A selector of a target-less transform names a field, not an object; that is the
        // schema's business and the shell's `SelectorCompleter` answers it from the contracts.
        let Some(target) = command.target() else {
            return Vec::new();
        };
        // Every selector of the command, not only the one the *position* would bind: the binder
        // resolves a positional word by type, so `get user 0` is a uid and `get user root` is a
        // name, and completion has to be able to offer both. Offering only the first selector's
        // field would answer `get user ro<TAB>` with uids, which is the one answer that is wrong.
        let mut fields: Vec<String> = command
            .selectors()
            .iter()
            .map(|selector| selector.name().to_owned())
            .collect();
        if fields.is_empty() {
            fields.push(parameter.name().to_owned());
        }

        let known = objects(&self.environment, target, &fields);
        known
            .into_iter()
            .filter(|value| value.starts_with(prefix))
            .take(OFFERED)
            .map(|value| Candidate::value(value).with_doc(parameter.doc()))
            .collect()
    }
}

/// What is known about `target`'s objects: from the cache when it is fresh, otherwise from the
/// providers, within the budget.
fn objects(environment: &[(String, String)], target: &str, fields: &[String]) -> Vec<String> {
    if let Some(cached) = cached(target) {
        return cached;
    }

    let (answer, wait) = std::sync::mpsc::channel();
    let environment = environment.to_vec();
    let owned_target = target.to_owned();
    let owned_fields = fields.to_vec();
    // Detached on purpose: if the provider is slower than the budget the keystroke is already
    // answered, and the thread ends by itself when the query does — leaving what it read in the
    // cache, which is what makes the next keystroke instant.
    std::thread::spawn(move || {
        let values = read(&environment, &owned_target, &owned_fields);
        remember(&owned_target, &values);
        let _ = answer.send(values);
    });

    wait.recv_timeout(BUDGET).unwrap_or_default()
}

/// The values of `fields` that the providers for `target` report.
fn read(environment: &[(String, String)], target: &str, fields: &[String]) -> Vec<String> {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return Vec::new();
    };
    let registry = providers(environment);
    runtime.block_on(async {
        let mut found: Vec<String> = Vec::new();
        for provider in registry.for_target(target) {
            if !provider.availability().is_available() {
                continue;
            }
            let Ok(stream) = provider.snapshot(&Query::target(target).limit(CEILING)) else {
                continue;
            };
            for value in stream.collect().await.into_values() {
                let Ok(record) = value.as_record() else {
                    continue;
                };
                for field in fields {
                    if let Some(text) = record.get(field).and_then(readable) {
                        found.push(text);
                    }
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        found
    })
}

/// What a provider last said about a target, while it is still fresh.
type Cache = Mutex<BTreeMap<String, (Instant, Vec<String>)>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn cached(target: &str) -> Option<Vec<String>> {
    let held = cache().lock().ok()?;
    let (read_at, values) = held.get(target)?;
    (read_at.elapsed() < FRESH).then(|| values.clone())
}

fn remember(target: &str, values: &[String]) {
    if let Ok(mut held) = cache().lock() {
        held.insert(target.to_owned(), (Instant::now(), values.to_vec()));
    }
}

/// The providers completion asks, built once.
///
/// Building a registry constructs every provider, and two of them look for something on the
/// machine — a container socket, a host-source file (ADR-0103, ADR-0112). Doing that per
/// keystroke would be paying for discovery over and over. This is the *synchronous* registry:
/// systemd and the login-session provider are reached with an `await` and registered separately
/// at startup, so no completion can end up waiting on a D-Bus round trip.
fn providers(environment: &[(String, String)]) -> &'static ProviderRegistry {
    static REGISTRY: OnceLock<ProviderRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| crate::providers::registry(environment.to_vec()))
}

/// How a value reads as a completion candidate, where it reads as one at all.
///
/// A name, a path or a number can be typed back into the line; a record, a list or an error
/// cannot, and offering their debug shape would be offering a line that does not parse.
fn readable(value: &ono_value::Value) -> Option<String> {
    match value {
        ono_value::Value::String(text) => Some(text.to_string()),
        ono_value::Value::Int(number) => Some(number.to_string()),
        ono_value::Value::Path(path) => Some(path.display().to_string()),
        _ => None,
    }
}
