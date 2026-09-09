//! Which providers this shell can ask, and the runtime they run on.
//!
//! Both are built on first use rather than at startup. A shell that runs `echo hi` should not
//! have paid for a thread pool and a netlink socket to do it, and spec §34's cold-start budget is
//! measured on exactly that command.
//!
//! Every provider is registered behind `TemporalProvider`, the seam v0.5 §4.5 and §9.6 are
//! answered at: a `get <target>` evaluated at a historical coordinate reads the reconstruction,
//! and one asked in the present reads the system, through one decision rather than one per verb
//! (ADR-0780).

use std::sync::Arc;

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_pipeline::ValueStream;
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, EventStream, ObjectRef, Provider,
    ProviderRegistry, Query, Selector, TemporalCapabilities, TimeWindow,
};
use ono_spatial_core::{SpatialScope, SpatialType, types_of_target};
use ono_temporal_core::LedgerRead;
use ono_temporal_reconstruct::{
    ReconstructedCollection, ReconstructedWorld, ReconstructionRequest, Reconstructor,
    SourceMatrix, attach_temporal,
};
use ono_value::{ErrorValue, MapValue, Value};

/// Every provider this build knows about, in the order they are asked.
///
/// Registration order decides which provider answers a target that two claim, which is how a
/// KUANG/11 package will later extend a target without displacing what is already there
/// (spec §31.23).
pub fn registry(environment: impl IntoIterator<Item = (String, String)>) -> ProviderRegistry {
    let environment: Vec<(String, String)> = environment.into_iter().collect();
    let env = Arc::new(ono_provider_linux::EnvProvider::new(
        environment
            .iter()
            .map(|(name, value)| ono_provider_linux::EnvBinding::inherited(name, value)),
    ));
    registry_with_tables(environment, Arc::default(), env)
}

/// The same registry, with the shell's own tables (`ono.shell`) answering from `tables` — the
/// job and link tables the session publishes before each pipeline runs (spec §18.4, §21;
/// ADR-0090, ADR-0103), the host sources the environment points at, and `env` answering `get env`
/// from the bindings the session publishes to it.
pub fn registry_with_tables(
    environment: impl IntoIterator<Item = (String, String)>,
    tables: Arc<std::sync::Mutex<crate::session_provider::SessionTables>>,
    env: Arc<ono_provider_linux::EnvProvider>,
) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    let environment: Vec<(String, String)> = environment.into_iter().collect();
    // The host sources of spec §9.1 are where the environment says they are (ADR-0103).
    let sources = crate::hosts::HostSources::from_environment(
        environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str())),
    );

    ono_provider_linux::register_with_env(&mut registry, env);
    // The container runtime is found the way `docker` and `podman` find it: through
    // DOCKER_HOST / CONTAINER_HOST, or the well-known sockets (ADR-0112).
    registry.register(Arc::new(
        ono_provider_container::ContainerProvider::from_environment(
            environment
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        ),
    ));

    registry.register(Arc::new(ono_provider_netlink::InterfaceProvider::new()));
    registry.register(Arc::new(ono_provider_netlink::RouteProvider::new()));
    registry.register(Arc::new(ono_provider_netlink::NeighborProvider::new()));
    registry.register(Arc::new(ono_provider_netlink::SocketProvider::new()));
    registry.register(Arc::new(ono_provider_systemd::JournalProvider::new()));

    registry.register(Arc::new(crate::session_provider::SessionProvider::new(
        tables, sources,
    )));
    registry.register(Arc::new(ono_provider_net::DnsProvider::new()));
    registry.register(Arc::new(ono_provider_net::PortProvider::new()));

    // v0.5 §4.5, §9.6: every provider is asked through the temporal seam, so `get <target>` at a
    // historical coordinate reads the reconstruction and never the live system (§55.2). The
    // wrapping happens here rather than at each `register` because `register_with_env` mounts
    // several providers of its own, and a seam with a hole in it is not one (ADR-0780).
    through_time(&registry)
}

/// The same providers, each behind the temporal seam of [`TemporalProvider`].
fn through_time(registry: &ProviderRegistry) -> ProviderRegistry {
    let mut wrapped = ProviderRegistry::new();
    for provider in registry.providers() {
        wrapped.register(Arc::new(TemporalProvider::new(Arc::clone(provider))));
    }
    wrapped
}

/// Adds the providers that have to be reached asynchronously.
///
/// systemd is behind D-Bus, so building its provider is an `await`. It is registered separately
/// rather than being made synchronous, because pretending an I/O-bound constructor is not one is
/// how a shell acquires a hang at startup.
pub async fn register_async(registry: &mut ProviderRegistry) {
    // Two connections to the same bus, each a handshake and a round trip to its manager, and
    // neither waiting on the other: opened side by side, they cost one of them (spec §34).
    // Registration order is what decides which provider answers a target (see `registry`), and
    // it is kept.
    let (systemd, logind) = tokio::join!(
        ono_provider_systemd::SystemdProvider::connect(),
        ono_provider_systemd::SessionProvider::connect(),
    );
    registry.register(Arc::new(TemporalProvider::new(Arc::new(systemd))));
    registry.register(Arc::new(TemporalProvider::new(Arc::new(logind))));
}

/// One provider, asked at the session's temporal coordinate rather than only in the present
/// (spec v0.5 §4.5, §9.6, §55.2, §55.9).
///
/// §55.9 is the rule this type exists for: "If `at -10m` changes the prompt but `look`/`map`
/// still show present objects, the feature is invalid." The spatial commands already answer from
/// [`crate::spatial::HistoricalWorld`]; `get <target>` reached the provider registry directly, so
/// the coordinate had to be honoured where the registry is asked. Wrapping the providers rather
/// than teaching `ProviderProducer` about time is what keeps §4.5's "MUST NOT implement a
/// separate historical code path" true: there is one seam, and both spellings of the coordinate
/// pass through it.
///
/// Everything except [`Provider::snapshot`] is delegated untouched. The provider's identity, its
/// targets, its capabilities and its declared temporal reach are the inner provider's, so the
/// registry, `spec-check` and the conformance suites see exactly the surface they saw before.
#[derive(Debug)]
struct TemporalProvider {
    inner: Arc<dyn Provider>,
}

impl TemporalProvider {
    const fn new(inner: Arc<dyn Provider>) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl Provider for TemporalProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn targets(&self) -> &[&str] {
        self.inner.targets()
    }

    fn identity_token(&self) -> Option<&str> {
        self.inner.identity_token()
    }

    fn schemas(&self) -> Vec<Arc<ono_value::Schema>> {
        self.inner.schemas()
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.inner.capabilities()
    }

    fn availability(&self) -> Availability {
        self.inner.availability()
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        match coordinate_of(query)? {
            Some(past) => reconstructed(query, past.at, past.ledger.as_ref()),
            None => self.inner.snapshot(query),
        }
    }

    fn subscribe(&self, query: &Query) -> Result<EventStream, ErrorValue> {
        self.inner.subscribe(query)
    }

    fn temporal(&self) -> TemporalCapabilities {
        self.inner.temporal()
    }

    fn history(&self, query: &Query, window: &TimeWindow) -> Result<ValueStream, ErrorValue> {
        self.inner.history(query, window)
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        self.inner.resolve(selector).await
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        self.inner.act(action).await
    }
}

/// A coordinate in the past, and the ledger that can answer about it.
struct PastQuery {
    at: Timestamp,
    ledger: Arc<dyn LedgerRead>,
}

/// Where in time this query evaluates, or `None` for the present (§4.1, §4.5).
///
/// Two spellings, one engine. `--at` is resolved through `crate::temporal::coordinate::resolve` —
/// the same function `at` itself calls — and the session's own coordinate is read through the
/// evidence the temporal session installs, which is the one place §4 keeps it. Neither of them
/// parses a selector or composes coverage here; §55.7 keeps that in the temporal crates.
///
/// The interception covers the two verbs that enumerate a class of objects — `get`, which §9.6
/// legislates, and `find`, which walks a source to build the same kind of set. `resolve dns`,
/// `test port`, `test host` and `tail journal` probe a live system rather than enumerating a
/// state, and §4.8 rather than §9.6 is what governs those; they are left as they were (ADR-0780).
fn coordinate_of(query: &Query) -> Result<Option<PastQuery>, ErrorValue> {
    if !matches!(query.verb(), "get" | "find") {
        return Ok(None);
    }
    if let Some(text) = option_text(query.option_value("at")) {
        // §12.1: the selector is resolved before anything is answered, so an unreadable one is a
        // refusal rather than a differently-timed answer.
        let state = crate::temporal::session::session_state()
            .try_lock()
            .map_err(|_| {
                ErrorValue::new(
                    ErrorCode::TemporalStoreUnavailable,
                    "the session's temporal coordinate is busy and `--at` could not be resolved",
                )
                .with_help("run the command again; nothing was answered from the present")
            })?;
        let context = crate::temporal::coordinate::resolve(&state, &text, Timestamp::now())?;
        return Ok(context.instant().map(|at| PastQuery {
            at,
            ledger: crate::temporal::session::ledger_handle(),
        }));
    }
    // §4.2: with no `--at`, the session's coordinate is the command's, and a session in the
    // present is every v0.2–v0.4 behaviour unchanged.
    Ok(
        crate::spatial::historical::active().map(|active| PastQuery {
            at: active.at(),
            ledger: active.ledger_handle(),
        }),
    )
}

/// The text of an `--at` option, or `None` where it was not written.
fn option_text(value: Option<&Value>) -> Option<String> {
    let text = match value? {
        Value::Null => return None,
        Value::String(text) => text.to_string(),
        other => ono_value::canonical_text(other).ok()?,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// The objects of `query`'s target as reconstruction supports them at `at` (§9.1, §9.6).
///
/// The rows come from [`ReconstructedWorld`] and from nowhere else — there is no provider, no
/// index and no live registry reachable from this function — which is what makes §55.2's
/// prohibited "today's graph with an old timestamp" unreachable rather than merely avoided.
///
/// # Errors
///
/// `temporal.unsupported_source` for a target no source reconstructs, `temporal.not_recorded`
/// where nothing covers the class at `at`, and whatever §34 refusal the ledger raises.
fn reconstructed(
    query: &Query,
    at: Timestamp,
    ledger: &dyn LedgerRead,
) -> Result<ValueStream, ErrorValue> {
    let target = query.target_name();
    let types = types_of_target(target);
    if types.is_empty() {
        return Err(unsupported_target(target, at));
    }
    let scope = crate::spatial::local_scope();
    // §21.1: a source answers about the past only where it said it can. Nothing has declared
    // anything here, so §14.5's four supports are absent and a filesystem tree is refused — the
    // same matrix `crate::spatial::HistoricalWorld` reconstructs against.
    let sources = SourceMatrix::new();
    let world = Reconstructor::new(ledger)
        .with_sources(&sources)
        .reconstruct(&ReconstructionRequest::new(scope.clone(), at))?;

    let mut rows = Vec::new();
    let mut proven = false;
    for object_type in types {
        let collection = world.collection(*object_type);
        proven |= collection.is_enumeration_proven();
        let metadata = collection_metadata(&world, &collection)?;
        for id in collection.members() {
            let Some(record) = world.object(id).and_then(|object| object.record()) else {
                continue;
            };
            // The selectors of `get process 4419` narrow the reconstructed set the same way they
            // narrow a live one, so one spelling of the command means one thing at both
            // coordinates (§28.2).
            if !query.matches(record) {
                continue;
            }
            rows.push(Value::Record(Arc::new(attach_temporal(
                record,
                metadata.clone(),
            )?)));
        }
    }

    // §8.2 and §55.5: "there were none" is a claim only complete coverage can support. With
    // nothing found and no proof that the enumeration is complete, an empty list would be the
    // silent gap §55.5 forbids, so the answer is the refusal §12.3 already words.
    if rows.is_empty() && !proven {
        return Err(nothing_reconstructs(target, types, at, &scope));
    }
    Ok(ValueStream::from_values(rows))
}

/// §9.4's temporal metadata for one row, carrying §9.6's collection-level coverage.
///
/// ADR-0613 fixes where it rides — the reserved `ono.temporal` extension key — and §9.6 adds the
/// `collection` member to it: the capability the set rests on, what coverage composed to over the
/// window, whether the enumeration could be proven complete, and the gaps that qualify it. A
/// reader that sees `enumeration_proven: false` has been told the rows are not the whole list,
/// which is the sentence §9.6 makes a MUST.
fn collection_metadata(
    world: &ReconstructedWorld,
    collection: &ReconstructedCollection,
) -> Result<Value, ErrorValue> {
    let mut described = MapValue::new();
    described.insert(
        "object_type".into(),
        Value::string(collection.object_type().as_str()),
    );
    described.insert("capability".into(), Value::string(collection.capability()));
    described.insert(
        "completeness".into(),
        Value::string(collection.completeness().as_str()),
    );
    described.insert(
        "enumeration_proven".into(),
        Value::Bool(collection.is_enumeration_proven()),
    );
    described.insert(
        "members".into(),
        Value::Int(i128::try_from(collection.len()).unwrap_or(i128::MAX)),
    );
    let mut gaps = Vec::with_capacity(collection.gaps().len());
    for gap in collection.gaps() {
        gaps.push(Value::Record(Arc::new(
            ono_temporal_core::value::gap_record(gap)?,
        )));
    }
    described.insert("gaps".into(), Value::list(gaps));

    let metadata = world.temporal_metadata()?;
    let Value::Map(map) = metadata else {
        return Ok(Value::Map(Arc::new(described)));
    };
    let mut carried = (*map).clone();
    carried.insert("collection".into(), Value::Map(Arc::new(described)));
    Ok(Value::Map(Arc::new(carried)))
}

/// §34's `temporal.unsupported_source` for a target the reconstruction cannot carry (§21.1).
fn unsupported_target(target: &str, at: Timestamp) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalUnsupportedSource,
        format!("nothing reconstructs `{target}`, so it cannot be asked about {at}"),
    )
    .with_metadata("at", Value::Timestamp(at))
    .with_metadata("target", Value::string(target))
    .with_help(
        "`now` returns to the present, where this target is answered live; \
         `get temporal-source` lists what each source can answer (spec v0.5 §21.1, §34)",
    )
}

/// §12.3's refusal, for a class no source covered at `at`.
///
/// The filesystem tree is kept apart because §14.5 already has a word for it: it is not that
/// nothing was recorded, it is that no source of the four §14.5 names is present at all.
fn nothing_reconstructs(
    target: &str,
    types: &[SpatialType],
    at: Timestamp,
    scope: &SpatialScope,
) -> ErrorValue {
    if types
        .iter()
        .all(|object_type| ono_temporal_reconstruct::is_path_structure(*object_type))
    {
        return ErrorValue::new(
            ErrorCode::TemporalUnsupportedSource,
            format!(
                "historical filesystem structure is not supported here: nothing in this \
                 session's evidence carries `{target}` at {at}"
            ),
        )
        .with_metadata("at", Value::Timestamp(at))
        .with_metadata("target", Value::string(target))
        .with_help(
            "a recorder checkpoint, a filesystem snapshot provider, audit or inotify evidence \
             sufficient for reconstruction, or a KUANG/11 provider can carry it; current \
             directory contents are not the past (spec v0.5 §14.5)",
        );
    }
    let available = match crate::temporal::session::session_state().try_lock() {
        Ok(state) => crate::temporal::coordinate::availability(&state, Timestamp::now()),
        Err(_) => Vec::new(),
    };
    ono_temporal_core::error::not_recorded(scope, at, &available)
        .with_metadata("target", Value::string(target))
        .with_help(format!(
            "no source covers `{target}` at that instant, and an empty list would claim there \
             were none (spec v0.5 §8.2, §9.6, §55.5); `now` returns to the present"
        ))
}
