//! Capability policy and the broker's evaluation order (spec §31.19).
//!
//! Precedence, spec §31.19 verbatim: "system deny > user deny > scoped grant > plugin request >
//! default deny". A plugin request produces a prompt under `ask` — and this supervisor is a
//! library with no prompt to offer, so `ask` resolves to `deny`, exactly as a non-interactive
//! session must: a prompt nobody can answer is a denial that pretends otherwise
//! (`docs/spec/kuang/capabilities.v1.yaml` → `grant.decisions`).

use globset::{Glob, GlobSetBuilder};
use ono_kuang_protocol::{Capability, KuangError, KuangErrorCode};
use serde_json::{Map as JsonMap, Value as Json};

/// One standing grant: a capability, optionally bounded by a scope.
#[derive(Debug, Clone, PartialEq)]
pub struct Grant {
    /// The granted family.
    pub capability: Capability,
    /// The granted scope. `None` grants the capability unscoped.
    pub scope: Option<JsonMap<String, Json>>,
}

/// The policy the broker evaluates on every call. Deny by default is the floor, not a fallback
/// (spec §31.80).
#[derive(Debug, Clone, Default)]
pub struct Policy {
    system_denies: Vec<Capability>,
    user_denies: Vec<Capability>,
    grants: Vec<Grant>,
}

/// Where a denial came from, for the audit trail and the error message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DenialSource {
    /// A deny in system policy. Nothing overrides it.
    System,
    /// A deny in the operator's policy.
    User,
    /// No rule matched: default deny.
    Default,
}

/// The broker's answer for one capability use.
#[derive(Debug, Clone, PartialEq)]
pub enum Evaluation {
    /// A grant covers the use. Carries the granted scope for the audit record.
    Allowed(Grant),
    /// No grant covers the capability at all.
    Denied(DenialSource),
    /// A grant exists, but the concrete use falls outside its scope.
    ScopeViolation {
        /// The grant whose scope was exceeded.
        grant: Grant,
        /// The attempted value, for the error metadata and the audit trail.
        attempted: String,
    },
}

/// A concrete scope-relevant value one call is about to use, checked against the grant —
/// against the value the operation will actually use, never a name for it (ADR-0022 §3).
#[derive(Debug, Clone, PartialEq)]
pub enum ScopeUse {
    /// A resolved filesystem path, matched against `path-glob` scope entries.
    Path {
        /// The scope key, e.g. `paths`.
        key: &'static str,
        /// The resolved path.
        value: String,
    },
    /// A name matched against `name-list`/`id-list` scope entries (exact or `*`-glob).
    Name {
        /// The scope key, e.g. `secrets`.
        key: &'static str,
        /// The concrete name.
        value: String,
    },
    /// A port matched against `port-list` scope entries.
    Port {
        /// The scope key, e.g. `ports`.
        key: &'static str,
        /// The concrete port.
        value: u16,
    },
}

impl ScopeUse {
    fn key(&self) -> &'static str {
        match self {
            ScopeUse::Path { key, .. }
            | ScopeUse::Name { key, .. }
            | ScopeUse::Port { key, .. } => key,
        }
    }

    fn display(&self) -> String {
        match self {
            ScopeUse::Path { value, .. } | ScopeUse::Name { value, .. } => value.clone(),
            ScopeUse::Port { value, .. } => value.to_string(),
        }
    }
}

impl Policy {
    /// An empty policy: everything denied by default.
    #[must_use]
    pub fn deny_all() -> Self {
        Self::default()
    }

    /// Adds a grant for `capability`, bounded by `scope` (or unscoped for `None`).
    #[must_use]
    pub fn grant(mut self, capability: Capability, scope: Option<JsonMap<String, Json>>) -> Self {
        self.grants.push(Grant { capability, scope });
        self
    }

    /// Adds an operator deny. Overrides any grant for the family.
    #[must_use]
    pub fn deny(mut self, capability: Capability) -> Self {
        self.user_denies.push(capability);
        self
    }

    /// Adds a system deny. Nothing overrides it, and no prompt is offered for it (spec §31.19).
    #[must_use]
    pub fn deny_system(mut self, capability: Capability) -> Self {
        self.system_denies.push(capability);
        self
    }

    /// Whether any grant exists for `capability`, before scope checking.
    #[must_use]
    pub fn grants_capability(&self, capability: Capability) -> bool {
        !self.system_denies.contains(&capability)
            && !self.user_denies.contains(&capability)
            && self
                .grants
                .iter()
                .any(|grant| grant.capability == capability)
    }

    /// Evaluates one concrete use in precedence order (spec §31.19).
    #[must_use]
    pub fn evaluate(&self, capability: Capability, used: &[ScopeUse]) -> Evaluation {
        if self.system_denies.contains(&capability) {
            return Evaluation::Denied(DenialSource::System);
        }
        if self.user_denies.contains(&capability) {
            return Evaluation::Denied(DenialSource::User);
        }
        let mut nearest_violation = None;
        for grant in self
            .grants
            .iter()
            .filter(|grant| grant.capability == capability)
        {
            match scope_covers(grant, used) {
                Ok(()) => return Evaluation::Allowed(grant.clone()),
                Err(attempted) => {
                    nearest_violation.get_or_insert(Evaluation::ScopeViolation {
                        grant: grant.clone(),
                        attempted,
                    });
                }
            }
        }
        nearest_violation.unwrap_or(Evaluation::Denied(DenialSource::Default))
    }
}

/// Whether the grant's scope covers every concrete value the call uses.
///
/// A grant with no scope covers everything the capability permits. A grant that scopes a key
/// covers only uses whose value matches; a scope key the grant does not carry leaves that key
/// unconstrained. "A scoped grant covers a request only if the request's scope is a subset of
/// the grant's. Overlap is not coverage" (`capabilities.v1.yaml` → `grant.precedence`).
fn scope_covers(grant: &Grant, used: &[ScopeUse]) -> Result<(), String> {
    let Some(scope) = &grant.scope else {
        return Ok(());
    };
    for use_ in used {
        let Some(allowed) = scope.get(use_.key()) else {
            // The grant does not bound this key; the use is inside the grant.
            continue;
        };
        let allowed_list: Vec<String> = match allowed {
            Json::Array(items) => items
                .iter()
                .map(|item| match item {
                    Json::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .collect(),
            Json::String(text) => vec![text.clone()],
            _ => return Err(use_.display()),
        };
        let covered = match use_ {
            ScopeUse::Path { value, .. } => path_covered(&allowed_list, value),
            ScopeUse::Name { value, .. } => allowed_list
                .iter()
                .any(|pattern| name_matches(pattern, value)),
            ScopeUse::Port { value, .. } => allowed_list
                .iter()
                .any(|allowed| allowed.parse::<u16>().is_ok_and(|port| port == *value)),
        };
        if !covered {
            return Err(use_.display());
        }
    }
    Ok(())
}

fn path_covered(patterns: &[String], value: &str) -> bool {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder
        .build()
        .map(|set| set.is_match(value))
        .unwrap_or(false)
}

fn name_matches(pattern: &str, value: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        pattern == value
    }
}

/// The structured denial for an evaluation that did not allow the call, carrying the metadata
/// `docs/spec/kuang/errors.v1.yaml` promises: the attempted value and the granted scope.
#[must_use]
pub fn denial_error(capability: Capability, evaluation: &Evaluation) -> KuangError {
    match evaluation {
        Evaluation::Allowed(_) => KuangError::new(
            KuangErrorCode::CapabilityDenied,
            "internal: an allowed evaluation is not a denial",
        ),
        Evaluation::Denied(source) => {
            let reason = match source {
                DenialSource::System => "denied by system policy",
                DenialSource::User => "denied by operator policy",
                DenialSource::Default => "no grant covers it; deny by default (spec §31.19)",
            };
            KuangError::new(
                KuangErrorCode::CapabilityDenied,
                format!("`{capability}` is not granted: {reason}"),
            )
            .with_help("`get capability --plugin <id>` shows what the package holds")
        }
        Evaluation::ScopeViolation { grant, attempted } => KuangError::new(
            KuangErrorCode::CapabilityScopeViolation,
            format!("`{capability}` is granted, but `{attempted}` is outside the granted scope"),
        )
        .with_metadata("attempted", Json::String(attempted.clone()))
        .with_metadata(
            "granted",
            grant.scope.clone().map_or(Json::Null, Json::Object),
        )
        .with_help("widen the scope deliberately with `grant capability --scope`, or leave it"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths_scope(patterns: &[&str]) -> Option<JsonMap<String, Json>> {
        let mut scope = JsonMap::new();
        scope.insert(
            "paths".to_owned(),
            Json::Array(
                patterns
                    .iter()
                    .map(|p| Json::String((*p).to_owned()))
                    .collect(),
            ),
        );
        Some(scope)
    }

    #[test]
    fn should_deny_by_default_when_no_rule_matches() {
        let policy = Policy::deny_all();
        assert_eq!(
            policy.evaluate(Capability::ClockRead, &[]),
            Evaluation::Denied(DenialSource::Default)
        );
    }

    #[test]
    fn should_let_a_system_deny_override_a_grant() {
        // spec §31.19: system deny > scoped grant, and no prompt is offered for it.
        let policy = Policy::deny_all()
            .grant(Capability::FilesystemRead, None)
            .deny_system(Capability::FilesystemRead);
        assert_eq!(
            policy.evaluate(Capability::FilesystemRead, &[]),
            Evaluation::Denied(DenialSource::System)
        );
    }

    #[test]
    fn should_allow_a_path_inside_the_granted_scope_and_refuse_one_outside() {
        let policy =
            Policy::deny_all().grant(Capability::FilesystemRead, paths_scope(&["/var/log/**"]));
        let inside = policy.evaluate(
            Capability::FilesystemRead,
            &[ScopeUse::Path {
                key: "paths",
                value: "/var/log/syslog".into(),
            }],
        );
        assert!(matches!(inside, Evaluation::Allowed(_)));
        let outside = policy.evaluate(
            Capability::FilesystemRead,
            &[ScopeUse::Path {
                key: "paths",
                value: "/etc/shadow".into(),
            }],
        );
        let Evaluation::ScopeViolation { attempted, .. } = outside else {
            panic!("a path outside the scope is a scope violation, not a plain denial");
        };
        assert_eq!(attempted, "/etc/shadow");
    }

    #[test]
    fn should_treat_overlap_as_no_coverage_when_two_grants_each_miss() {
        // "Overlap is not coverage": the second grant covers the path, and is found.
        let policy = Policy::deny_all()
            .grant(Capability::FilesystemRead, paths_scope(&["/var/log/**"]))
            .grant(Capability::FilesystemRead, paths_scope(&["/etc/**"]));
        let evaluation = policy.evaluate(
            Capability::FilesystemRead,
            &[ScopeUse::Path {
                key: "paths",
                value: "/etc/hosts".into(),
            }],
        );
        assert!(matches!(evaluation, Evaluation::Allowed(_)));
    }

    #[test]
    fn should_match_names_by_glob_when_the_scope_uses_one() {
        let mut scope = JsonMap::new();
        scope.insert(
            "units".to_owned(),
            Json::Array(vec![Json::String("web-*".into())]),
        );
        let policy = Policy::deny_all().grant(Capability::ServiceRead, Some(scope));
        let allowed = policy.evaluate(
            Capability::ServiceRead,
            &[ScopeUse::Name {
                key: "units",
                value: "web-frontend.service".into(),
            }],
        );
        assert!(matches!(allowed, Evaluation::Allowed(_)));
        let refused = policy.evaluate(
            Capability::ServiceRead,
            &[ScopeUse::Name {
                key: "units",
                value: "sshd.service".into(),
            }],
        );
        assert!(matches!(refused, Evaluation::ScopeViolation { .. }));
    }
}
