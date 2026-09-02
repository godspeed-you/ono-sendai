//! Who a listening agent lets in, and what it lets them do (v0.4.1 §9, §10).
//!
//! [`crate::trust`] answers a different question. It decides whether the key a peer proved it
//! holds is the key that peer had last time — identity, not permission. §9.1 states the gap this
//! module closes in one sentence: "a valid client certificate proves only that the connecting
//! process holds a private key. It does not prove that the agent operator wants to expose system
//! data or actions to that key."
//!
//! # The store
//!
//! [`AuthorizedClients`] is the `authorized_clients` file of §9.2 — line-oriented, human-readable
//! and **strictly parsed**. One line is one client:
//!
//! ```text
//! # ono authorized clients: one line per client key.
//! sha256:1f0c… observe=true
//! sha256:9ab4… observe=true actions=process.signal,service.manage label=deploy
//! ```
//!
//! Three properties are not conveniences and must not be softened:
//!
//! - **A malformed line fails the whole load.** §9.2 forbids treating a malformed store as empty
//!   and forbids falling back to permissive access; §65.2 and §65.4 name the fail-open variants
//!   as failure modes with names. So the parser returns
//!   [`ErrorCode::RemoteAuthorizationStoreInvalid`] naming the file and the line, and the agent
//!   that cannot load its policy does not listen.
//! - **A missing store and an unreadable store are different conditions.** Both authorize nobody;
//!   they are told apart in the refusal, because "you are not on the list" and "there is no list"
//!   send an operator to different places.
//! - **An action grant is an exact capability id.** [`ActionGrant`] cannot hold `*`,
//!   `process.*`, a risk class or a prefix — not because those are validated away, but because
//!   the grammar of the type has no room for them (§9.5, and the shape of `Budget` in ADR-0453).
//!
//! # The context
//!
//! [`AuthorizationContext`] is §10.3: built once, immediately after the transport authenticated
//! the peer, and immutable for the life of the connection. It has no `&mut self` method and no
//! setter, so a request handler cannot widen the policy it was handed, and nothing re-reads the
//! file per request. Authorization changes therefore reach the next connection, never the one
//! already running — a running connection's *grant* is fixed. Whether the connection continues to
//! exist is a different question, and §12.5's answer to it lives in `ono-remote`'s listening
//! agent: removing a client key ends the sessions that key holds (ADR-0505).

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use ono_core::ErrorCode;
use ono_value::{ErrorValue, Value};

use crate::trust::Fingerprint;

/// One capability id an operator granted a client, and nothing that stands for several.
///
/// The grammar is the one `docs/spec/capabilities.yaml` uses for its provider capabilities:
/// lowercase segments of letters, digits and dashes, separated by dots, at least two of them.
/// `*`, `process.*`, `**`, `mutate`, `process.` and the empty string are all outside it, so a
/// wildcard grant cannot be constructed, stored, parsed or compared — §9.5's "wildcards MUST NOT
/// be the storage default" is met by there being no wildcard to store.
///
/// ```
/// use ono_protocol::ActionGrant;
///
/// assert!("process.signal".parse::<ActionGrant>().is_ok());
/// assert!("process.*".parse::<ActionGrant>().is_err());
/// assert!("*".parse::<ActionGrant>().is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionGrant(String);

impl ActionGrant {
    /// The capability id this grant names.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ActionGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ActionGrant {
    type Err = ErrorValue;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let segments: Vec<&str> = text.split('.').collect();
        let well_formed = segments.len() >= 2
            && segments.iter().all(|segment| {
                !segment.is_empty()
                    && segment.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
            });
        if well_formed {
            return Ok(Self(text.to_owned()));
        }
        Err(ErrorValue::new(
            ErrorCode::RemoteAuthorizationStoreInvalid,
            format!("`{text}` is not a capability id, so it cannot be granted"),
        )
        .with_retryable(false)
        .with_help(
            "an action grant names one exact capability from docs/spec/capabilities.yaml, such \
             as `process.signal` (v0.4.1 section 9.5). A pattern, a risk class or `*` is not a \
             grant: a capability added in a later version would then be authorized by something \
             written before it existed.",
        ))
    }
}

/// One client an operator authorized, as §9.3 models it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedClient {
    fingerprint: Fingerprint,
    label: Option<String>,
    observe: bool,
    actions: BTreeSet<ActionGrant>,
}

impl AuthorizedClient {
    /// A client that may observe and do nothing else — the default grant of §9.4.
    #[must_use]
    pub fn observing(fingerprint: Fingerprint) -> Self {
        Self {
            fingerprint,
            label: None,
            observe: true,
            actions: BTreeSet::new(),
        }
    }

    /// Names the client, for the operator's own reading and for the audit trail.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets whether the client may query and subscribe.
    #[must_use]
    pub const fn with_observe(mut self, observe: bool) -> Self {
        self.observe = observe;
        self
    }

    /// Replaces the action allowlist with these exact capability ids.
    #[must_use]
    pub fn with_actions<I: IntoIterator<Item = ActionGrant>>(mut self, actions: I) -> Self {
        self.actions = actions.into_iter().collect();
        self
    }

    /// The key this entry authorizes.
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// What the operator called the client, where they called it anything.
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// Whether the client may query and subscribe.
    #[must_use]
    pub const fn observes(&self) -> bool {
        self.observe
    }

    /// The exact capability ids the client may act with, in id order.
    #[must_use]
    pub fn actions(&self) -> &BTreeSet<ActionGrant> {
        &self.actions
    }
}

/// The `authorized_clients` store of §9.2: the clients an operator listed, and where they listed
/// them.
///
/// A store that is *absent* holds no entries and says so ([`is_present`](Self::is_present)); a
/// store that is *malformed* is never one of these values at all, because [`open`](Self::open)
/// returns the refusal instead.
#[derive(Debug, Clone, Default)]
pub struct AuthorizedClients {
    entries: Vec<AuthorizedClient>,
    path: Option<PathBuf>,
    present: bool,
}

/// The header written above the entries, so a person opening the file knows what it is.
const FILE_HEADER: &str = "\
# ono authorized clients: one line per client key, strictly parsed (v0.4.1 section 9.2).
# `<sha256 fingerprint> observe=<true|false> [actions=<id>,<id>…] [label=<name>]`
# An action grant names one exact capability id; there is no wildcard and no risk class.
# A malformed line refuses the whole file: this store is never read as an empty one.
";

impl AuthorizedClients {
    /// A store that authorizes nobody and is kept nowhere.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// A store holding `entries`, for a caller that assembled them itself.
    #[must_use]
    pub fn of<I: IntoIterator<Item = AuthorizedClient>>(entries: I) -> Self {
        Self {
            entries: entries.into_iter().collect(),
            path: None,
            present: true,
        }
    }

    /// The store recorded in `path`.
    ///
    /// A file that is not there authorizes nobody, which is the same answer as an empty file and
    /// a different *condition*: [`is_present`](Self::is_present) tells them apart, and the
    /// refusal a client receives says which one it met.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::RemoteAuthorizationStoreInvalid`] when the file exists and cannot be read or
    /// does not parse. Never an empty store: §9.2 forbids reading a malformed store as one.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ErrorValue> {
        let path = path.as_ref().to_path_buf();
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Self {
                entries: parse(&text, &path)?,
                path: Some(path),
                present: true,
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                entries: Vec::new(),
                path: Some(path),
                present: false,
            }),
            Err(error) => Err(ErrorValue::new(
                ErrorCode::RemoteAuthorizationStoreInvalid,
                format!(
                    "the authorization store at {} could not be read: {error}",
                    path.display()
                ),
            )
            .with_retryable(false)
            .with_help(
                "a listening agent that cannot read its policy authorizes nobody and does not \
                 fall back to permissive access (v0.4.1 section 9.2)",
            )),
        }
    }

    /// Whether a file exists at the store's path.
    #[must_use]
    pub const fn is_present(&self) -> bool {
        self.present
    }

    /// Where the store is kept, when it is kept anywhere.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Every authorized client, in the order the file lists them.
    #[must_use]
    pub fn entries(&self) -> &[AuthorizedClient] {
        &self.entries
    }

    /// The entry for `fingerprint`, if the operator listed it.
    #[must_use]
    pub fn client(&self, fingerprint: Fingerprint) -> Option<&AuthorizedClient> {
        self.entries
            .iter()
            .find(|entry| entry.fingerprint == fingerprint)
    }

    /// The policy for one authenticated connection, or the refusal to send instead.
    ///
    /// This is the only way an [`AuthorizationContext`] is made from a store, so §2.2's order —
    /// proof, then trust, then policy, then negotiation — holds by construction: the fingerprint
    /// argument can only have come from a transport that verified it.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::RemoteUnauthorized`] when the client is not listed. The message says whether
    /// there is a store at all, because "not on the list" and "there is no list" are different
    /// things for the operator to fix.
    pub fn authorize(&self, fingerprint: Fingerprint) -> Result<AuthorizationContext, ErrorValue> {
        let Some(entry) = self.client(fingerprint) else {
            return Err(self.unlisted(fingerprint));
        };
        Ok(AuthorizationContext::of(entry))
    }

    /// The refusal an unlisted client receives.
    ///
    /// The two conditions §9.2 keeps apart are in the sentence — "is not authorized" against "and
    /// no client is" — and the store's *path* is deliberately not: it is a directory name on the
    /// agent's host, and §59.1 wants a refused client to learn as little about the machine as the
    /// refusal allows. The path is in the operator's own diagnostic and in the audit trail, which
    /// is where somebody who can fix it is reading.
    fn unlisted(&self, fingerprint: Fingerprint) -> ErrorValue {
        let condition = if self.present {
            "is authenticated and is not authorized on this host"
        } else {
            "is authenticated and is not authorized on this host, where no client is authorized yet"
        };
        ErrorValue::new(
            ErrorCode::RemoteUnauthorized,
            format!("the client {fingerprint} {condition}"),
        )
        .with_retryable(false)
        .with_metadata("peer_fingerprint", Value::string(&fingerprint.to_string()))
        .with_metadata("store_present", Value::Bool(self.present))
        .with_help(format!(
            "authentication proves which key connected, never that the operator wants it here \
             (v0.4.1 section 9.1). On the agent's host, `add client-key {fingerprint}` authorizes \
             this client to observe and nothing else."
        ))
    }
}

/// The refusal a connection meets when the transport authenticated nobody (§10.4, §2.2).
#[must_use]
pub fn unauthenticated_refusal() -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RemoteUnauthenticated,
        "the client proved possession of no key, so there is no identity to authorize",
    )
    .with_retryable(false)
    .with_help(
        "authorization follows authentication and never replaces it (v0.4.1 section 2.2). A \
         direct link presents a peer identity on both ends.",
    )
}

/// Parses the store, refusing the whole file when any non-comment line is not an entry.
fn parse(text: &str, path: &Path) -> Result<Vec<AuthorizedClient>, ErrorValue> {
    let mut entries: Vec<AuthorizedClient> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let entry = parse_entry(line).map_err(|detail| malformed(path, index + 1, &detail))?;
        if entries
            .iter()
            .any(|existing| existing.fingerprint == entry.fingerprint)
        {
            return Err(malformed(
                path,
                index + 1,
                "this fingerprint is already listed above, and two grants for one key would \
                 leave which of them applies to the order of the file",
            ));
        }
        entries.push(entry);
    }
    Ok(entries)
}

fn parse_entry(line: &str) -> Result<AuthorizedClient, String> {
    let mut fields = line.split_whitespace();
    let fingerprint: Fingerprint = fields
        .next()
        .ok_or_else(|| "an entry begins with the client's fingerprint".to_owned())?
        .parse()
        .map_err(|error: ErrorValue| error.message().to_owned())?;

    let mut observe: Option<bool> = None;
    let mut actions: Option<BTreeSet<ActionGrant>> = None;
    let mut label: Option<String> = None;
    for field in fields {
        let Some((name, value)) = field.split_once('=') else {
            return Err(format!(
                "`{field}` is not a `name=value` field; an entry is `<fingerprint> observe=… \
                 [actions=…] [label=…]`"
            ));
        };
        let already = match name {
            "observe" => {
                let parsed = match value {
                    "true" => true,
                    "false" => false,
                    other => {
                        return Err(format!("`observe={other}` is neither `true` nor `false`"));
                    }
                };
                observe.replace(parsed).is_some()
            }
            "actions" => {
                let mut granted = BTreeSet::new();
                // An empty `actions=` is the empty allowlist written out, which a person editing
                // the file by hand will reach for; it grants nothing, exactly as omitting it does.
                for id in value.split(',').filter(|id| !id.is_empty()) {
                    let grant: ActionGrant = id
                        .parse()
                        .map_err(|error: ErrorValue| error.message().to_owned())?;
                    granted.insert(grant);
                }
                actions.replace(granted).is_some()
            }
            "label" => label.replace(value.to_owned()).is_some(),
            other => {
                return Err(format!(
                    "`{other}` is not a field of an authorization entry. The fields are \
                     `observe`, `actions` and `label`; an unknown one is refused rather than \
                     ignored, because a field this build does not understand may be the one that \
                     was meant to restrict something (v0.4.1 section 9.3)."
                ));
            }
        };
        if already {
            return Err(format!("`{name}` is given twice on one entry"));
        }
    }

    let Some(observe) = observe else {
        return Err(
            "an entry says `observe=true` or `observe=false`; leaving it out would make the \
             weakest grant depend on a default nobody wrote down"
                .to_owned(),
        );
    };
    Ok(AuthorizedClient {
        fingerprint,
        label,
        observe,
        actions: actions.unwrap_or_default(),
    })
}

fn malformed(path: &Path, line: usize, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RemoteAuthorizationStoreInvalid,
        format!(
            "{}: line {line} is not an authorization entry — {detail}",
            path.display()
        ),
    )
    .with_retryable(false)
    .with_help(
        "the whole store is refused, and no client is authorized by the lines that did parse: a \
         store read past the line it could not understand is a store that silently grants \
         whatever the rest of the file happens to say (v0.4.1 section 9.2, section 59.5). Repair \
         the line, or remove the file to authorize nobody deliberately.",
    )
}

/// Renders the store as the file records it.
#[must_use]
pub fn render(entries: &[AuthorizedClient]) -> String {
    let mut text = String::from(FILE_HEADER);
    for entry in entries {
        text.push_str(&entry.fingerprint.to_string());
        text.push_str(if entry.observe {
            " observe=true"
        } else {
            " observe=false"
        });
        if !entry.actions.is_empty() {
            let ids: Vec<&str> = entry.actions.iter().map(ActionGrant::id).collect();
            text.push_str(&format!(" actions={}", ids.join(",")));
        }
        if let Some(label) = &entry.label {
            text.push_str(&format!(" label={label}"));
        }
        text.push('\n');
    }
    text
}

/// The policy one connection runs under, decided once and unable to change under it (§10.3).
///
/// There is no constructor that widens a grant, no `&mut self` method and no setter: a handler
/// that holds one of these can read it and nothing else. That is the whole of §10.3's "MUST NOT
/// re-read a mutable authorization file on each individual request", enforced by the type rather
/// than by everyone remembering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationContext {
    peer_fingerprint: Fingerprint,
    client_label: Option<String>,
    observe_allowed: bool,
    allowed_action_capabilities: BTreeSet<ActionGrant>,
    connection_id: String,
    connected_at: jiff::Timestamp,
}

/// Connection ids are unique within one agent process, which is the scope an audit trail
/// correlates over.
static CONNECTIONS: AtomicU64 = AtomicU64::new(1);

impl AuthorizationContext {
    /// The context for a connection by `entry`'s client.
    #[must_use]
    pub fn of(entry: &AuthorizedClient) -> Self {
        Self {
            peer_fingerprint: entry.fingerprint,
            client_label: entry.label.clone(),
            observe_allowed: entry.observe,
            allowed_action_capabilities: entry.actions.clone(),
            connection_id: format!("conn-{}", CONNECTIONS.fetch_add(1, Ordering::Relaxed)),
            connected_at: jiff::Timestamp::now(),
        }
    }

    /// The key the transport proved this peer holds.
    #[must_use]
    pub const fn peer_fingerprint(&self) -> Fingerprint {
        self.peer_fingerprint
    }

    /// What the operator called this client.
    #[must_use]
    pub fn client_label(&self) -> Option<&str> {
        self.client_label.as_deref()
    }

    /// Whether this connection may query and subscribe.
    #[must_use]
    pub const fn observe_allowed(&self) -> bool {
        self.observe_allowed
    }

    /// The exact capability ids this connection may act with.
    #[must_use]
    pub fn allowed_action_capabilities(&self) -> &BTreeSet<ActionGrant> {
        &self.allowed_action_capabilities
    }

    /// This connection's id, unique within the agent process.
    #[must_use]
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    /// When the context was built, which is when the peer was authenticated.
    #[must_use]
    pub const fn connected_at(&self) -> jiff::Timestamp {
        self.connected_at
    }

    /// Whether `capability` is one of the exact ids granted.
    ///
    /// An id nothing granted is denied, including one this build has never heard of: Appendix C
    /// ends with "for action authorization, an unknown capability ID is always denied".
    #[must_use]
    pub fn allows_action(&self, capability: &str) -> bool {
        self.allowed_action_capabilities
            .iter()
            .any(|grant| grant.id() == capability)
    }
}

/// What a connection is allowed to do, as every dispatch path asks it.
///
/// Two variants, because there are two ways a peer can be admitted and only one of them is a
/// policy this process resolved. Keeping "the carrier decided" as its own word means an
/// [`AuthorizationContext`] never has to be able to say "everything", and a wildcard therefore
/// has nowhere to live (§9.5).
#[derive(Debug, Clone)]
pub enum PeerAuthorization {
    /// Whatever authenticated the byte stream also decided who may use it: the stdio agent of
    /// §4.3, reached through `ssh <host> ono --agent`, where OpenSSH already decided who may run
    /// the command and there is no peer key for a policy to name.
    CarriedByTransport,
    /// The listening agent's own `authorized_clients` policy for this connection (§9.2, §10.3).
    Policy(Arc<AuthorizationContext>),
}

impl PeerAuthorization {
    /// The context, where a policy decided; `None` where the carrier did.
    #[must_use]
    pub fn context(&self) -> Option<&AuthorizationContext> {
        match self {
            Self::CarriedByTransport => None,
            Self::Policy(context) => Some(context),
        }
    }

    /// Whether a query or a subscription is permitted.
    #[must_use]
    pub fn allows_observe(&self) -> bool {
        match self {
            Self::CarriedByTransport => true,
            Self::Policy(context) => context.observe_allowed(),
        }
    }

    /// Whether an action needing `capability` is permitted.
    #[must_use]
    pub fn allows_action(&self, capability: &str) -> bool {
        match self {
            Self::CarriedByTransport => true,
            Self::Policy(context) => context.allows_action(capability),
        }
    }

    /// Refuses a read or a subscription this connection may not make.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::RemoteCapabilityDenied`] when observe access is off, saying so in the
    /// metadata: §10.4 requires the denial to distinguish "observe is off" from "the action
    /// capability is absent", because they are fixed by different commands.
    pub fn require_observe(&self, what: &str) -> Result<(), ErrorValue> {
        if self.allows_observe() {
            return Ok(());
        }
        Err(self
            .denial(format!(
                "this client is authorized, but not to observe, so `{what}` is refused"
            ))
            .with_metadata("denied_because", Value::string("observe_not_allowed"))
            .with_help(
                "on the agent's host, `set client-key <fingerprint> --observe true` grants query \
                 and subscription access (v0.4.1 section 9.7)",
            ))
    }

    /// Refuses an action this connection may not perform.
    ///
    /// `capability` is `None` when the serving side cannot resolve which capability the request
    /// needs, and that is a denial too: Appendix C denies an unknown capability id always, so a
    /// request the agent cannot classify is a request it does not run.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::RemoteCapabilityDenied`] when the exact id was not granted.
    pub fn require_action(&self, capability: Option<&str>, what: &str) -> Result<(), ErrorValue> {
        let Some(capability) = capability else {
            return Err(self
                .denial(format!(
                    "`{what}` needs a capability this agent cannot name, and an unnamed \
                     capability is never granted"
                ))
                .with_metadata("denied_because", Value::string("capability_unknown"))
                .with_help(
                    "Appendix C: an unknown capability id is always denied. The agent grants \
                     exactly the ids it can resolve to a command contract.",
                ));
        };
        if self.allows_action(capability) {
            return Ok(());
        }
        Err(self
            .denial(format!(
                "this client is authorized, but not for `{capability}`, so `{what}` is refused"
            ))
            .with_metadata("requested_capability", Value::string(capability))
            .with_metadata("denied_because", Value::string("action_not_granted"))
            .with_help(format!(
                "grants name exact capability ids (v0.4.1 section 9.5). On the agent's host, \
                 `set client-key <fingerprint> --allow {capability}` grants this one and leaves \
                 every other action refused."
            )))
    }

    fn denial(&self, message: String) -> ErrorValue {
        let error =
            ErrorValue::new(ErrorCode::RemoteCapabilityDenied, message).with_retryable(false);
        match self.context() {
            None => error,
            Some(context) => error
                .with_metadata(
                    "peer_fingerprint",
                    Value::string(&context.peer_fingerprint().to_string()),
                )
                .with_metadata("connection_id", Value::string(context.connection_id())),
        }
    }
}

/// Writes `entries` to `path`, atomically, leaving the previous store intact if anything fails.
///
/// §9.8: write to a temporary, fsync it, rename it over the store, then sync the directory. A
/// reader therefore sees the old file or the new one and never a prefix of the new one, and an
/// update that dies part-way leaves a store that still loads — which matters more here than for
/// most files, because §9.2's strict parser would refuse a half-written store and take the
/// agent's whole policy down with it.
///
/// The file is created `0600`. It is the operator's decision about who may reach this machine,
/// and an account that can rewrite it can authorize itself.
///
/// # Errors
///
/// `io.permission_denied` naming the path that could not be written. The store at `path` is
/// unchanged whenever this returns an error.
pub fn write_store(path: &Path, entries: &[AuthorizedClient]) -> Result<(), ErrorValue> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let contents = render(entries);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| store_io_error(parent, &error))?;
    }
    let temporary = path.with_extension("tmp");
    // Removed rather than truncated in place: `create_new` then refuses a leftover another
    // process is still writing, instead of two updates interleaving into one file.
    let _ = std::fs::remove_file(&temporary);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| store_io_error(&temporary, &error))?;
    file.write_all(contents.as_bytes())
        .map_err(|error| store_io_error(&temporary, &error))?;
    file.sync_all()
        .map_err(|error| store_io_error(&temporary, &error))?;
    drop(file);
    std::fs::rename(&temporary, path).map_err(|error| {
        // The rename is the only step that can leave a temporary behind, and a temporary that
        // outlives its update would refuse the next one.
        let _ = std::fs::remove_file(&temporary);
        store_io_error(path, &error)
    })?;
    // The rename is durable only once the directory entry is. Not every filesystem supports
    // syncing a directory handle, and one that does not is not a reason to fail an update that
    // already happened.
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && let Ok(directory) = std::fs::File::open(parent)
    {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn store_io_error(path: &Path, error: &std::io::Error) -> ErrorValue {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => ErrorCode::IoNotFound,
        _ => ErrorCode::IoPermissionDenied,
    };
    ErrorValue::new(
        code,
        format!(
            "the authorization store at {} could not be written: {error}",
            path.display()
        ),
    )
    .with_help(
        "the previous store is untouched: an update writes a temporary beside it and renames it \
         into place, so a failure never leaves half a policy (v0.4.1 section 9.8)",
    )
}

/// The remediation a refused client is owed, for a refusal that arrived as a bare code and
/// message on the wire (§10.4, §54.1, §54.2).
///
/// A `Reject` carries a code and a sentence, because those are the two things a refusal has to be
/// able to say before a link exists. The guidance is not secret and is the same for everyone who
/// meets the code, so the receiving side supplies it rather than the wire carrying it — which
/// also keeps §59.1's rule that a refusal disclose nothing about the machine that sent it.
#[must_use]
pub fn refusal_guidance(code: ErrorCode) -> Option<&'static str> {
    match code {
        ErrorCode::RemoteUnauthenticated => Some(
            "authorization follows authentication and never replaces it (v0.4.1 section 2.2). A \
             direct link presents a peer identity on both ends; `ono --print-peer-key` prints \
             this shell's.",
        ),
        ErrorCode::RemoteUnauthorized => Some(
            "holding a private key proves who you are, never that the operator wants you here \
             (v0.4.1 section 9.1). `ono --print-peer-key` prints the fingerprint to send them; on \
             the agent's host, `add client-key <fingerprint>` authorizes it to observe.",
        ),
        ErrorCode::RemoteCapabilityDenied => Some(
            "grants name exact capability ids (v0.4.1 section 9.5). On the agent's host, `set \
             client-key <fingerprint> --allow <capability>` grants exactly the one named and \
             leaves every other action refused.",
        ),
        ErrorCode::RemoteConnectionLimit => Some(
            "a listening agent bounds concurrent connections globally and per client key \
             (v0.4.1 section 12.1, section 12.3). Nothing about this client was refused: wait \
             for a session to end, or raise `limits.remote_connections` / \
             `limits.remote_connections_per_client` on the agent's host.",
        ),
        ErrorCode::RemoteHandshakeTimeout => Some(
            "TLS and Ono negotiation together have a deadline on the agent's side (v0.4.1 \
             section 12.2), configured with `limits.remote_handshake_timeout_ms` on its host. A \
             link dropped this way is worth trying again.",
        ),
        _ => None,
    }
}
