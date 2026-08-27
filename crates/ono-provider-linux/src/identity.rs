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
            other => {
                return Err(ErrorValue::new(
                    ErrorCode::ProviderUnsupported,
                    format!("{PROVIDER_ID} changes `user` accounts, not `{other}`"),
                ));
            }
        };
        let command = match change {
            Ok(command) => command,
            // The provider could not even say what to run — the account is not there, or an
            // option is not one it understands. That is this target's outcome (spec §16.5).
            Err(error) => return Ok(ActionOutcome::failed(action, error)),
        };
        // Spec §11.6: asked without being obeyed. The plan is the exact invocation, so a user
        // sees what elevation would buy before paying for it.
        if action.is_dry_run() {
            return Ok(ActionOutcome::skipped(
                action,
                format!("would run `{command}`"),
            ));
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
                    "`{}` is what would run; elevate explicitly (spec §17.2) rather than \
                     re-running blind",
                    command.program()
                )),
            ));
        }
        match command.run().await {
            Ok(()) => Ok(ActionOutcome::succeeded(action, true)),
            Err(error) => Ok(ActionOutcome::failed(action, error)),
        }
    }
}

/// The login name an action's object refers to: the name itself when it arrived unresolved
/// (`add`), or the account behind a resolved uid (`remove`, `set`).
async fn user_name_of(accounts: &Arc<dyn Accounts>, action: &Action) -> Result<String, ErrorValue> {
    let first = action.target().values().first().cloned();
    match first {
        Some(Value::String(name)) => Ok(name.to_string()),
        Some(Value::Int(uid)) => {
            let uid = u32::try_from(uid).map_err(|_| {
                ErrorValue::new(ErrorCode::IoNotFound, format!("no user has the uid {uid}"))
            })?;
            accounts
                .user(uid)
                .await
                .map(|account| account.name)
                .ok_or_else(|| {
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

impl IdentityProvider {
    /// What a `user` action asks to run, or why nothing can be run for it.
    async fn user_change(&self, action: &Action) -> Result<AccountCommand, ErrorValue> {
        let name = user_name_of(&self.accounts, action).await?;
        match action.operation() {
            "add" => Ok(AccountCommand::add_user(
                &name,
                id_argument(action, "uid")?,
                path_argument(action, "home")?.as_deref(),
                path_argument(action, "shell")?.as_deref(),
                group_argument(action, "group")?.as_deref(),
            )),
            "remove" => Ok(AccountCommand::remove_user(
                &name,
                bool_argument(action, "remove-home")?,
            )),
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
                Ok(AccountCommand::set_user(
                    &name,
                    shell.as_deref(),
                    home.as_deref(),
                    group.as_deref(),
                ))
            }
            other => Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!("{PROVIDER_ID} has no operation `{other}` for a user"),
            )
            .with_help("it can `add`, `remove` and `set` a user account")),
        }
    }
}
