//! `install plugin` as one transaction (K11P §12, §13, §20, §21; ADR-0602): resolve, verify,
//! plan, consent, stage, place, enable, decide, register, report ready — with every step before
//! the rename writing nothing outside the staging directory, and every step after it undone
//! when a later one fails.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_kuang_protocol::{
    DeltaEntry, PermissionDescriptor, PermissionPhase, PermissionRisk, PermissionSet, PluginRef,
    RECOMMENDED, RuntimeKind, delta,
};
use ono_value::{ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::kuang_acquire::Preference;
use crate::kuang_catalog::{Located, candidates_of};
use crate::kuang_host::{
    Host, Installed, Management, Resolved, action, action_result, capability_requests,
    declared_contributions, integrity_of, isolation_statement, map, network_of, signature_of,
};
use crate::kuang_permissions::{
    apply_profile, describe_descriptor, profile_title, publisher_identity, read_answer,
};
use crate::kuang_trust::Trust;
use crate::plugins::Produced;
use crate::session::Session;

/// What `install plugin` was told besides the reference.
#[derive(Debug, Default, Clone)]
pub struct InstallOptions {
    /// `--access <profile>`; `recommended` when absent.
    pub access: Option<String>,
    /// `--confirm`.
    pub confirm: bool,
    /// `--source system|catalog|local` (K11A §10.4); absent, the installed package's own
    /// lineage, then a system package, a local copy, the network.
    pub source: Option<String>,
}

/// The permission half of an install plan (K11P §12.1, §29.1).
#[derive(Debug, Clone)]
struct PermissionPlan {
    set: PermissionSet,
    profile: Option<ono_kuang_protocol::AccessProfile>,
    /// Decided `allow` by the profile, at install.
    granted: Vec<PermissionDescriptor>,
    /// Included without a question.
    automatic: Vec<PermissionDescriptor>,
    /// Asked at first concrete need.
    jit: Vec<PermissionDescriptor>,
    /// Left undecided: mutation among them.
    explicit: Vec<PermissionDescriptor>,
    /// For an upgrade, what the new version needs beyond what was consented to.
    delta: Option<Vec<DeltaEntry>>,
}

impl PermissionPlan {
    fn build(
        package: &Installed,
        profile_name: &str,
        previous: Option<&Installed>,
    ) -> Result<Self, ErrorValue> {
        let set = package.manifest.permission_set();
        let profile = if set.descriptors.is_empty() {
            None
        } else {
            Some(set.profile(profile_name).cloned().ok_or_else(|| {
                ErrorValue::new(
                    ErrorCode::KuangPermissionInvalidProfile,
                    format!(
                        "`{}` offers no access profile named `{profile_name}`",
                        package.manifest.package.name
                    ),
                )
                .with_help(format!(
                    "it offers {}",
                    set.profiles
                        .iter()
                        .map(|profile| format!("`{}`", profile.name))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?)
        };
        let selected: Vec<String> = profile
            .as_ref()
            .map(|profile| profile.permissions.clone())
            .unwrap_or_default();
        let mut granted = Vec::new();
        let mut automatic = Vec::new();
        let mut jit = Vec::new();
        let mut explicit = Vec::new();
        for descriptor in &set.descriptors {
            if descriptor.is_automatic() {
                automatic.push(descriptor.clone());
            } else if selected.contains(&descriptor.id) {
                granted.push(descriptor.clone());
            } else if descriptor.phase == PermissionPhase::Jit {
                jit.push(descriptor.clone());
            } else {
                explicit.push(descriptor.clone());
            }
        }
        let delta =
            previous.map(|previous| delta(&previous.manifest.permission_set(), &set, profile_name));
        Ok(Self {
            set,
            profile,
            granted,
            automatic,
            jit,
            explicit,
            delta,
        })
    }

    /// Whether the selected profile widens beyond safe defaults (an `operate` profile).
    fn widens(&self) -> bool {
        self.granted
            .iter()
            .any(|descriptor| !descriptor.is_safe_default())
    }

    /// Whether the selected profile carries a destructive permission (K11P §7.5).
    fn destructive(&self) -> bool {
        self.granted
            .iter()
            .any(|descriptor| descriptor.risk == PermissionRisk::Destructive)
    }
}

/// Runs `install plugin <reference> [--access <profile>] [--confirm]` (ADR-0602).
///
/// # Errors
///
/// The structured refusal of whichever step stopped: resolution, verification, an unconfirmed
/// plan, a plan this context may not accept, or the transaction's own failure.
pub fn install(session: &mut Session, reference: &str, options: &InstallOptions) -> Eval<Produced> {
    let started = std::time::Instant::now();
    session.publish_host();
    let parsed = PluginRef::parse(reference);

    // 0. Which sources may answer (K11A §10.3, §10.4, §14.2, §27): what `--source` named; else
    //    the lineage an installed package was acquired through, so a system package that
    //    appears later never silently replaces a catalog install and vice versa; else the
    //    default order. A root ordinary users can write to is set aside, and said.
    let mut preference = Preference::parse(options.source.as_deref()).map_err(Flow::Failed)?;
    if preference == Preference::Default && !parsed.is_explicit() {
        let lineage = session.with_kuang(|host| {
            host.resolve_installed(reference)
                .ok()
                .and_then(|package| host.management(&package.manifest.package.id).origin)
                .and_then(|origin| origin.source_kind())
        });
        if let Some(kind) = lineage {
            preference = Preference::Lineage(kind);
        }
    }
    for warning in
        session.with_kuang(|host| crate::kuang_catalog::rejected_roots(&host.system_scan()))
    {
        eprintln!("warning: {} {}", warning.code(), warning.message());
    }

    // 1. Resolve. An ambiguity is a picker interactively and a structured refusal otherwise
    //    (K11P §10.3).
    let located = match session.with_kuang(|host| host.locate_for_install(&parsed, preference)) {
        Ok(located) => located,
        Err(error)
            if error.code() == ErrorCode::PluginReferenceAmbiguous && session.is_interactive() =>
        {
            let candidates = candidates_of(&error);
            let Some(chosen) = pick(reference, &candidates) else {
                return Err(Flow::Failed(error));
            };
            session
                .with_kuang(|host| host.locate_for_install(&PluginRef::parse(&chosen), preference))
                .map_err(Flow::Failed)?
        }
        Err(error) => return Err(Flow::Failed(error)),
    };

    // 2. Verify. A blocking answer refuses before any plan is shown and never offers to
    //    continue (lifecycle.v1, ADR-0015 rule 4, Gate T).
    let resolved = Resolved {
        source: located.source.clone(),
        package: Ok(located.package.clone()),
    };
    let verification = session
        .with_kuang(|host| host.verify(&resolved))
        .map_err(Flow::Failed)?;
    if let Some(failure) = verification.blocking.into_iter().next() {
        return Err(Flow::Failed(failure));
    }
    let package = located.package.clone();
    let id = package.manifest.package.id.clone();
    let name = package.manifest.package.name.clone();
    let version = package.manifest.package.version.clone();

    // 3. What is already there: the same version refuses, another is an upgrade, and an
    //    upgrade keeps its lineage (K11P §21.1, §34.4).
    let (destination, previous, previous_management) = session
        .with_kuang(|host| {
            host.install_destination(&package).map(|destination| {
                let previous = host.installed_package(&id);
                let management = previous.as_ref().map(|_| host.management(&id));
                (destination, previous, management)
            })
        })
        .map_err(Flow::Failed)?;
    if previous
        .as_ref()
        .is_some_and(|previous| previous.manifest.package.version == version)
    {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::IoAlreadyExists,
                format!("`{id}` {version} is already installed"),
            )
            .with_help(
                "`remove plugin` it first; a package version is never silently replaced \
                 (spec §31.35)",
            ),
        ));
    }
    let new_key = publisher_identity(&package);
    if let Some(management) = &previous_management
        && let Some(old_key) = management.publisher_key.as_deref()
        && old_key != "unsigned"
        && new_key != "unsigned"
        && old_key != new_key
    {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::KuangPublisherUntrusted,
                format!(
                    "`{id}` is installed signed by {old_key}, and this package is signed by \
                     {new_key}; a package signed by an unrelated key is not an update"
                ),
            )
            .with_help(
                "a key rotation is a deliberate trust decision: enrol the new key, then \
                 `remove plugin` and install again, which asks for permission anew (K11P §34.4)",
            ),
        ));
    }

    // 4. The permission plan (K11P §12.1).
    let profile_name = options
        .access
        .clone()
        .or_else(|| {
            previous_management
                .as_ref()
                .and_then(|management| management.profile.clone())
        })
        .unwrap_or_else(|| RECOMMENDED.to_owned());
    let plan =
        PermissionPlan::build(&package, &profile_name, previous.as_ref()).map_err(Flow::Failed)?;
    let trust = trust_words(&package, session);
    let plan_value = plan_value(&package, &located, &destination, &plan, &trust, options);
    let unattended = unattended(&package, &located, &plan, &trust, options);

    // 5. Consent (K11P §13, §20; ADR-0602 §4).
    if session.is_interactive() {
        if !prompt(
            &package,
            &located,
            &plan,
            &trust,
            &plan_value,
            previous.is_some(),
        )? {
            return Err(Flow::Failed(ErrorValue::new(
                ErrorCode::SafetyConfirmationRequired,
                format!("installing `{name}` was not confirmed"),
            )));
        }
    } else if !options.confirm {
        // An update names what it asks for beyond what is held, so a script's refusal reads
        // the same delta a person would be shown (K11P §21.2, §34.5).
        let delta = plan
            .delta
            .as_ref()
            .filter(|entries| !entries.is_empty())
            .map(|entries| {
                format!(
                    "; {} {version} requests additional access: {}",
                    capitalise(&name),
                    entries
                        .iter()
                        .map(|entry| entry.detail.clone())
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            })
            .unwrap_or_default();
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::SafetyConfirmationRequired,
                format!(
                    "installing `{id}` {version} from {} needs the install plan confirmed{delta}",
                    located.source
                ),
            )
            .with_help(
                "nothing was written. Write `--confirm` to accept the plan non-interactively \
                 (spec §17.4, §31.9); `--access <profile>` selects the access it confirms",
            )
            .with_metadata("plan", plan_value.clone()),
        ));
    } else if let Err(reason) = unattended {
        return Err(Flow::Failed(
            reason.with_metadata("plan", plan_value.clone()),
        ));
    } else if trust.signature == "absent"
        && package
            .manifest
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.kind == RuntimeKind::NativeProcess)
    {
        // K11P §13.4: local development semantics, said rather than silent.
        eprintln!(
            "warning: `{name}` is an unsigned native plugin; it runs as your user account and \
             this execution tier is not complete filesystem or network isolation (K11P §27.1)"
        );
    } else if trust.signature == "valid"
        && !matches!(trust.standing, Trust::SystemTrusted | Trust::UserTrusted)
        && located.origin.source_kind() == Some(ono_kuang_protocol::SourceKind::SystemPackage)
    {
        // K11A §11.1: a system package is provenance, not publisher trust, and the difference is
        // said at the one moment it is decided.
        eprintln!(
            "warning: `{name}` is signed by a key no trust store enrols ({}); it came from a \
             system package, which is provenance and not publisher trust (K11A §11.1) — enrol \
             the key in `<config>/kuang/trust.yaml` to make it trusted",
            trust.key
        );
    }

    // 6. The transaction (K11P §12.4, Gate U).
    let interactive = session.is_interactive();
    let outcome = session.with_kuang(|host| {
        let action = action("install", &id, &version);
        match transact(
            host,
            &package,
            &located,
            &destination,
            previous.as_ref(),
            &plan,
            &profile_name,
        ) {
            Ok(()) => ono_provider_api::ActionOutcome::succeeded(&action, true),
            Err(error) => {
                host.record_host_event(&id, "plugin.install", "plugin.install", false);
                ono_provider_api::ActionOutcome::failed(&action, error)
            }
        }
    });
    let failed = !outcome.is_success();
    let failure = if failed {
        outcome.error().cloned()
    } else {
        None
    };
    if !failed {
        // 7. The contributions are placeholders now, so `install plugin x; get x-thing` is one
        //    session (ADR-0602 §2), and the package is ready without a `load` (Gate C).
        crate::plugin_registry::refresh();
        if interactive {
            match previous {
                Some(previous) => {
                    eprintln!(
                        "Updated {name} {} -> {version}",
                        previous.manifest.package.version
                    );
                    if plan.delta.as_ref().is_some_and(Vec::is_empty) {
                        eprintln!("Permissions unchanged.");
                    }
                }
                None => eprintln!("Installed {name} {version}"),
            }
            eprintln!("Ready to use.");
        }
    }
    let mut result = action_result(outcome, "ono.plugin.install", started);
    if !failed && let Value::Record(record) = &mut result {
        let _ = record;
    }
    Ok(Produced {
        values: vec![result],
        failure,
    })
}

/// The interactive picker of K11P §10.3: numbered candidates, one chosen by number, none by
/// order.
fn pick(reference: &str, candidates: &[crate::kuang_catalog::Candidate]) -> Option<String> {
    eprintln!("`{reference}` names {} packages:", candidates.len());
    for (index, candidate) in candidates.iter().enumerate() {
        eprintln!(
            "  [{}] {} — {} ({})",
            index + 1,
            candidate.id,
            candidate.description,
            candidate.selector
        );
    }
    let answer = read_answer("Which one? [number, or nothing to stop] ")?;
    let index: usize = answer.trim().parse().ok()?;
    candidates
        .get(index.checked_sub(1)?)
        .map(|candidate| candidate.selector.clone())
}

/// The four separate trust answers, as words (spec §31.36, K11P §18.1).
struct TrustWords {
    signature: String,
    publisher: String,
    key: String,
    standing: Trust,
}

fn trust_words(package: &Installed, session: &Session) -> TrustWords {
    let signature = signature_of(package);
    let standing =
        session.with_kuang(|host| crate::kuang_host::standing_of(&signature, host.trust()));
    // What a person is shown as *who signed*: a key where a key signed, and the identity a
    // certificate carried where nobody kept one (ADR-0609 §2 step 5).
    let key = match (&signature.document, &signature.identity) {
        _ if signature.failure.is_some() => "unsigned".to_owned(),
        (Some(document), _) => document.key().to_string(),
        (None, Some(identity)) => identity.subject.clone(),
        (None, None) => "unsigned".to_owned(),
    };
    TrustWords {
        signature: signature.state.to_owned(),
        publisher: package.manifest.package.publisher.clone(),
        key,
        standing,
    }
}

fn standing_word(standing: Trust) -> &'static str {
    match standing {
        Trust::SystemTrusted => "system-trusted",
        Trust::UserTrusted => "user-trusted",
        Trust::Untrusted => "untrusted",
        Trust::Unknown => "unknown",
    }
}

/// Whether `--confirm` may accept this plan without a person, and why not (ADR-0602 §4).
fn unattended(
    package: &Installed,
    located: &Located,
    plan: &PermissionPlan,
    trust: &TrustWords,
    options: &InstallOptions,
) -> Result<(), ErrorValue> {
    let name = &package.manifest.package.name;
    if plan.destructive() {
        return Err(ErrorValue::new(
            ErrorCode::KuangPermissionEscalationRequiresConfirmation,
            format!(
                "`{name}`'s `{}` profile carries a destructive permission, which is never \
                 enabled without a person (K11P §7.5)",
                plan.profile
                    .as_ref()
                    .map_or(RECOMMENDED, |profile| profile.name.as_str())
            ),
        )
        .with_help("install interactively, or with `--access recommended`"));
    }
    if plan.widens() && options.access.is_none() {
        return Err(ErrorValue::new(
            ErrorCode::KuangPermissionEscalationRequiresConfirmation,
            format!("`{name}`'s recorded profile widens authority beyond the recommended access"),
        )
        .with_help("name the profile deliberately with `--access <profile>` (K11P §20.2)"));
    }
    if let Some(entries) = &plan.delta
        && !entries.is_empty()
        && entries.iter().any(|entry| {
            plan.set
                .descriptor(&entry.permission)
                .is_some_and(|descriptor| !descriptor.is_safe_default())
        })
    {
        return Err(ErrorValue::new(
            ErrorCode::KuangPermissionEscalationRequiresConfirmation,
            format!("the update of `{name}` widens authority beyond safe defaults"),
        )
        .with_help("install interactively, or name the profile with `--access` (K11P §21.3)"));
    }
    let native = package
        .manifest
        .runtime
        .as_ref()
        .is_some_and(|runtime| runtime.kind == RuntimeKind::NativeProcess);
    // A signed native package whose key no store enrols is the first-trust decision of K11P
    // §13.3, and a flag does not make it. An unsigned package has no publisher identity to
    // enrol: it installs under the local-development semantics of §13.4, with the warning.
    // A payload a system package supplied is the operator's own provenance (K11A §2.5, §13):
    // it installs unattended like a local package, with its trust stated as what it is —
    // unknown until the key is enrolled — and never as more (§11.1).
    let from_system =
        located.origin.source_kind() == Some(ono_kuang_protocol::SourceKind::SystemPackage);
    if native
        && located.catalog.is_some()
        && !from_system
        && trust.signature == "valid"
        && !matches!(trust.standing, Trust::SystemTrusted | Trust::UserTrusted)
    {
        return Err(ErrorValue::new(
            ErrorCode::SafetyConfirmationRequired,
            format!(
                "`{name}` is a native plugin from a catalog and no trust store enrols its \
                 publisher key ({}); an unattended native install needs that policy",
                trust.key
            ),
        )
        .with_help(
            "enrol the key in `<config>/kuang/trust.yaml` or `/etc/ono/kuang/trust.yaml`, \
             or install interactively (K11P §13.3, ADR-0312)",
        ));
    }
    Ok(())
}

/// The compact prompt of K11P §13.2, with `details` one step away (K11P §5.3).
fn prompt(
    package: &Installed,
    located: &Located,
    plan: &PermissionPlan,
    trust: &TrustWords,
    plan_value: &Value,
    upgrade: bool,
) -> Eval<bool> {
    let manifest = &package.manifest;
    let native = manifest
        .runtime
        .as_ref()
        .is_some_and(|runtime| runtime.kind == RuntimeKind::NativeProcess);
    // A signed native package no trust store enrols is the first-trust decision of K11P §13.3;
    // an unsigned one installs under the local-development semantics of §13.4, warned but with
    // the ordinary default (ADR-0602 §4).
    let unknown_native = native
        && trust.signature == "valid"
        && !matches!(trust.standing, Trust::SystemTrusted | Trust::UserTrusted);
    loop {
        eprintln!();
        eprintln!(
            "{} {}",
            capitalise(&manifest.package.name),
            manifest.package.version
        );
        eprintln!("Publisher: {}", trust.publisher);
        eprintln!("Signature: {}", trust.signature);
        eprintln!(
            "Publisher trust: {}",
            match trust.standing {
                Trust::SystemTrusted => "system-trusted",
                Trust::UserTrusted => "user-trusted",
                Trust::Untrusted => "untrusted",
                Trust::Unknown if trust.signature == "absent" => "unknown (unsigned local package)",
                Trust::Unknown => "unknown",
            }
        );
        eprintln!(
            "Runtime: {}",
            match manifest.runtime.as_ref().map(|runtime| runtime.kind) {
                Some(RuntimeKind::NativeProcess) => "native process",
                Some(RuntimeKind::WasmComponent) => "isolated component",
                Some(RuntimeKind::RemoteService) => "remote service",
                _ => "declarative",
            }
        );
        if let Some((catalog, verification)) = &located.catalog {
            eprintln!("Catalog: {catalog} ({})", verification.id());
        }
        // Where the payload came from, as a fact beside the trust facts and never in place of
        // the permission plan (K11A §11.1, §12).
        eprintln!(
            "Source: {}{}",
            located
                .origin
                .source_kind()
                .map_or("unknown", ono_kuang_protocol::SourceKind::human),
            located
                .origin
                .system_package
                .as_deref()
                .map_or_else(String::new, |package| format!(" ({package})"))
        );
        if native {
            // Gate M: the isolation statement of K11P §18.2, on every native install.
            eprintln!("  {}", isolation_statement(manifest));
        }
        if unknown_native {
            eprintln!();
            eprintln!("Runtime warning");
            eprintln!("This is a native plugin. It runs as your user account.");
            eprintln!(
                "Ono can mediate brokered capabilities, but this execution tier does not prevent"
            );
            eprintln!("direct filesystem or network access available to your user.");
            eprintln!("No trust store enrols its publisher key ({}).", trust.key);
        } else if native && trust.signature == "absent" {
            eprintln!();
            eprintln!(
                "Warning: this package is unsigned; it installs with local-development semantics \
                 (K11P §13.4)."
            );
        }
        if let Some(entries) = plan.delta.as_ref().filter(|entries| !entries.is_empty()) {
            eprintln!();
            eprintln!(
                "{} {} requests additional access:",
                capitalise(&manifest.package.name),
                manifest.package.version
            );
            for entry in entries {
                eprintln!("  {}", entry.detail);
            }
            eprintln!();
            eprintln!("Existing access is unchanged.");
        } else {
            let profile_title = plan.profile.as_ref().map_or_else(
                || "No".to_owned(),
                |profile| profile_title(&plan.set, profile),
            );
            eprintln!();
            eprintln!("{profile_title} access:");
            for descriptor in plan.granted.iter().chain(&plan.automatic) {
                eprintln!("  - {}", line_of(descriptor));
            }
            if plan.granted.is_empty() && plan.automatic.is_empty() {
                eprintln!("  (nothing)");
            }
            if !plan.jit.is_empty() {
                eprintln!();
                eprintln!("Asked only when needed:");
                for descriptor in &plan.jit {
                    eprintln!("  - {}", descriptor.title);
                }
            }
            if !plan.explicit.is_empty() {
                eprintln!();
                eprintln!("Not granted:");
                for descriptor in &plan.explicit {
                    eprintln!("  - {}", descriptor.title);
                }
            }
        }
        eprintln!();
        let question = match (
            upgrade,
            plan.delta
                .as_ref()
                .is_some_and(|entries| !entries.is_empty()),
        ) {
            (true, true) => "Update and grant the new access? [y/N/details] ".to_owned(),
            (true, false) => "Update? [Y/n/details] ".to_owned(),
            _ if unknown_native || plan.widens() => format!(
                "Install with {} access? [y/N/details] ",
                plan.profile
                    .as_ref()
                    .map_or(RECOMMENDED, |profile| profile.name.as_str())
            ),
            _ => format!(
                "Install with {} access? [Y/n/details] ",
                plan.profile
                    .as_ref()
                    .map_or(RECOMMENDED, |profile| profile.name.as_str())
            ),
        };
        let default_yes = !question.contains("[y/N");
        let answer = read_answer(&question).unwrap_or_default();
        match answer.trim().to_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "" if default_yes => return Ok(true),
            "d" | "details" => {
                let rendered = ono_value::to_yaml_data(plan_value).unwrap_or_default();
                eprintln!("INSTALL PLAN\n{rendered}");
                for descriptor in &plan.set.descriptors {
                    eprintln!("{}", describe_descriptor(descriptor));
                }
            }
            _ => return Ok(false),
        }
    }
}

fn line_of(descriptor: &PermissionDescriptor) -> String {
    let scope: Vec<String> = descriptor
        .grants
        .iter()
        .filter_map(|template| match &template.scope {
            Some(ono_kuang_protocol::ScopeTemplate::Concrete(_)) => Some(
                ono_kuang_protocol::describe_scope(template.capability, template.scope.as_ref()),
            ),
            _ => None,
        })
        .collect();
    if scope.is_empty() {
        descriptor.title.clone()
    } else {
        format!("{} ({})", descriptor.title, scope.join("; "))
    }
}

/// The plan as one value (spec §31.9, lifecycle.v1 `install_plan`, K11P §29.1): what
/// `--confirm`'s refusal carries and `details` renders.
fn plan_value(
    package: &Installed,
    located: &Located,
    destination: &Path,
    plan: &PermissionPlan,
    trust: &TrustWords,
    options: &InstallOptions,
) -> Value {
    let manifest = &package.manifest;
    let signature = match trust.signature.as_str() {
        "valid" => format!("valid / {}", trust.publisher),
        "absent" => "unsigned".to_owned(),
        other => other.to_owned(),
    };
    let descriptor_values = |descriptors: &[PermissionDescriptor]| {
        Value::list(descriptors.iter().map(|descriptor| {
            map([
                ("id", Value::string(&descriptor.id)),
                ("title", Value::string(&descriptor.title)),
                ("kind", Value::string(descriptor.kind.id())),
                ("phase", Value::string(descriptor.phase.id())),
                ("risk", Value::string(descriptor.risk.id())),
                (
                    "capabilities",
                    Value::list(
                        descriptor
                            .capabilities()
                            .map(|capability| Value::string(capability.id())),
                    ),
                ),
                (
                    "scopes",
                    Value::list(descriptor.grants.iter().map(|template| {
                        map([
                            ("capability", Value::string(template.capability.id())),
                            (
                                "scope",
                                template.scope.as_ref().map_or(Value::Null, |scope| {
                                    Value::string(&scope.as_json().to_string())
                                }),
                            ),
                            (
                                "enforcement",
                                Value::string(match template.enforcement() {
                                    ono_kuang_protocol::Enforcement::Broker => "broker",
                                    ono_kuang_protocol::Enforcement::Advisory => "advisory",
                                }),
                            ),
                        ])
                    })),
                ),
                (
                    "decision",
                    Value::string(if descriptor.is_automatic() {
                        "included"
                    } else if plan
                        .granted
                        .iter()
                        .any(|granted| granted.id == descriptor.id)
                    {
                        "allow"
                    } else if descriptor.phase == PermissionPhase::Jit {
                        "ask when needed"
                    } else {
                        "not granted"
                    }),
                ),
                ("duration", Value::string("always")),
            ])
        }))
    };
    let unattended = unattended(package, located, plan, trust, options);
    map([
        (
            "package",
            Value::string(&format!(
                "{}@{}",
                manifest.package.id, manifest.package.version
            )),
        ),
        ("source", Value::string(&located.source)),
        ("source_kind", Value::string(&located.origin.kind)),
        (
            "system_package",
            located
                .origin
                .system_package
                .as_deref()
                .map_or(Value::Null, Value::string),
        ),
        (
            "acquisition",
            located.origin.value(&manifest.package.version),
        ),
        (
            "catalog",
            located
                .catalog
                .as_ref()
                .map_or(Value::Null, |(name, verification)| {
                    Value::string(&format!("{name} ({})", verification.id()))
                }),
        ),
        ("integrity", Value::string(&integrity_of(package))),
        ("signature", Value::string(&signature)),
        (
            "trust",
            map([
                ("signature", Value::string(&trust.signature)),
                ("publisher", Value::string(&trust.publisher)),
                ("key", Value::string(&trust.key)),
                ("standing", Value::string(standing_word(trust.standing))),
            ]),
        ),
        (
            "runtime_tier",
            Value::string(crate::kuang_host::execution_tier(manifest)),
        ),
        ("isolation", Value::string(isolation_statement(manifest))),
        ("contributions", declared_contributions(manifest)),
        ("capabilities", capability_requests(manifest, None)),
        (
            "selected_profile",
            Value::string(
                plan.profile
                    .as_ref()
                    .map_or("none", |profile| profile.name.as_str()),
            ),
        ),
        ("permissions", descriptor_values(&plan.granted)),
        ("automatic_privileges", descriptor_values(&plan.automatic)),
        ("asked_when_needed", descriptor_values(&plan.jit)),
        (
            "denied_explicit_permissions",
            descriptor_values(&plan.explicit),
        ),
        (
            "delta",
            plan.delta.as_ref().map_or(Value::Null, |entries| {
                Value::list(entries.iter().map(|entry| {
                    map([
                        ("permission", Value::string(&entry.permission)),
                        ("kind", Value::string(entry.kind.id())),
                        ("detail", Value::string(&entry.detail)),
                    ])
                }))
            }),
        ),
        (
            "unattended",
            map([
                ("accepted_by_confirm", Value::Bool(unattended.is_ok())),
                (
                    "reason",
                    unattended
                        .map_or_else(|error| Value::string(error.message()), |()| Value::Null),
                ),
            ]),
        ),
        (
            "filesystem",
            Value::list([Value::Path(Arc::from(destination))]),
        ),
        (
            "state",
            manifest.state.as_ref().map_or(Value::Null, |state| {
                map([
                    (
                        "persistence",
                        Value::string(&format!("{:?}", state.persistence).to_lowercase()),
                    ),
                    (
                        "quota",
                        state.quota.map_or(Value::Null, |quota| {
                            Value::ByteSize(ono_value::ByteSize::from_bytes(u128::from(quota)))
                        }),
                    ),
                ])
            }),
        ),
        ("network", network_of(manifest)),
    ])
}

/// What has been done so far, so a failure after the rename can be undone (K11P §12.4).
#[derive(Default)]
struct Undo {
    placed: Option<PathBuf>,
    previous: Option<(PathBuf, PathBuf)>,
    management: Option<Option<Management>>,
    grants: Vec<ono_value::Uuid>,
    decisions: Vec<String>,
}

/// The transaction proper: nothing outside the staging directory changes before the rename,
/// and everything after it is undone on failure (ADR-0602 §1).
fn transact(
    host: &mut Host,
    package: &Installed,
    located: &Located,
    destination: &Path,
    previous: Option<&Installed>,
    plan: &PermissionPlan,
    profile_name: &str,
) -> Result<(), ErrorValue> {
    let id = package.manifest.package.id.clone();
    let root = destination
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| ErrorValue::new(ErrorCode::IoNotFound, "no plugin home to install into"))?;
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        jiff::Timestamp::now().as_nanosecond()
    );
    let staging_root = root.join(".staging");
    std::fs::create_dir_all(&staging_root)
        .map_err(|error| crate::kuang_host::io_error(&staging_root, &error))?;
    let staging = staging_root.join(format!("{id}-{nonce}"));
    crate::kuang_host::copy_tree(&package.directory, &staging).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&staging);
    })?;

    let mut undo = Undo::default();
    let outcome = (|| -> Result<(), ErrorValue> {
        if destination.exists() {
            let aside = staging_root.join(format!("{id}.previous-{nonce}"));
            std::fs::rename(destination, &aside)
                .map_err(|error| crate::kuang_host::io_error(destination, &error))?;
            undo.previous = Some((aside, destination.to_path_buf()));
        }
        std::fs::rename(&staging, destination)
            .map_err(|error| crate::kuang_host::io_error(destination, &error))?;
        undo.placed = Some(destination.to_path_buf());
        let installed = Installed {
            directory: destination.to_path_buf(),
            manifest: package.manifest.clone(),
        };
        let before = previous.map(|_| host.management(&id));
        undo.management = Some(before.clone());
        let management = Management {
            enabled: true,
            installed_from: Some(located.source.clone()),
            integrity: Some(integrity_of(&installed)),
            background: before.as_ref().is_some_and(|before| before.background),
            catalog: located.catalog.as_ref().map(|(name, _)| name.clone()),
            publisher_key: Some(publisher_identity(&installed)),
            profile: plan.profile.as_ref().map(|profile| profile.name.clone()),
            origin: Some(located.origin.clone()),
        };
        host.write_management(&id, &management)?;
        // Decisions retained from an earlier install apply only to the same publisher
        // identity (K11P §22.3, §34.8); a mismatch is dropped, and said.
        let publisher = publisher_identity(&installed);
        let stale: Vec<String> = host
            .decisions_of(&id)
            .filter(|decision| decision.record.publisher != publisher)
            .map(|decision| decision.permission.clone())
            .collect();
        for permission in stale {
            eprintln!(
                "note: the earlier decision about `{permission}` was made for another publisher \
                 key and does not apply"
            );
            host.forget_decision(&id, &permission);
        }
        let correlation = format!("{id}:install:{nonce}");
        if let Some(profile) = &plan.profile {
            let minted =
                apply_profile(host, &installed, &plan.set, profile, "always", &correlation);
            undo.grants = minted.into_iter().map(|grant| grant.id).collect();
            undo.decisions = profile
                .permissions
                .iter()
                .cloned()
                .chain(
                    plan.automatic
                        .iter()
                        .map(|descriptor| descriptor.id.clone()),
                )
                .collect();
        }
        let _ = profile_name;
        host.persist_permissions()?;
        host.record_host_event_correlated(
            &id,
            "plugin.install",
            "plugin.install",
            true,
            Some(correlation),
        );
        Ok(())
    })();

    match outcome {
        Ok(()) => {
            if let Some((aside, _)) = undo.previous {
                let _ = std::fs::remove_dir_all(aside);
            }
            let _ = std::fs::remove_dir(&staging_root);
            Ok(())
        }
        Err(error) => {
            // Undo, in reverse: grants and decisions this transaction made, the management
            // state, the placed directory, the previous version (K11P §12.4, Gate U).
            for grant in undo.grants {
                host.revoke(grant);
            }
            for permission in undo.decisions {
                host.forget_decision(&id, &permission);
            }
            let _ = host.persist_permissions();
            match undo.management {
                Some(Some(before)) => {
                    let _ = host.write_management(&id, &before);
                }
                Some(None) => host.remove_management(&id),
                None => {}
            }
            if let Some(placed) = undo.placed {
                let _ = std::fs::remove_dir_all(&placed);
            } else {
                let _ = std::fs::remove_dir_all(&staging);
            }
            if let Some((aside, back)) = undo.previous {
                let _ = std::fs::rename(aside, back);
            }
            let _ = std::fs::remove_dir(&staging_root);
            Err(error)
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
