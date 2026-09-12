//! The human layer over the broker, in the shell (K11P §6, §14, §16, §23; ADR-0600, ADR-0603,
//! ADR-0604): a decision and its store, the grants a decision mints, the projection
//! `get permission` answers, the profile application `install plugin` and `set permission`
//! share, and the consent source that asks a person at the broker's request.
//!
//! Nothing here is a second security engine. A decision is executed by minting ordinary grants
//! through [`Host::grant`], and `policy.yaml` stays what the broker enforces; this module keeps
//! the record that links a human intention to those grants, and reads it back.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ono_core::{ErrorCode, ExitStatus};
use ono_kuang_protocol::{
    Capability, DeclarationClass, Enforcement, GrantTemplate, KuangError, KuangErrorCode, Manifest,
    PermissionDescriptor, PermissionPhase, PermissionSet, ScopeTemplate, describe_scope,
};
use ono_kuang_supervisor::{ConsentAnswer, ConsentDuration, ConsentRequest, ConsentSource};
use ono_value::{ErrorValue, RecordValue, Value};
use serde_json::{Map as JsonMap, Value as Json};

use crate::eval::{Eval, Flow};
use crate::kuang_host::{Grant, GrantOrigin, Host, Installed, Management, map, schema};
use crate::plugins::Produced;
use crate::session::Session;

/// The decision store's format line.
pub const DECISIONS_FORMAT: &str = "kuang-permissions/1";

/// One stored decision (K11P §23.1, `permissions.v1.yaml` → `decision.record`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DecisionRecord {
    /// `allow`, `deny` or `ask`.
    pub decision: String,
    /// `always` in the store; `session` in memory only.
    pub duration: String,
    /// The profile that selected it, or `None` for a decision made one at a time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// When it was first made.
    pub created_at: String,
    /// When it last changed.
    pub updated_at: String,
    /// The grants it minted, by id.
    #[serde(default)]
    pub grants: Vec<String>,
    /// The capabilities the permission resolves to — what a `deny` holds ahead of any grant.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// The package version consented to.
    pub package_version: String,
    /// The signing key at consent, or `unsigned`.
    pub publisher: String,
}

/// A decision in memory: whose, which, and the record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// The package id.
    pub plugin: String,
    /// The permission id.
    pub permission: String,
    /// The record.
    pub record: DecisionRecord,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct StoredDecisions {
    #[serde(default)]
    format: String,
    #[serde(default)]
    plugins: BTreeMap<String, BTreeMap<String, DecisionRecord>>,
}

impl Host {
    fn decisions_path(&self) -> Option<PathBuf> {
        self.config_dir()
            .map(|dir| dir.join("kuang").join("permissions.yaml"))
    }

    /// Reads the operator's stored decisions into this session, once per store (ADR-0604 §1).
    pub(crate) fn read_decisions(&mut self) {
        let Some(path) = self.decisions_path() else {
            return;
        };
        if self.decisions_read.as_ref() == Some(&path) {
            return;
        }
        self.decisions_read = Some(path.clone());
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        if ono_value::yaml_depth(&text) > ono_value::MAX_YAML_DEPTH {
            return;
        }
        let Ok(stored) = serde_yaml_ng::from_str::<StoredDecisions>(&text) else {
            return;
        };
        for (plugin, decisions) in stored.plugins {
            for (permission, record) in decisions {
                self.decisions.push(Decision {
                    plugin: plugin.clone(),
                    permission,
                    record,
                });
            }
        }
    }

    /// Writes the `always` decisions back to the store (ADR-0604 §1).
    ///
    /// # Errors
    ///
    /// The I/O failure, which an install transaction rolls back on.
    pub(crate) fn write_decisions(&self) -> Result<(), ErrorValue> {
        let Some(path) = self.decisions_path() else {
            return Ok(());
        };
        let mut stored = StoredDecisions {
            format: DECISIONS_FORMAT.to_owned(),
            plugins: BTreeMap::new(),
        };
        for decision in self
            .decisions
            .iter()
            .filter(|decision| decision.record.duration == "always")
        {
            stored
                .plugins
                .entry(decision.plugin.clone())
                .or_default()
                .insert(decision.permission.clone(), decision.record.clone());
        }
        let text = serde_yaml_ng::to_string(&stored).map_err(|error| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                format!("the permission store does not serialise: {error}"),
            )
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| crate::kuang_host::io_error(parent, &error))?;
        }
        std::fs::write(&path, text).map_err(|error| crate::kuang_host::io_error(&path, &error))
    }

    /// Writes both stores, so a decision never points at a grant that is not there.
    ///
    /// # Errors
    ///
    /// The first I/O failure.
    pub(crate) fn persist_permissions(&self) -> Result<(), ErrorValue> {
        self.write_policy()?;
        self.write_decisions()
    }

    /// The standing decision of `plugin` about `permission`.
    #[must_use]
    pub fn decision(&self, plugin: &str, permission: &str) -> Option<&Decision> {
        self.decisions
            .iter()
            .find(|decision| decision.plugin == plugin && decision.permission == permission)
    }

    /// Every decision about `plugin`.
    pub fn decisions_of<'a>(&'a self, plugin: &'a str) -> impl Iterator<Item = &'a Decision> + 'a {
        self.decisions
            .iter()
            .filter(move |decision| decision.plugin == plugin)
    }

    /// Records or replaces a decision (ADR-0604 §1).
    #[allow(
        clippy::too_many_arguments,
        reason = "a decision record simply has this many parts"
    )]
    pub(crate) fn record_decision(
        &mut self,
        plugin: &str,
        permission: &str,
        decision: &str,
        duration: &str,
        profile: Option<&str>,
        grants: Vec<String>,
        capabilities: Vec<String>,
        package_version: &str,
        publisher: &str,
    ) {
        let now = jiff::Timestamp::now().to_string();
        let created_at = self.decision(plugin, permission).map_or_else(
            || now.clone(),
            |existing| existing.record.created_at.clone(),
        );
        self.decisions
            .retain(|existing| !(existing.plugin == plugin && existing.permission == permission));
        self.decisions.push(Decision {
            plugin: plugin.to_owned(),
            permission: permission.to_owned(),
            record: DecisionRecord {
                decision: decision.to_owned(),
                duration: duration.to_owned(),
                profile: profile.map(str::to_owned),
                created_at,
                updated_at: now,
                grants,
                capabilities,
                package_version: package_version.to_owned(),
                publisher: publisher.to_owned(),
            },
        });
    }

    /// Forgets the decision of `plugin` about `permission`, answering whether one stood.
    pub(crate) fn forget_decision(&mut self, plugin: &str, permission: &str) -> bool {
        let before = self.decisions.len();
        self.decisions
            .retain(|existing| !(existing.plugin == plugin && existing.permission == permission));
        self.decisions.len() != before
    }

    /// Forgets every decision about `plugin` (K11P §22.1).
    pub(crate) fn forget_decisions_of(&mut self, plugin: &str) -> usize {
        let before = self.decisions.len();
        self.decisions.retain(|existing| existing.plugin != plugin);
        before - self.decisions.len()
    }

    /// Remembers a just-in-time denial for this session (ADR-0603 §2).
    pub(crate) fn remember_denial(&mut self, plugin: &str, permission: &str, subject: &str) {
        self.session_denials
            .insert((plugin.to_owned(), permission.to_owned(), subject.to_owned()));
    }

    /// Whether this session already refused this exact request.
    #[must_use]
    pub fn denied_in_session(&self, plugin: &str, permission: &str, subject: &str) -> bool {
        self.session_denials.contains(&(
            plugin.to_owned(),
            permission.to_owned(),
            subject.to_owned(),
        ))
    }

    fn forget_denials(&mut self, plugin: &str, permission: &str) {
        self.session_denials
            .retain(|(held_plugin, held_permission, _)| {
                !(held_plugin == plugin && held_permission == permission)
            });
    }

    /// The grants that stand for `plugin` and were minted by `permission`.
    fn grants_of_permission(&self, plugin: &str, permission: &str) -> Vec<ono_value::Uuid> {
        self.standing_grants(plugin)
            .filter(|grant| grant.permission.as_deref() == Some(permission))
            .map(|grant| grant.id)
            .collect()
    }

    /// The `ono.permission/1` records of `package` (K11P §16.1), or of every installed package.
    ///
    /// # Errors
    ///
    /// `provider.schema_violation` when a record does not fit its contract.
    pub fn permission_records(
        &self,
        plugin: Option<&Installed>,
        all: bool,
    ) -> Result<Vec<RecordValue>, ErrorValue> {
        let packages: Vec<Installed> = match plugin {
            Some(package) => vec![package.clone()],
            None => self.installed().0,
        };
        let schema = schema("ono.permission")?;
        let mut records = Vec::new();
        for package in &packages {
            records.extend(self.permission_records_of(&schema, package, all)?);
        }
        Ok(records)
    }

    fn permission_records_of(
        &self,
        schema: &Arc<ono_value::Schema>,
        package: &Installed,
        all: bool,
    ) -> Result<Vec<RecordValue>, ErrorValue> {
        let id = package.manifest.package.id.as_str();
        let set = package.manifest.permission_set();
        let mut records = Vec::new();
        let mut mapped = BTreeSet::new();
        for descriptor in &set.descriptors {
            mapped.extend(descriptor.capabilities());
            // The support permissions the host derived for an extension-local family hide in
            // the compact view (K11P §16.1's `--all`); a package's own wording never does.
            if !all && descriptor.derived && descriptor.is_automatic() {
                continue;
            }
            records.push(self.permission_record(schema, package, descriptor)?);
        }
        if all {
            for grant in self.standing_grants(id) {
                if mapped.contains(&grant.capability) {
                    continue;
                }
                records.push(self.legacy_record(schema, package, grant)?);
            }
        }
        Ok(records)
    }

    /// The projection of one descriptor onto what stands (K11P §16.1, §28.3, ADR-0604 §2).
    fn permission_record(
        &self,
        schema: &Arc<ono_value::Schema>,
        package: &Installed,
        descriptor: &PermissionDescriptor,
    ) -> Result<RecordValue, ErrorValue> {
        let id = package.manifest.package.id.as_str();
        let decision = self.decision(id, &descriptor.id);
        let mut chosen: Vec<(&GrantTemplate, Option<&Grant>)> = Vec::new();
        let mut held = true;
        let mut exact = true;
        for template in &descriptor.grants {
            let standing: Vec<&Grant> = self
                .standing_grants(id)
                .filter(|grant| grant.capability == template.capability)
                .collect();
            let expected = resolve_scope(package, template, None);
            let by_permission = standing
                .iter()
                .find(|grant| grant.permission.as_deref() == Some(descriptor.id.as_str()));
            let grant = by_permission.or_else(|| standing.first()).copied();
            match grant {
                None => held = false,
                Some(grant) => {
                    // A runtime-derived scope has no expected value: the grant the permission
                    // minted carries the program or address the moment of use named (K11P §19.3).
                    let derived = matches!(template.scope, Some(ScopeTemplate::RuntimeDerived));
                    if grant.permission.as_deref() != Some(descriptor.id.as_str())
                        || (grant.scope != expected && !derived)
                    {
                        exact = false;
                    }
                }
            }
            chosen.push((template, grant));
        }
        let (state, when) = match decision.map(|decision| decision.record.decision.as_str()) {
            Some("deny") => ("denied", "always"),
            Some("ask") => ("ask", "when-needed"),
            // An automatic permission the install included is `included`, not a decision the
            // user made (K11P §7.1, §16.1).
            Some(_) if descriptor.is_automatic() && held && exact => ("included", "automatic"),
            Some(_) if held && exact => ("allowed", decision_when(decision)),
            Some(_) if held => ("custom", decision_when(decision)),
            Some(_) => ("denied", "always"),
            None => match descriptor.phase {
                PermissionPhase::Automatic => {
                    if held && exact {
                        ("included", "automatic")
                    } else if held {
                        ("custom", "automatic")
                    } else {
                        ("included", "automatic")
                    }
                }
                PermissionPhase::Jit => {
                    if held {
                        ("custom", "always")
                    } else {
                        ("ask", "when-needed")
                    }
                }
                PermissionPhase::Explicit => {
                    if held {
                        ("custom", "always")
                    } else {
                        ("denied", "explicit")
                    }
                }
                PermissionPhase::Install => {
                    if held {
                        ("custom", "always")
                    } else {
                        ("denied", "always")
                    }
                }
            },
        };
        let source = match (decision, state) {
            (_, "included") => Some("automatic"),
            (Some(decision), _) => Some(if decision.record.duration == "session" {
                "session"
            } else {
                "user-policy"
            }),
            (None, "custom") => Some("prompt"),
            _ => None,
        };
        // A `custom` permission is held by grants that differ from its mapping, so the scope it
        // declares is not the one the broker enforces: its record carries the stored scope of the
        // standing grants, in the human column and in the exact one (issue #128, K11P §19.2,
        // ADR-0857). `custom` implies every template is held, so each has a grant to read.
        let enforced = |template: &GrantTemplate, grant: Option<&Grant>| -> Option<ScopeTemplate> {
            match grant {
                Some(grant) if state == "custom" => {
                    grant.scope.clone().map(ScopeTemplate::Concrete)
                }
                _ => template.scope.clone(),
            }
        };
        let grants: Vec<Value> = chosen
            .iter()
            .map(|&(template, grant)| {
                let (broker_decision, source, duration, grant_id) = match grant {
                    Some(grant) => ("allow", grant.source, grant.duration, Value::Uuid(grant.id)),
                    None => ("deny", "default", "always", Value::Null),
                };
                map([
                    ("capability", Value::string(template.capability.id())),
                    (
                        "scope",
                        enforced(template, grant)
                            .map_or(Value::Null, |scope| json_value(&scope.as_json())),
                    ),
                    (
                        "enforcement",
                        Value::string(match template.enforcement() {
                            Enforcement::Broker => "broker",
                            Enforcement::Advisory => "advisory",
                        }),
                    ),
                    ("decision", Value::string(broker_decision)),
                    ("source", Value::string(source)),
                    ("duration", Value::string(duration)),
                    ("grant", grant_id),
                ])
            })
            .collect();
        let scope_text = chosen
            .iter()
            .map(|&(template, grant)| {
                describe_scope(template.capability, enforced(template, grant).as_ref())
            })
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("; ");
        Ok(
            RecordValue::builder(Arc::clone(schema), crate::kuang_host::provenance(schema))
                .set("plugin", Value::string(id))?
                .set("id", Value::string(&descriptor.id))?
                .set("title", Value::string(&descriptor.title))?
                .set(
                    "purpose",
                    descriptor
                        .purpose
                        .as_deref()
                        .map_or(Value::Null, Value::string),
                )?
                .set("kind", Value::string(descriptor.kind.id()))?
                .set("phase", Value::string(descriptor.phase.id()))?
                .set("risk", Value::string(descriptor.risk.id()))?
                .set("recommended", Value::Bool(descriptor.recommended))?
                .set("state", Value::string(state))?
                .set("when", Value::string(when))?
                .set(
                    "scope",
                    if scope_text.is_empty() {
                        Value::Null
                    } else {
                        Value::string(&scope_text)
                    },
                )?
                .set(
                    "capabilities",
                    Value::list(
                        descriptor
                            .capabilities()
                            .map(|capability| Value::string(capability.id())),
                    ),
                )?
                .set("grants", Value::list(grants))?
                .set(
                    "enforcement",
                    Value::string(match descriptor.enforcement() {
                        Enforcement::Broker => "broker",
                        Enforcement::Advisory => "advisory",
                    }),
                )?
                .set("source", source.map_or(Value::Null, Value::string))?
                .set(
                    "profile",
                    decision
                        .and_then(|decision| decision.record.profile.as_deref())
                        .map_or(Value::Null, Value::string),
                )?
                .set("derived", Value::Bool(descriptor.derived))?
                .set(
                    "decided_at",
                    decision.map_or(Value::Null, |decision| {
                        Value::parse_timestamp(&decision.record.updated_at)
                            .unwrap_or_else(|_| Value::string(&decision.record.updated_at))
                    }),
                )?
                .build(),
        )
    }

    /// A standing grant no descriptor maps, as the `legacy` row K11P §28.3 asks for.
    fn legacy_record(
        &self,
        schema: &Arc<ono_value::Schema>,
        package: &Installed,
        grant: &Grant,
    ) -> Result<RecordValue, ErrorValue> {
        let capability = grant.capability;
        let template = GrantTemplate {
            capability,
            scope: grant.scope.clone().map(ScopeTemplate::Concrete),
        };
        Ok(
            RecordValue::builder(Arc::clone(schema), crate::kuang_host::provenance(schema))
                .set("plugin", Value::string(&package.manifest.package.id))?
                .set(
                    "id",
                    Value::string(&format!("capability:{}", capability.id())),
                )?
                .set(
                    "title",
                    Value::string(&format!(
                        "{} (a grant no permission of this package maps)",
                        ono_kuang_protocol::family_title(capability)
                    )),
                )?
                .set("purpose", Value::Null)?
                .set(
                    "kind",
                    Value::string(
                        ono_kuang_protocol::default_kind(capability, template.scope.as_ref()).id(),
                    ),
                )?
                .set("phase", Value::string("explicit"))?
                .set(
                    "risk",
                    Value::string(
                        ono_kuang_protocol::minimum_risk(capability, template.scope.as_ref()).id(),
                    ),
                )?
                .set("recommended", Value::Bool(false))?
                .set("state", Value::string("legacy"))?
                .set("when", Value::string(grant.duration))?
                .set(
                    "scope",
                    Value::string(&describe_scope(capability, template.scope.as_ref())),
                )?
                .set(
                    "capabilities",
                    Value::list([Value::string(capability.id())]),
                )?
                .set(
                    "grants",
                    Value::list([map([
                        ("capability", Value::string(capability.id())),
                        (
                            "scope",
                            grant.scope.as_ref().map_or(Value::Null, |scope| {
                                json_value(&Json::Object(scope.clone()))
                            }),
                        ),
                        (
                            "enforcement",
                            Value::string(match template.enforcement() {
                                Enforcement::Broker => "broker",
                                Enforcement::Advisory => "advisory",
                            }),
                        ),
                        ("decision", Value::string("allow")),
                        ("source", Value::string(grant.source)),
                        ("duration", Value::string(grant.duration)),
                        ("grant", Value::Uuid(grant.id)),
                    ])]),
                )?
                .set(
                    "enforcement",
                    Value::string(match template.enforcement() {
                        Enforcement::Broker => "broker",
                        Enforcement::Advisory => "advisory",
                    }),
                )?
                .set("source", Value::string(grant.source))?
                .set("profile", Value::Null)?
                .set("derived", Value::Bool(true))?
                .set("decided_at", grant.granted_at.clone())?
                .build(),
        )
    }

    /// The human layer of an inspection (K11P §25.3).
    ///
    /// # Errors
    ///
    /// `provider.schema_violation` when a record does not fit its contract.
    pub fn inspection_human(
        &self,
        package: &Installed,
        management: &Management,
        instance: Option<&crate::kuang_host::Instance>,
    ) -> Result<crate::kuang_host::InspectionHuman, ErrorValue> {
        let permissions = self
            .permission_records(Some(package), true)?
            .into_iter()
            .map(RecordValue::into_value)
            .collect();
        let grants = self
            .capability_records(Some(&package.manifest.package.id))?
            .into_iter()
            .filter(|record| {
                record.get("decision").and_then(|d| d.as_str().ok()) == Some("allow")
                    && record.get("revoked_at").is_none_or(Value::is_null)
            })
            .map(RecordValue::into_value)
            .collect();
        Ok(crate::kuang_host::InspectionHuman {
            permissions,
            profiles: profiles_value(&package.manifest.permission_set()),
            readiness: self.readiness(package, management, instance).to_owned(),
            grants,
        })
    }
}

fn decision_when(decision: Option<&Decision>) -> &'static str {
    match decision.map(|decision| decision.record.duration.as_str()) {
        Some("session") => "session",
        Some("once") => "once",
        _ => "always",
    }
}

fn json_value(json: &Json) -> Value {
    ono_value::from_json(json, ono_value::builtin_schemas()).unwrap_or(Value::Null)
}

/// The profiles a package offers, as records with the host's own mutation flag (K11P §9.3).
#[must_use]
pub fn profiles_value(set: &PermissionSet) -> Value {
    Value::list(set.profiles.iter().map(|profile| {
        let adds_mutation = set
            .profile_descriptors(profile)
            .any(|descriptor| !descriptor.is_safe_default());
        map([
            ("name", Value::string(&profile.name)),
            ("title", Value::string(&profile_title(set, profile))),
            (
                "permissions",
                Value::list(profile.permissions.iter().map(|id| Value::string(id))),
            ),
            ("adds_mutation", Value::Bool(adds_mutation)),
        ])
    }))
}

/// A profile's title with the host's derived risk appended where it adds mutation
/// (K11P §9.3): `Operate — can change external resources`, never merely `Full`.
#[must_use]
pub fn profile_title(set: &PermissionSet, profile: &ono_kuang_protocol::AccessProfile) -> String {
    let adds_mutation = set
        .profile_descriptors(profile)
        .any(|descriptor| !descriptor.is_safe_default());
    if adds_mutation {
        format!("{} — can change external or host state", profile.title)
    } else {
        profile.title.clone()
    }
}

/// The signing key of a package, or `unsigned`: what a decision records as the publisher
/// identity it was consented to (K11P §22.3, §23.1).
#[must_use]
pub fn publisher_identity(package: &Installed) -> String {
    let signature = crate::kuang_host::signature_of(package);
    match &signature.document {
        Some(document) if signature.failure.is_none() => document.key().to_string(),
        _ => "unsigned".to_owned(),
    }
}

/// The concrete scope a grant template resolves to for `package` (ADR-0600 §3, ADR-0604):
/// a record as written; the package's own declared relation ids for `package-contributions`;
/// nothing yet for a scope derived at use or bound to a provider instance — the broker checks
/// the value in use, and the record says the enforcement is what it is.
#[must_use]
pub fn resolve_scope(
    package: &Installed,
    template: &GrantTemplate,
    override_scope: Option<&JsonMap<String, Json>>,
) -> Option<JsonMap<String, Json>> {
    let base = match &template.scope {
        None => None,
        Some(ScopeTemplate::Concrete(scope)) => Some(scope.clone()),
        Some(ScopeTemplate::PackageContributions) => {
            let id = &package.manifest.package.id;
            let ids: Vec<Json> = package
                .manifest
                .contributions
                .as_ref()
                .and_then(|contributions| contributions.relations.as_ref())
                .map(|shapes| {
                    shapes
                        .iter()
                        .filter_map(|shape| ono_spatial_core::relation::parse_shape(shape))
                        .map(|(from, to)| {
                            Json::String(ono_spatial_core::relation::contributed_id(id, from, to))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut scope = JsonMap::new();
            scope.insert("relations".to_owned(), Json::Array(ids));
            Some(scope)
        }
        Some(ScopeTemplate::RuntimeDerived | ScopeTemplate::ProviderInstance) => None,
    };
    match (base, override_scope) {
        (base, None) => base,
        (None, Some(named)) => Some(named.clone()),
        (Some(mut base), Some(named)) => {
            for (key, value) in named {
                base.insert(key.clone(), value.clone());
            }
            Some(base)
        }
    }
}

fn declared_class(manifest: &Manifest, capability: Capability) -> Option<DeclarationClass> {
    manifest
        .capability_requests()
        .find(|(_, request)| request.capability == capability)
        .map(|(class, _)| class)
}

/// The duration word a decision carries, as a `'static` the grant record can hold.
fn duration_word(duration: &str) -> &'static str {
    if duration == "session" {
        "session"
    } else {
        "always"
    }
}

/// Executes an `allow`: revokes what the permission minted before, mints a grant per template
/// and records the decision (ADR-0604 §1). Answers the grants.
#[allow(
    clippy::too_many_arguments,
    reason = "a decision names its package, permission, duration, source, profile, scope and request"
)]
pub fn allow(
    host: &mut Host,
    package: &Installed,
    descriptor: &PermissionDescriptor,
    duration: &str,
    source: &'static str,
    profile: Option<&str>,
    override_scope: Option<&JsonMap<String, Json>>,
    correlation: &str,
) -> Vec<Grant> {
    let id = package.manifest.package.id.clone();
    for grant in host.grants_of_permission(&id, &descriptor.id) {
        host.revoke_correlated(grant, correlation);
    }
    host.forget_denials(&id, &descriptor.id);
    let duration = duration_word(duration);
    let mut minted = Vec::new();
    for template in &descriptor.grants {
        let scope = resolve_scope(package, template, override_scope);
        let grant = host.grant(
            &id,
            template.capability,
            scope,
            declared_class(&package.manifest, template.capability),
            source,
            duration,
            None,
            GrantOrigin {
                permission: Some(descriptor.id.clone()),
                profile: profile.map(str::to_owned),
                correlation: Some(correlation.to_owned()),
            },
        );
        minted.push(grant);
    }
    host.record_decision(
        &id,
        &descriptor.id,
        "allow",
        duration,
        profile,
        minted.iter().map(|grant| grant.id.to_string()).collect(),
        descriptor
            .capabilities()
            .map(|capability| capability.id().to_owned())
            .collect(),
        &package.manifest.package.version,
        &publisher_identity(package),
    );
    host.record_host_event_correlated(
        &id,
        &descriptor.id,
        "permission.allow",
        true,
        Some(correlation.to_owned()),
    );
    minted
}

/// Executes a `deny`: revokes what the permission minted and records a deny the broker
/// honours ahead of any grant (ADR-0604 §2).
pub fn deny(
    host: &mut Host,
    package: &Installed,
    descriptor: &PermissionDescriptor,
    correlation: &str,
) {
    let id = package.manifest.package.id.clone();
    for grant in host.grants_of_permission(&id, &descriptor.id) {
        host.revoke_correlated(grant, correlation);
    }
    host.record_decision(
        &id,
        &descriptor.id,
        "deny",
        "always",
        None,
        Vec::new(),
        descriptor
            .capabilities()
            .map(|capability| capability.id().to_owned())
            .collect(),
        &package.manifest.package.version,
        &publisher_identity(package),
    );
    host.record_host_event_correlated(
        &id,
        &descriptor.id,
        "permission.deny",
        false,
        Some(correlation.to_owned()),
    );
}

/// Executes an `ask`: clears the decision, its grants and any remembered denial, so the next
/// concrete need asks again (ADR-0604 §2).
pub fn ask(
    host: &mut Host,
    package: &Installed,
    descriptor: &PermissionDescriptor,
    correlation: &str,
) {
    let id = package.manifest.package.id.clone();
    for grant in host.grants_of_permission(&id, &descriptor.id) {
        host.revoke_correlated(grant, correlation);
    }
    host.forget_decision(&id, &descriptor.id);
    host.forget_denials(&id, &descriptor.id);
    host.record_host_event_correlated(
        &id,
        &descriptor.id,
        "permission.ask",
        true,
        Some(correlation.to_owned()),
    );
}

/// Applies a profile: every permission it names is allowed, and every automatic permission is
/// included (K11P §9, ADR-0604 §2). Answers the grants minted.
pub fn apply_profile(
    host: &mut Host,
    package: &Installed,
    set: &PermissionSet,
    profile: &ono_kuang_protocol::AccessProfile,
    duration: &str,
    correlation: &str,
) -> Vec<Grant> {
    let mut minted = Vec::new();
    let id = package.manifest.package.id.clone();
    host.record_host_event_correlated(
        &id,
        &profile.name,
        "permission.profile_apply",
        true,
        Some(correlation.to_owned()),
    );
    for descriptor in set.automatic() {
        if host
            .decision(&id, &descriptor.id)
            .is_some_and(|decision| decision.record.decision == "deny")
        {
            continue;
        }
        minted.extend(allow(
            host,
            package,
            descriptor,
            duration,
            "automatic",
            Some(&profile.name),
            None,
            correlation,
        ));
    }
    let named: Vec<PermissionDescriptor> = set.profile_descriptors(profile).cloned().collect();
    for descriptor in &named {
        if descriptor.is_automatic() {
            continue;
        }
        minted.extend(allow(
            host,
            package,
            descriptor,
            duration,
            "prompt",
            Some(&profile.name),
            None,
            correlation,
        ));
    }
    minted
}

/// Includes the automatic permissions of a package placed by hand rather than installed —
/// session grants, unless a decision denies them — so a bounded extension-local privilege is
/// what the manifest says it is whichever way the package arrived (K11P §7.1, ADR-0600 §2).
pub fn ensure_automatic(host: &mut Host, package: &Installed) {
    let id = package.manifest.package.id.clone();
    let set = package.manifest.permission_set();
    for descriptor in set.automatic() {
        if host
            .decision(&id, &descriptor.id)
            .is_some_and(|decision| decision.record.decision == "deny")
        {
            continue;
        }
        let held = descriptor.grants.iter().all(|template| {
            host.standing_grants(&id)
                .any(|grant| grant.capability == template.capability)
        });
        if held {
            continue;
        }
        for template in &descriptor.grants {
            let scope = resolve_scope(package, template, None);
            host.grant(
                &id,
                template.capability,
                scope,
                declared_class(&package.manifest, template.capability),
                "automatic",
                "session",
                None,
                GrantOrigin {
                    permission: Some(descriptor.id.clone()),
                    profile: None,
                    correlation: None,
                },
            );
        }
    }
}

/// The install-time permissions of the package's selected profile that nothing has decided
/// `allow`, plus every required capability nothing grants — what keeps a package short of
/// `ready` (ADR-0602 §2).
#[must_use]
pub fn undecided(host: &Host, package: &Installed, management: &Management) -> Vec<String> {
    let id = package.manifest.package.id.as_str();
    let set = package.manifest.permission_set();
    let profile = management
        .profile
        .as_deref()
        .unwrap_or(ono_kuang_protocol::RECOMMENDED);
    let mut missing = Vec::new();
    if let Some(profile) = set.profile(profile) {
        for descriptor in set.profile_descriptors(profile) {
            if descriptor.phase != PermissionPhase::Install {
                continue;
            }
            let held = descriptor.grants.iter().all(|template| {
                host.standing_grants(id)
                    .any(|grant| grant.capability == template.capability)
            });
            if !held {
                missing.push(descriptor.id.clone());
            }
        }
    }
    for request in &package.manifest.required_capabilities {
        let held = host
            .standing_grants(id)
            .any(|grant| grant.capability == request.capability);
        let automatic = set
            .for_capability(request.capability)
            .any(PermissionDescriptor::is_automatic);
        if !held && !automatic {
            missing.push(format!("capability:{}", request.capability.id()));
        }
    }
    missing
}

// --- `set permission`, K11P §16.2 --------------------------------------------------------------

/// What `set permission` was told.
#[derive(Debug, Default, Clone)]
pub struct SetPermission {
    /// The package reference.
    pub plugin: String,
    /// The permission id, when one is decided rather than a profile applied.
    pub permission: Option<String>,
    /// `--profile <name>`.
    pub profile: Option<String>,
    /// `--decision allow|deny|ask`.
    pub decision: Option<String>,
    /// `--duration session|always`.
    pub duration: Option<String>,
    /// `--scope key=value`, repeatable.
    pub scopes: Vec<String>,
    /// `--confirm`.
    pub confirm: bool,
}

impl SetPermission {
    /// Reads the words after `set permission`.
    #[must_use]
    pub fn from_words(words: &[String]) -> Self {
        let mut read = Self::default();
        let mut positional = Vec::new();
        let mut iter = words.iter().skip(1);
        while let Some(word) = iter.next() {
            match word.as_str() {
                "--profile" => read.profile = iter.next().cloned(),
                "--decision" => read.decision = iter.next().cloned(),
                "--duration" => read.duration = iter.next().cloned(),
                "--scope" => {
                    if let Some(scope) = iter.next() {
                        read.scopes.push(scope.clone());
                    }
                }
                "--confirm" => read.confirm = true,
                other if other.starts_with("--") => {}
                other => positional.push(other.to_owned()),
            }
        }
        let mut positional = positional.into_iter();
        read.plugin = positional.next().unwrap_or_default();
        read.permission = positional.next();
        read
    }
}

/// Runs `set permission <plugin> [--profile <name>] [<permission> --decision <d>]`
/// (K11P §15.2, §16.2; ADR-0604 §2), answering the changed rows.
///
/// # Errors
///
/// `plugin.not_found`, `permission.invalid_profile`, an unknown permission id, a decision
/// that needs a confirmation this context cannot give, or the store's I/O failure.
pub fn set_permission(session: &mut Session, request: &SetPermission) -> Eval<Produced> {
    if request.plugin.is_empty() {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::PluginNotFound,
                "`set permission` needs the package whose permission to decide",
            )
            .with_help("`set permission <plugin> --profile <name>` or `set permission <plugin> <permission> --decision allow|deny|ask` (K11P §16.2)"),
        ));
    }
    session.publish_host();
    let package = session
        .with_kuang(|host| host.resolve_installed(&request.plugin))
        .map_err(Flow::Failed)?;
    let id = package.manifest.package.id.clone();
    let name = package.manifest.package.name.clone();
    let set = package.manifest.permission_set();
    let correlation = format!("{id}:set:{}", jiff::Timestamp::now().as_millisecond());
    let interactive = session.is_interactive();
    let changed: Vec<String> = match (&request.profile, &request.permission) {
        (Some(profile_name), None) => {
            let Some(profile) = set.profile(profile_name).cloned() else {
                return Err(Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::KuangPermissionInvalidProfile,
                        format!("`{name}` offers no access profile named `{profile_name}`"),
                    )
                    .with_help(format!(
                        "it offers {}",
                        if set.profiles.is_empty() {
                            "no profile at all: it asks for nothing".to_owned()
                        } else {
                            set.profiles
                                .iter()
                                .map(|profile| format!("`{}`", profile.name))
                                .collect::<Vec<_>>()
                                .join(", ")
                        }
                    )),
                ));
            };
            let widening: Vec<&PermissionDescriptor> = set
                .profile_descriptors(&profile)
                .filter(|descriptor| !descriptor.is_safe_default())
                .collect();
            if !widening.is_empty() {
                confirm_widening(
                    session,
                    &name,
                    &format!(
                        "This profile adds permission to {}.",
                        widening
                            .iter()
                            .map(|descriptor| lowercase_first(&descriptor.title))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    &widening,
                    request.confirm,
                    true,
                )?;
            }
            let duration = request
                .duration
                .clone()
                .unwrap_or_else(|| "always".to_owned());
            let profile_permissions: Vec<String> = profile.permissions.clone();
            let outcome = session.with_kuang(|host| {
                apply_profile(host, &package, &set, &profile, &duration, &correlation);
                let mut management = host.management(&id);
                management.profile = Some(profile.name.clone());
                host.write_management(&id, &management)?;
                host.persist_permissions()
            });
            outcome.map_err(Flow::Failed)?;
            if interactive {
                eprintln!("{name} access profile: {}", profile.name);
            }
            profile_permissions
        }
        (None, Some(permission_id)) => {
            let Some(descriptor) = set.descriptor(permission_id).cloned() else {
                return Err(Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::ResolveTargetNotFound,
                        format!("`{name}` declares no permission `{permission_id}`"),
                    )
                    .with_help(format!("`get permission {name} --all` lists them")),
                ));
            };
            let decision = request
                .decision
                .clone()
                .unwrap_or_else(|| "allow".to_owned());
            let duration = request
                .duration
                .clone()
                .unwrap_or_else(|| "always".to_owned());
            if !matches!(decision.as_str(), "allow" | "deny" | "ask") {
                return Err(Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        format!("`{decision}` is not a decision"),
                    )
                    .with_help("`--decision allow`, `deny` or `ask` (K11P §16.2)"),
                ));
            }
            if !matches!(duration.as_str(), "session" | "always") {
                return Err(Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        format!("`{duration}` is not a duration a decision can hold itself to"),
                    )
                    .with_help("`--duration session` or `always`; `once` is what a just-in-time answer offers at the moment of use (K11P §14.4)"),
                ));
            }
            let scope = scope_from_words(&descriptor, &request.scopes).map_err(Flow::Failed)?;
            // A just-in-time permission allowed for a named program is the `always` answer of
            // K11P §14.4 written down — the remedy `permission.required` itself names — and not
            // a widening; the same permission allowed for every program is.
            let narrowed_jit = descriptor.phase == PermissionPhase::Jit
                && descriptor.risk < ono_kuang_protocol::PermissionRisk::Mutate
                && scope.is_some();
            if decision == "allow" && !descriptor.is_safe_default() && !narrowed_jit {
                confirm_widening(
                    session,
                    &name,
                    &format!(
                        "{name} will be allowed to {}.",
                        lowercase_first(&descriptor.title)
                    ),
                    &[&descriptor],
                    request.confirm,
                    false,
                )?;
            }
            let outcome = session.with_kuang(|host| {
                let mut dropped = Vec::new();
                match decision.as_str() {
                    "allow" => {
                        dropped = dropped_by_scope(host, &package, &descriptor, scope.as_ref());
                        allow(
                            host,
                            &package,
                            &descriptor,
                            &duration,
                            "prompt",
                            None,
                            scope.as_ref(),
                            &correlation,
                        );
                    }
                    "deny" => deny(host, &package, &descriptor, &correlation),
                    _ => ask(host, &package, &descriptor, &correlation),
                }
                host.persist_permissions().map(|()| dropped)
            });
            let dropped = outcome.map_err(Flow::Failed)?;
            // `--scope` replaces the values of each key it names (ADR-0857). A replacement that
            // leaves out a value the permission declared or held narrows it, and that is said
            // on the line that did it rather than discovered at the next refusal (issue #128).
            if !dropped.is_empty() {
                crate::report::notice(&format!(
                    "{name} {}: `--scope` sets the whole list for each key it names, so the \
                     permission no longer covers {}; name every value to keep, e.g. `--scope \
                     \"paths=<one>,<two>\"`",
                    descriptor.id,
                    dropped
                        .iter()
                        .map(|(key, values)| format!("{key} {}", values.join(", ")))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            vec![descriptor.id.clone()]
        }
        (Some(_), Some(_)) => {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    "`set permission` applies a profile or decides one permission, not both",
                )
                .with_help("`--profile <name>`, or `<permission> --decision <d>` (K11P §16.2)"),
            ));
        }
        (None, None) => {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    "`set permission` needs `--profile <name>` or a permission and `--decision`",
                )
                .with_help(format!("`get permission {name}` shows what can be decided")),
            ));
        }
    };
    // A running instance evaluates the new policy at its next call (spec §31.19).
    let (policy, instance) = session.with_kuang(|host| (host.policy_for(&id), host.plugin(&id)));
    if let Some(instance) = instance
        && let Some(runtime) = session.runtime_handle()
    {
        runtime.block_on(instance.update_policy(policy));
    }
    let values = session
        .with_kuang(|host| host.permission_records(Some(&package), true))
        .map_err(Flow::Failed)?
        .into_iter()
        .filter(|record| {
            record
                .get("id")
                .and_then(|value| value.as_str().ok())
                .is_some_and(|permission| changed.iter().any(|wanted| wanted == permission))
        })
        .map(RecordValue::into_value)
        .collect();
    Ok(Produced {
        values,
        failure: None,
    })
}

/// A widening decision — mutation, or anything not a safe default — is summarised and asked
/// about interactively, needs `--confirm` in a script, and is never accepted unattended when
/// destructive (K11P §7.5, §15.2, ADR-0602 §4).
fn confirm_widening(
    session: &Session,
    name: &str,
    summary: &str,
    widening: &[&PermissionDescriptor],
    confirmed: bool,
    is_profile: bool,
) -> Eval<()> {
    let destructive = widening
        .iter()
        .any(|descriptor| descriptor.risk == ono_kuang_protocol::PermissionRisk::Destructive);
    if session.is_interactive() {
        eprintln!("{summary}");
        eprintln!(
            "Individual mutate/destructive commands keep their own confirmations and dry-run rules."
        );
        loop {
            let answer = read_answer("Allow changes? [y/N/details] ").unwrap_or_default();
            match answer.trim().to_lowercase().as_str() {
                "y" | "yes" => return Ok(()),
                "d" | "details" => {
                    for descriptor in widening {
                        eprintln!("{}", describe_descriptor(descriptor));
                    }
                }
                _ => {
                    return Err(Flow::Failed(
                        ErrorValue::new(
                            ErrorCode::SafetyConfirmationRequired,
                            format!("{name}: the change was not confirmed"),
                        )
                        .with_help("nothing was decided"),
                    ));
                }
            }
        }
    }
    if destructive {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::KuangPermissionEscalationRequiresConfirmation,
                format!(
                    "{name}: {} is destructive and is never enabled without a person (K11P §7.5)",
                    widening
                        .iter()
                        .map(|descriptor| format!("`{}`", descriptor.id))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
            .with_help("run this interactively; no flag accepts a destructive permission"),
        ));
    }
    if confirmed {
        return Ok(());
    }
    Err(Flow::Failed(
        ErrorValue::new(
            ErrorCode::KuangPermissionEscalationRequiresConfirmation,
            format!(
                "{name}: {} widens authority beyond the recommended access and this context \
                 cannot ask",
                if is_profile {
                    "the profile"
                } else {
                    "the permission"
                }
            ),
        )
        .with_help(
            "add `--confirm` to accept the change deliberately, or run this interactively \
             (K11P §20.2, ADR-0602 §4)",
        ),
    ))
}

/// One descriptor in full, for `details` (K11P §5.3).
#[must_use]
pub fn describe_descriptor(descriptor: &PermissionDescriptor) -> String {
    let mut lines = vec![format!(
        "  {} [{}] — kind {}, risk {}, decided {}{}",
        descriptor.title,
        descriptor.id,
        descriptor.kind,
        descriptor.risk,
        descriptor.phase,
        if descriptor.derived {
            ", wording the host's own"
        } else {
            ""
        }
    )];
    if let Some(purpose) = &descriptor.purpose {
        lines.push(format!("    purpose: {purpose}"));
    }
    for template in &descriptor.grants {
        lines.push(format!(
            "    capability {} — scope {}; enforcement {}",
            template.capability,
            template.scope.as_ref().map_or_else(
                || "unscoped".to_owned(),
                |scope| scope.as_json().to_string()
            ),
            match template.enforcement() {
                Enforcement::Broker => "broker",
                Enforcement::Advisory => "advisory — recorded and audited, not a boundary",
            }
        ));
    }
    lines.join("\n")
}

/// Reads the repeated `--scope key=value[,value]` words (ADR-0264's spelling) against the
/// families the permission grants; a key none of them declares is refused.
fn scope_from_words(
    descriptor: &PermissionDescriptor,
    written: &[String],
) -> Result<Option<JsonMap<String, Json>>, ErrorValue> {
    if written.is_empty() {
        return Ok(None);
    }
    let mut scope = JsonMap::new();
    for entry in written {
        let Some((key, values)) = entry.split_once('=') else {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`--scope {entry}` names no key"),
            )
            .with_help("`--scope <key>=<value>[,<value>]`, repeated once per key (spec §31.16)"));
        };
        let declared = descriptor.grants.iter().any(|template| {
            template
                .capability
                .scope_keys()
                .iter()
                .any(|declared| declared.name == key)
        });
        if !declared {
            return Err(ErrorValue::new(
                ErrorCode::TypeUnknownField,
                format!(
                    "`{}` grants no capability with a scope key `{key}`",
                    descriptor.id
                ),
            )
            .with_help("spec §31.16: a key the capability does not declare is invalid"));
        }
        scope.insert(
            key.to_owned(),
            Json::Array(
                values
                    .split(',')
                    .filter(|value| !value.is_empty())
                    .map(|value| Json::String(value.to_owned()))
                    .collect(),
            ),
        );
    }
    Ok(Some(scope))
}

/// What a `--scope` replacement leaves out, per key it names, in first-seen order: the values
/// the permission's declared scope and its standing grant listed that the new value does not
/// (issue #128, ADR-0857). Read before the replacement revokes the grant it replaces.
fn dropped_by_scope(
    host: &Host,
    package: &Installed,
    descriptor: &PermissionDescriptor,
    named: Option<&JsonMap<String, Json>>,
) -> Vec<(String, Vec<String>)> {
    let Some(named) = named else {
        return Vec::new();
    };
    let words = |value: &Json| -> Vec<String> {
        match value {
            Json::Array(items) => items
                .iter()
                .map(|item| match item {
                    Json::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .collect(),
            Json::String(text) => vec![text.clone()],
            other => vec![other.to_string()],
        }
    };
    let id = package.manifest.package.id.as_str();
    let mut before: Vec<JsonMap<String, Json>> = Vec::new();
    for template in &descriptor.grants {
        before.extend(resolve_scope(package, template, None));
        before.extend(
            host.standing_grants(id)
                .filter(|grant| {
                    grant.capability == template.capability
                        && grant.permission.as_deref() == Some(descriptor.id.as_str())
                })
                .filter_map(|grant| grant.scope.clone()),
        );
    }
    let mut dropped = Vec::new();
    for (key, value) in named {
        let kept = words(value);
        let mut left_out: Vec<String> = Vec::new();
        for scope in &before {
            for word in scope.get(key).map(&words).unwrap_or_default() {
                if !kept.contains(&word) && !left_out.contains(&word) {
                    left_out.push(word);
                }
            }
        }
        if !left_out.is_empty() {
            dropped.push((key.clone(), left_out));
        }
    }
    dropped
}

/// Prints a question on stderr and reads one line of stdin. `None` when stdin is gone.
pub(crate) fn read_answer(question: &str) -> Option<String> {
    eprint!("{question}");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).ok()?;
    Some(answer)
}

fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

// --- the shell's consent source, K11P §14, ADR-0603 §2 -----------------------------------------

/// Who answers a just-in-time request in the shell: a person at the terminal, or nobody.
#[derive(Debug)]
pub struct ShellConsent {
    tables: Arc<Mutex<crate::session_provider::SessionTables>>,
    interactive: bool,
    package: Installed,
}

impl ShellConsent {
    /// A source for `package`, asking through this session's terminal when there is one.
    #[must_use]
    pub fn new(
        tables: Arc<Mutex<crate::session_provider::SessionTables>>,
        interactive: bool,
        package: Installed,
    ) -> Self {
        Self {
            tables,
            interactive,
            package,
        }
    }

    fn with_host<T>(&self, body: impl FnOnce(&mut Host) -> T) -> T {
        let mut tables = self
            .tables
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        body(&mut tables.kuang)
    }

    fn remedy(&self, request: &ConsentRequest) -> String {
        let name = &self.package.manifest.package.name;
        let scope = ono_kuang_supervisor::scope_words(request.scope.as_ref());
        format!(
            "`set permission {name} {} --decision allow{scope}`, or `grant capability {} --plugin {}{scope} --duration always`",
            request.permission.id,
            request.capability.id(),
            request.package
        )
    }

    /// Records an answer that lasts longer than once, as the grant and the decision it is
    /// (ADR-0603 §2).
    fn keep(&self, request: &ConsentRequest, duration: ConsentDuration, correlation: &str) {
        let package = self.package.clone();
        let descriptor = request.permission.clone();
        let word = duration.word();
        self.with_host(|host| {
            allow(
                host,
                &package,
                &descriptor,
                word,
                "prompt",
                None,
                request.scope.as_ref(),
                correlation,
            );
            if duration == ConsentDuration::Always {
                let _ = host.persist_permissions();
            }
        });
    }
}

impl ConsentSource for ShellConsent {
    fn consent(&self, request: &ConsentRequest) -> ConsentAnswer {
        let subject = request.subject();
        let remedy = self.remedy(request);
        let remembered = self.with_host(|host| {
            host.denied_in_session(&request.package, &request.permission.id, &subject)
        });
        if remembered {
            return ConsentAnswer::denied(request, &remedy);
        }
        if !self.interactive {
            return ConsentAnswer::required(request, &remedy);
        }
        let name = &self.package.manifest.package.name;
        let display = capitalise(name);
        let correlation = format!(
            "{}:jit:{}",
            request.package,
            jiff::Timestamp::now().as_millisecond()
        );
        loop {
            eprintln!();
            match request.program() {
                Some(program) => {
                    eprintln!("{display} needs to run:");
                    eprintln!("  {program} {}", request.arguments.join(" "));
                }
                None => {
                    eprintln!(
                        "{display} needs permission to {}:",
                        lowercase_first(&request.permission.title)
                    );
                    eprintln!("  {subject}");
                }
            }
            eprintln!();
            eprintln!("Reason:");
            eprintln!(
                "  {}",
                request
                    .permission
                    .purpose
                    .clone()
                    .unwrap_or_else(|| lowercase_first(&request.permission.title))
            );
            eprintln!();
            let question = match request.program() {
                Some(_) => "Allow this helper?",
                None => "Allow this?",
            };
            eprintln!("{question}");
            eprintln!(
                "  [o] once  [s] this session  [a] always for this {}  [n] deny  [d] details",
                match request.program() {
                    Some(_) => "program",
                    None => "scope",
                }
            );
            let answer = read_answer("> ").unwrap_or_default();
            match answer.trim().to_lowercase().as_str() {
                "o" | "once" => {
                    return ConsentAnswer::Allow {
                        duration: ConsentDuration::Once,
                        scope: None,
                    };
                }
                "s" | "session" => {
                    self.keep(request, ConsentDuration::Session, &correlation);
                    eprintln!("Allowed {subject} for {display} this session.");
                    return ConsentAnswer::Allow {
                        duration: ConsentDuration::Session,
                        scope: None,
                    };
                }
                "a" | "always" => {
                    self.keep(request, ConsentDuration::Always, &correlation);
                    eprintln!("Allowed {subject} for {display}.");
                    return ConsentAnswer::Allow {
                        duration: ConsentDuration::Always,
                        scope: None,
                    };
                }
                "d" | "details" => {
                    eprintln!("{}", describe_descriptor(&request.permission));
                    eprintln!(
                        "    this call: capability {}, {}",
                        request.capability,
                        request
                            .uses
                            .iter()
                            .map(|(key, value)| format!("{key} {value}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    eprintln!(
                        "    an `always` answer grants exactly this scope: {}",
                        request.scope.as_ref().map_or_else(
                            || "unscoped".to_owned(),
                            |scope| Json::Object(scope.clone()).to_string()
                        )
                    );
                }
                _ => {
                    let package = self.package.clone();
                    self.with_host(|host| {
                        host.remember_denial(
                            &package.manifest.package.id,
                            &request.permission.id,
                            &subject,
                        );
                    });
                    return ConsentAnswer::denied(request, &remedy);
                }
            }
        }
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// A structured refusal of a permission the shell itself checks before an invocation, in the
/// human-first wording of K11P §24.4 — `permission.denied` for an explicit permission nobody
/// enabled, with the elevation named.
#[must_use]
pub fn denied_error(
    package: &Installed,
    descriptor: &PermissionDescriptor,
    operation: &str,
) -> KuangError {
    let name = &package.manifest.package.name;
    KuangError::new(
        KuangErrorCode::PermissionDenied,
        format!(
            "{} is installed without permission to {}; {} is not allowed",
            capitalise(name),
            lowercase_first(&descriptor.title),
            operation
        ),
    )
    .with_help(format!(
        "enable the \"{}\" permission to continue: `set permission {name} {} --decision allow`{}",
        descriptor.title,
        descriptor.id,
        if descriptor.phase == PermissionPhase::Explicit {
            String::new()
        } else {
            format!(", or `set permission {name} --profile recommended`")
        }
    ))
    .with_metadata("plugin", Json::String(package.manifest.package.id.clone()))
    .with_metadata("permission", Json::String(descriptor.id.clone()))
    .with_metadata(
        "capability",
        Json::Array(
            descriptor
                .capabilities()
                .map(|capability| Json::String(capability.id().to_owned()))
                .collect(),
        ),
    )
    .with_metadata("operation", Json::String(operation.to_owned()))
}

/// Whether the run should leave with success: a helper for callers that only need the status.
#[must_use]
pub const fn status_of(produced: &Produced) -> ExitStatus {
    if produced.failure.is_some() {
        ExitStatus::FAILURE
    } else {
        ExitStatus::SUCCESS
    }
}
