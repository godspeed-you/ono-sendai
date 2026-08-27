//! The `user` and `group` targets (spec §23.6, §28.7).
//!
//! Resolution goes through NSS, so an account served by LDAP or SSSD answers exactly like a
//! local one, and an id that resolves to no name keeps its number rather than disappearing —
//! which is what spec §23.6 asks for in as many words. See [`crate::accounts`] for what
//! enumeration can and cannot reach, and [`crate::account_tools`] for how `add`, `remove` and
//! `set` change the account database (ADR-0101).

use std::sync::Arc;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, StreamSink, ValueStream};
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, ObjectRef, Provider, Query, Risk, Selector,
};
use ono_value::{ErrorValue, RecordValue, Schema, Value};

use crate::account_tools::AccountCommand;
use crate::accounts::{Accounts, GroupAccount, NssAccounts, UserAccount};
use crate::common::{group_ref, provenance};
use crate::schemas;

/// The provider's stable id, as it appears in every record's provenance.
pub const PROVIDER_ID: &str = "linux.nss";

/// Users and groups.
#[derive(Debug)]
pub struct IdentityProvider {
    accounts: Arc<dyn Accounts>,
}

impl Default for IdentityProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl IdentityProvider {
    /// The accounts of the machine this shell runs on.
    #[must_use]
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(NssAccounts::new()),
        }
    }

    /// A provider over `accounts`.
    #[must_use]
    pub fn over(accounts: Arc<dyn Accounts>) -> Self {
        Self { accounts }
    }

    /// The name a selector asks for, when it asks by name.
    fn wanted_name(query: &Query) -> Option<String> {
        query
            .selectors()
            .iter()
            .find_map(|selector| match selector {
                Selector::Field { name, value } if name == "name" => {
                    value.as_str().ok().map(ToOwned::to_owned)
                }
                _ => None,
            })
    }

    /// The numeric id a selector asks for, when it asks by number.
    fn wanted_id(query: &Query, field: &str) -> Option<u32> {
        query
            .selectors()
            .iter()
            .find_map(|selector| match selector {
                Selector::Field { name, value } if name == field => {
                    value.as_int().ok().and_then(|id| u32::try_from(id).ok())
                }
                Selector::Identity(id) => id
                    .values()
                    .first()
                    .and_then(|value| value.as_int().ok())
                    .and_then(|id| u32::try_from(id).ok()),
                _ => None,
            })
    }
}

/// Builds one `ono.user/1` record.
async fn user_record(
    account: &UserAccount,
    schema: &Arc<Schema>,
    group_schema: &Arc<Schema>,
    accounts: &Arc<dyn Accounts>,
) -> Result<RecordValue, ErrorValue> {
    let group_name = accounts.group(account.gid).await.map(|group| group.name);
    Ok(RecordValue::builder(
        Arc::clone(schema),
        provenance(PROVIDER_ID, schema.id(), "getpwnam(3) / getpwuid(3)"),
    )
    .set("uid", Value::Int(i128::from(account.uid)))?
    .set("name", non_empty(&account.name))?
    .set(
        "primary_group",
        group_ref(group_schema, account.gid, group_name.as_deref()),
    )?
    .set("home", path_or_null(&account.home))?
    .set("shell", path_or_null(&account.shell))?
    .set("gecos", non_empty(&account.gecos))?
    .build())
}

/// Builds one `ono.group/1` record.
fn group_record(account: &GroupAccount, schema: &Arc<Schema>) -> Result<RecordValue, ErrorValue> {
    Ok(RecordValue::builder(
        Arc::clone(schema),
        provenance(PROVIDER_ID, schema.id(), "getgrnam(3) / getgrgid(3)"),
    )
    .set("gid", Value::Int(i128::from(account.gid)))?
    .set("name", non_empty(&account.name))?
    .set(
        "members",
        Value::list(account.members.iter().map(|member| Value::string(member))),
    )?
    .build())
}

/// Text the database left empty is unknown, not the empty string.
fn non_empty(text: &str) -> Value {
    if text.is_empty() {
        Value::Null
    } else {
        Value::string(text)
    }
}

fn path_or_null(path: &std::path::Path) -> Value {
    if path.as_os_str().is_empty() {
        Value::Null
    } else {
        Value::Path(Arc::from(path))
    }
}

async fn stream_users(
    accounts: Arc<dyn Accounts>,
    query: Query,
    schema: Arc<Schema>,
    group_schema: Arc<Schema>,
    sink: StreamSink,
) {
    let found = if let Some(name) = IdentityProvider::wanted_name(&query) {
        accounts.user_named(&name).await.into_iter().collect()
    } else if let Some(uid) = IdentityProvider::wanted_id(&query, "uid") {
        accounts.user(uid).await.into_iter().collect()
    } else {
        match accounts.users() {
            Ok(users) => users,
            Err(error) => {
                let _ = sink.fail(error).await;
                return;
            }
        }
    };
    let limit = query.max().unwrap_or(usize::MAX);
    for account in found.iter().take(limit) {
        match user_record(account, &schema, &group_schema, &accounts).await {
            Ok(record) => {
                if sink.send(record.into_value()).await.is_err() {
                    return;
                }
            }
            Err(error) => {
                if sink.fail(error).await.is_err() {
                    return;
                }
            }
        }
    }
}

async fn stream_groups(
    accounts: Arc<dyn Accounts>,
    query: Query,
    schema: Arc<Schema>,
    sink: StreamSink,
) {
    let found = if let Some(name) = IdentityProvider::wanted_name(&query) {
        accounts.group_named(&name).await.into_iter().collect()
    } else if let Some(gid) = IdentityProvider::wanted_id(&query, "gid") {
        accounts.group(gid).await.into_iter().collect()
    } else {
        match accounts.groups() {
            Ok(groups) => groups,
            Err(error) => {
                let _ = sink.fail(error).await;
                return;
            }
        }
    };
    let limit = query.max().unwrap_or(usize::MAX);
    for account in found.iter().take(limit) {
        match group_record(account, &schema) {
            Ok(record) => {
                if sink.send(record.into_value()).await.is_err() {
                    return;
                }
            }
            Err(error) => {
                if sink.fail(error).await.is_err() {
                    return;
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl Provider for IdentityProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["user", "group"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        [schemas::user_id(), schemas::group_id()]
            .iter()
            .filter_map(|id| schemas::require(id).ok())
            .collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("user.list", Risk::Read),
            Capability::new("group.list", Risk::Read),
            // `docs/spec/capabilities.yaml` gives both elevation `required`: only root may
            // change the account database, and the provider says so before trying (ADR-0101).
            Capability::new("user.manage", Risk::Mutate).needing_elevation(),
            Capability::new("group.manage", Risk::Mutate).needing_elevation(),
        ]
    }

    fn availability(&self) -> Availability {
        Availability::Available
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let accounts = Arc::clone(&self.accounts);
        let query = query.clone();
        match query.target_name() {
            "user" => {
                let schema = schemas::require(&schemas::user_id())?;
                let group_schema = schemas::require(&schemas::group_id())?;
                Ok(ValueStream::spawn(
                    PipelineConfig::new(),
                    Boundedness::Bounded,
                    move |sink| async move {
                        stream_users(accounts, query, schema, group_schema, sink).await;
                    },
                ))
            }
            "group" => {
                let schema = schemas::require(&schemas::group_id())?;
                Ok(ValueStream::spawn(
                    PipelineConfig::new(),
                    Boundedness::Bounded,
                    move |sink| async move {
                        stream_groups(accounts, query, schema, sink).await;
                    },
                ))
            }
            other => Err(ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("{PROVIDER_ID} does not answer `{other}`"),
            )),
        }
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let user_schema = schemas::require(&schemas::user_id())?;
        let group_schema = schemas::require(&schemas::group_id())?;
        let mut found = Vec::new();
        match selector {
            Selector::Field { name, value } if name == "name" => {
                let Ok(text) = value.as_str() else {
                    return Ok(found);
                };
                if let Some(account) = self.accounts.user_named(text).await {
                    let record =
                        user_record(&account, &user_schema, &group_schema, &self.accounts).await?;
                    found.extend(ObjectRef::of(&record));
                }
                if let Some(account) = self.accounts.group_named(text).await {
                    let record = group_record(&account, &group_schema)?;
                    found.extend(ObjectRef::of(&record));
                }
            }
            Selector::Field { name, value } if name == "uid" => {
                if let Ok(uid) = value.as_int()
                    && let Ok(uid) = u32::try_from(uid)
                    && let Some(account) = self.accounts.user(uid).await
                {
                    let record =
                        user_record(&account, &user_schema, &group_schema, &self.accounts).await?;
                    found.extend(ObjectRef::of(&record));
                }
            }
            Selector::Field { name, value } if name == "gid" => {
                if let Ok(gid) = value.as_int()
                    && let Ok(gid) = u32::try_from(gid)
                    && let Some(account) = self.accounts.group(gid).await
                {
                    let record = group_record(&account, &group_schema)?;
                    found.extend(ObjectRef::of(&record));
                }
            }
            _ => {}
        }
        Ok(found)
    }
    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let change = match action.target_name() {
            "user" => self.user_change(action).await,
            "group" => self.group_change(action).await,
            other => {
                return Err(ErrorValue::new(
                    ErrorCode::ProviderUnsupported,
                    format!("{PROVIDER_ID} changes `user` and `group` accounts, not `{other}`"),
                ));
            }
        };
        let commands = match change {
            Ok(commands) => commands,
            // The provider could not even say what to run — the account is not there, or an
            // option is not one it understands. That is this target's outcome (spec §16.5).
            Err(error) => return Ok(ActionOutcome::failed(action, error)),
        };
        let plan = commands
            .iter()
            .map(|command| format!("`{command}`"))
            .collect::<Vec<_>>()
            .join(", ");
        // Spec §11.6: asked without being obeyed. The plan is the exact invocation, so a user
        // sees what elevation would buy before paying for it.
        if action.is_dry_run() {
            return Ok(ActionOutcome::skipped(action, format!("would run {plan}")));
        }
        // The tools refuse an unprivileged caller with the same status they use for a locked
        // database, so the shell asks the kernel first and answers with the code that is true:
        // permission, not an external failure (ADR-0101).
        if !nix::unistd::Uid::effective().is_root() {
            return Ok(ActionOutcome::failed(
                action,
                ErrorValue::new(
                    ErrorCode::IoPermissionDenied,
                    format!(
                        "changing the account database needs root, and this shell runs as uid {}",
                        nix::unistd::Uid::effective()
                    ),
                )
                .with_help(format!(
                    "{plan} is what would run; elevate explicitly (spec §17.2) rather than \
                     re-running blind"
                )),
            ));
        }
        // A membership change is one tool run per member; the row is the group's, so the first
        // member the tool refuses is the group's outcome, and the members before it stay
        // changed — which the error says.
        for (index, command) in commands.iter().enumerate() {
            if let Err(error) = command.run().await {
                let error = if index == 0 {
                    error
                } else {
                    error.with_help(format!(
                        "{index} of {} changes were made before this one failed",
                        commands.len()
                    ))
                };
                return Ok(ActionOutcome::failed(action, error));
            }
        }
        Ok(ActionOutcome::succeeded(action, true))
    }
}

/// The login name an action's object refers to: the name itself when it arrived unresolved
/// (`add`), or the account behind a resolved uid (`remove`, `set`).
async fn user_name_of(accounts: &Arc<dyn Accounts>, action: &Action) -> Result<String, ErrorValue> {
    match action.target().values().first().cloned() {
        Some(Value::String(name)) => Ok(name.to_string()),
        Some(Value::Int(uid)) => {
            let found = match u32::try_from(uid) {
                Ok(uid) => accounts.user(uid).await.map(|account| account.name),
                Err(_) => None,
            };
            found.ok_or_else(|| {
                ErrorValue::new(ErrorCode::IoNotFound, format!("no user has the uid {uid}"))
            })
        }
        other => Err(ErrorValue::new(
            ErrorCode::ResolveTargetNotFound,
            format!(
                "`{}` does not name a user account",
                describe(other.as_ref())
            ),
        )),
    }
}

/// The group name an action's object refers to, on the same terms as [`user_name_of`].
async fn group_name_of(
    accounts: &Arc<dyn Accounts>,
    action: &Action,
) -> Result<String, ErrorValue> {
    match action.target().values().first().cloned() {
        Some(Value::String(name)) => Ok(name.to_string()),
        Some(Value::Int(gid)) => {
            let found = match u32::try_from(gid) {
                Ok(gid) => accounts.group(gid).await.map(|account| account.name),
                Err(_) => None,
            };
            found.ok_or_else(|| {
                ErrorValue::new(ErrorCode::IoNotFound, format!("no group has the gid {gid}"))
            })
        }
        other => Err(ErrorValue::new(
            ErrorCode::ResolveTargetNotFound,
            format!("`{}` does not name a group", describe(other.as_ref())),
        )),
    }
}

/// The members a repeatable `--member ref<ono.user/1>` names: login names, or uids as text,
/// either of which `gpasswd` accepts.
fn member_arguments(action: &Action) -> Result<Vec<String>, ErrorValue> {
    let mut members = Vec::new();
    for (name, value) in action.arguments() {
        if name != "member" {
            continue;
        }
        let values: Vec<&Value> = match value {
            Value::List(items) => items.iter().collect(),
            single => vec![single],
        };
        for value in values {
            match value {
                Value::Null => {}
                Value::String(text) => members.push(text.to_string()),
                Value::Int(uid) => members.push(uid.to_string()),
                Value::Record(record) => {
                    if let Some(name) = record.get("name").and_then(|value| value.as_str().ok()) {
                        members.push(name.to_owned());
                    } else if let Some(uid) =
                        record.get("uid").and_then(|value| value.as_int().ok())
                    {
                        members.push(uid.to_string());
                    }
                }
                other => {
                    return Err(ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        format!("`--member` names a user, not a {}", other.type_name()),
                    ));
                }
            }
        }
    }
    Ok(members)
}

fn describe(value: Option<&Value>) -> String {
    value.map_or_else(|| "nothing".to_owned(), |value| format!("{value:?}"))
}

/// A `path` option, which binding delivers as a path or — from a piped record — as text.
fn path_argument(action: &Action, name: &str) -> Result<Option<std::path::PathBuf>, ErrorValue> {
    match action.argument(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Path(path)) => Ok(Some(path.to_path_buf())),
        Some(Value::String(text)) => Ok(Some(std::path::PathBuf::from(text.as_ref()))),
        Some(other) => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("`--{name}` is a path, not a {}", other.type_name()),
        )),
    }
}

/// A `ref<ono.group/1>` option: the name or the number the user wrote, either of which the
/// tools accept as a group.
fn group_argument(action: &Action, name: &str) -> Result<Option<String>, ErrorValue> {
    match action.argument(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.to_string())),
        Some(Value::Int(gid)) => Ok(Some(gid.to_string())),
        Some(Value::Record(record)) => Ok(record
            .get("name")
            .and_then(|value| value.as_str().ok())
            .map(ToOwned::to_owned)
            .or_else(|| {
                record
                    .get("gid")
                    .and_then(|value| value.as_int().ok())
                    .map(|gid| gid.to_string())
            })),
        Some(other) => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("`--{name}` names a group, not a {}", other.type_name()),
        )),
    }
}

fn id_argument(action: &Action, name: &str) -> Result<Option<u32>, ErrorValue> {
    match action.argument(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let id = value.as_int()?;
            u32::try_from(id).map(Some).map_err(|_| {
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`--{name}` must be a non-negative id that fits the system, not {id}"),
                )
            })
        }
    }
}

fn bool_argument(action: &Action, name: &str) -> Result<bool, ErrorValue> {
    match action.argument(name) {
        None | Some(Value::Null) => Ok(false),
        Some(value) => value.as_bool(),
    }
}

/// The outcome of naming an account that is not there, in the words the shell uses for a
/// selector nothing answers to (ADR-0068 §2).
fn not_found(target: &str, name: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::IoNotFound,
        format!("no {target} answers to name {name}"),
    )
    .with_help(format!("`get {target}` lists what is there"))
}

impl IdentityProvider {
    /// What a `user` action asks to run, or why nothing can be run for it.
    async fn user_change(&self, action: &Action) -> Result<Vec<AccountCommand>, ErrorValue> {
        let name = user_name_of(&self.accounts, action).await?;
        // An account that is not there is that target's outcome before privilege is asked
        // for: the database is readable by anyone, so the provider can look first (ADR-0108).
        if matches!(action.operation(), "remove" | "set")
            && self.accounts.user_named(&name).await.is_none()
        {
            return Err(not_found("user", &name));
        }
        match action.operation() {
            "add" => Ok(vec![AccountCommand::add_user(
                &name,
                id_argument(action, "uid")?,
                path_argument(action, "home")?.as_deref(),
                path_argument(action, "shell")?.as_deref(),
                group_argument(action, "group")?.as_deref(),
            )]),
            "remove" => Ok(vec![AccountCommand::remove_user(
                &name,
                bool_argument(action, "remove-home")?,
            )]),
            "set" => {
                let shell = path_argument(action, "shell")?;
                let home = path_argument(action, "home")?;
                let group = group_argument(action, "group")?;
                if shell.is_none() && home.is_none() && group.is_none() {
                    return Err(ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        "`set user` changes `--shell`, `--home` or `--group`, and none was given"
                            .to_owned(),
                    ));
                }
                Ok(vec![AccountCommand::set_user(
                    &name,
                    shell.as_deref(),
                    home.as_deref(),
                    group.as_deref(),
                )])
            }
            other => Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("{PROVIDER_ID} has no operation `{other}` for a user"),
            )
            .with_help("it can `add`, `remove` and `set` a user account")),
        }
    }

    /// What a `group` action asks to run, or why nothing can be run for it.
    ///
    /// `--member` turns `add` and `remove` from creating or deleting the group into changing
    /// its membership — §7.1's "membership/association" sense of `add`, as `identity.yaml`
    /// documents it.
    async fn group_change(&self, action: &Action) -> Result<Vec<AccountCommand>, ErrorValue> {
        let name = group_name_of(&self.accounts, action).await?;
        let members = member_arguments(action)?;
        // As for a user: a group that is not there is answered before privilege (ADR-0108).
        // `add --member` extends a group the same way and needs it to exist too.
        let acts_on_existing =
            matches!(action.operation(), "remove" | "set") || !members.is_empty();
        if acts_on_existing && self.accounts.group_named(&name).await.is_none() {
            return Err(not_found("group", &name));
        }
        match action.operation() {
            "add" if !members.is_empty() => Ok(members
                .iter()
                .map(|member| AccountCommand::add_member(&name, member))
                .collect()),
            "add" => Ok(vec![AccountCommand::add_group(
                &name,
                id_argument(action, "gid")?,
            )]),
            "remove" if !members.is_empty() => Ok(members
                .iter()
                .map(|member| AccountCommand::remove_member(&name, member))
                .collect()),
            "remove" => Ok(vec![AccountCommand::remove_group(&name)]),
            "set" => match id_argument(action, "gid")? {
                Some(gid) => Ok(vec![AccountCommand::set_group(&name, gid)]),
                None => Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    "`set group` changes `--gid`, and none was given".to_owned(),
                )),
            },
            other => Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("{PROVIDER_ID} has no operation `{other}` for a group"),
            )
            .with_help("it can `add`, `remove` and `set` a group, and add or remove members")),
        }
    }
}
