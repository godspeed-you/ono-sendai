//! Target freezing policy (spec v0.6 §7.1, §4.3, §2.6).
//!
//! [`ono_change_core::FrozenTarget`] is a schema, an identity and a label. What it does not say is
//! *what the identity must contain*, and §7.1 does:
//!
//! - for a file, "canonical path plus containing persistence domain and relevant inode/generation
//!   information where available";
//! - for a service, "provider namespace and unit identity";
//! - for a remote target, "host/link identity is part of the target".
//!
//! That is the policy this module owns, and it is a policy rather than a formatting choice because
//! the identity is what §4.4 seals and what §7.3 revalidates against. A file identity that omitted
//! the dataset would let a plan resolved against `/etc` on one subvolume be applied against `/etc`
//! on another, and neither the seal nor drift detection would notice.
//!
//! The selector is kept as provenance and is never re-run. §2.6 and §4.3 are explicit: "If four
//! services match at resolution time and a fifth fails before execution, the fifth service MUST
//! NOT be added silently." Nothing here holds a selector in a form that could be evaluated; it is
//! a string that `explain` prints.

use std::path::Path;

use ono_change_core::{FrozenTarget, error};
use ono_value::ErrorValue;

/// The object schema a frozen file target carries.
pub const FILE_SCHEMA: &str = "ono.file/1";

/// The object schema a frozen service target carries.
pub const SERVICE_SCHEMA: &str = "ono.service/1";

/// The separator between a path and the persistence domain that contains it.
const DOMAIN_MARK: char = '@';

/// The separator before the generation facts that distinguish two objects at one name.
const GENERATION_MARK: char = '#';

/// A file about to be frozen into a plan target (§7.1).
///
/// The path is required to be canonical, and "canonical" is checked rather than assumed: a
/// relative path or one carrying `.` or `..` has not been resolved, and freezing it would put a
/// selector into the plan under the name of an identity.
#[derive(Debug, Clone)]
pub struct FileTarget<'a> {
    path: &'a Path,
    domain: Option<&'a str>,
    inode: Option<u64>,
    generation: Option<&'a str>,
    host: Option<&'a str>,
    selector: Option<&'a str>,
    place: Option<&'a str>,
}

impl<'a> FileTarget<'a> {
    /// The file at `path`, which must already be canonical.
    #[must_use]
    pub const fn at(path: &'a Path) -> Self {
        Self {
            path,
            domain: None,
            inode: None,
            generation: None,
            host: None,
            selector: None,
            place: None,
        }
    }

    /// The persistence domain that actually contains the file's state (§7.1, Appendix B).
    #[must_use]
    pub const fn in_domain(mut self, domain: &'a str) -> Self {
        self.domain = Some(domain);
        self
    }

    /// The inode, where the filesystem offers one (§7.1's "where available").
    #[must_use]
    pub const fn with_inode(mut self, inode: u64) -> Self {
        self.inode = Some(inode);
        self
    }

    /// The generation counter, for a filesystem that exposes one instead of an inode.
    #[must_use]
    pub const fn with_generation(mut self, generation: &'a str) -> Self {
        self.generation = Some(generation);
        self
    }

    /// The host the file lives on, which makes it a remote target (§7.1, §29).
    #[must_use]
    pub const fn on_host(mut self, host: &'a str) -> Self {
        self.host = Some(host);
        self
    }

    /// The selector this file came from, kept as provenance and never re-run (§4.3, §2.6).
    #[must_use]
    pub const fn resolved_from(mut self, selector: &'a str) -> Self {
        self.selector = Some(selector);
        self
    }

    /// The v0.4 spatial identity of the same object, where it has one.
    #[must_use]
    pub const fn at_place(mut self, place: &'a str) -> Self {
        self.place = Some(place);
        self
    }

    /// Freezes the file into a plan target (§4.3).
    ///
    /// # Errors
    ///
    /// Returns `change.target_unresolved` for a path that is not canonical, because a plan stores
    /// stable object identities and an unresolved path is not one (§7.1).
    pub fn freeze(&self) -> Result<FrozenTarget, ErrorValue> {
        let path = canonical_text(self.path, self.selector)?;
        let mut identity = path.clone();
        if let Some(domain) = self.domain {
            identity.push(DOMAIN_MARK);
            identity.push_str(domain);
        }
        if let Some(inode) = self.inode {
            identity.push(GENERATION_MARK);
            identity.push_str("inode=");
            identity.push_str(&inode.to_string());
        }
        if let Some(generation) = self.generation {
            identity.push(GENERATION_MARK);
            identity.push_str("generation=");
            identity.push_str(generation);
        }
        let mut target = FrozenTarget::new(FILE_SCHEMA, identity, path);
        if let Some(domain) = self.domain {
            target = target.in_domain(domain);
        }
        Ok(decorate(target, self.host, self.selector, self.place))
    }
}

/// A service about to be frozen into a plan target (§7.1).
///
/// The provider namespace is part of the identity because two providers may both offer a unit
/// called `nginx.service` — a systemd unit and a container supervisor's — and a plan resolved
/// against one must not revalidate against the other.
#[derive(Debug, Clone)]
pub struct ServiceTarget<'a> {
    namespace: &'a str,
    unit: &'a str,
    generation: Option<&'a str>,
    host: Option<&'a str>,
    selector: Option<&'a str>,
    place: Option<&'a str>,
}

impl<'a> ServiceTarget<'a> {
    /// The unit `unit` as the provider namespace `namespace` names it.
    #[must_use]
    pub const fn new(namespace: &'a str, unit: &'a str) -> Self {
        Self {
            namespace,
            unit,
            generation: None,
            host: None,
            selector: None,
            place: None,
        }
    }

    /// The generation or invocation identity the provider exposes (§7.2's "service generation").
    #[must_use]
    pub const fn with_generation(mut self, generation: &'a str) -> Self {
        self.generation = Some(generation);
        self
    }

    /// The host the unit runs on (§7.1).
    #[must_use]
    pub const fn on_host(mut self, host: &'a str) -> Self {
        self.host = Some(host);
        self
    }

    /// The selector this unit came from, kept as provenance and never re-run (§4.3).
    #[must_use]
    pub const fn resolved_from(mut self, selector: &'a str) -> Self {
        self.selector = Some(selector);
        self
    }

    /// The v0.4 spatial identity of the same object, where it has one.
    #[must_use]
    pub const fn at_place(mut self, place: &'a str) -> Self {
        self.place = Some(place);
        self
    }

    /// Freezes the unit into a plan target (§4.3).
    ///
    /// # Errors
    ///
    /// Returns `change.target_unresolved` where the namespace or the unit is empty: a target
    /// without a provider namespace is not the identity §7.1 asks for.
    pub fn freeze(&self) -> Result<FrozenTarget, ErrorValue> {
        if self.namespace.trim().is_empty() || self.unit.trim().is_empty() {
            return Err(error::target_unresolved(
                self.selector.unwrap_or(self.unit),
                "§7.1: a service identity is a provider namespace and a unit identity, and one of \
                 them is missing.",
            ));
        }
        let mut identity = format!("{}:{}", self.namespace, self.unit);
        if let Some(generation) = self.generation {
            identity.push(GENERATION_MARK);
            identity.push_str("generation=");
            identity.push_str(generation);
        }
        let target = FrozenTarget::new(SERVICE_SCHEMA, identity, self.unit);
        Ok(decorate(target, self.host, self.selector, self.place))
    }
}

/// Any other object on another host, frozen with the link identity §7.1 requires.
#[derive(Debug, Clone)]
pub struct RemoteTarget<'a> {
    schema: &'a str,
    identity: &'a str,
    label: &'a str,
    host: &'a str,
    link: Option<&'a str>,
    selector: Option<&'a str>,
    place: Option<&'a str>,
}

impl<'a> RemoteTarget<'a> {
    /// The object `identity` of type `schema` on `host`.
    #[must_use]
    pub const fn new(schema: &'a str, identity: &'a str, host: &'a str) -> Self {
        Self {
            schema,
            identity,
            label: identity,
            host,
            link: None,
            selector: None,
            place: None,
        }
    }

    /// The label a person reads, where it differs from the identity.
    #[must_use]
    pub const fn labelled(mut self, label: &'a str) -> Self {
        self.label = label;
        self
    }

    /// The link the host was reached over, where the transport has an identity of its own (§29.1).
    #[must_use]
    pub const fn over_link(mut self, link: &'a str) -> Self {
        self.link = Some(link);
        self
    }

    /// The selector this object came from, kept as provenance and never re-run (§4.3).
    #[must_use]
    pub const fn resolved_from(mut self, selector: &'a str) -> Self {
        self.selector = Some(selector);
        self
    }

    /// The v0.4 spatial identity of the same object, where it has one.
    #[must_use]
    pub const fn at_place(mut self, place: &'a str) -> Self {
        self.place = Some(place);
        self
    }

    /// Freezes the remote object into a plan target (§4.3, §7.1).
    ///
    /// # Errors
    ///
    /// Returns `change.target_unresolved` where the host is empty. §7.1 makes host identity part
    /// of a remote target, so a remote target without one is not resolved.
    pub fn freeze(&self) -> Result<FrozenTarget, ErrorValue> {
        if self.host.trim().is_empty() {
            return Err(error::target_unresolved(
                self.selector.unwrap_or(self.identity),
                "§7.1: host or link identity is part of a remote target, and no host was given.",
            ));
        }
        let identity = match self.link {
            Some(link) => format!("{}{DOMAIN_MARK}{}/{link}", self.identity, self.host),
            None => format!("{}{DOMAIN_MARK}{}", self.identity, self.host),
        };
        let target = FrozenTarget::new(self.schema, identity, self.label);
        Ok(decorate(target, Some(self.host), self.selector, self.place))
    }
}

/// Applies the three optional facts every kind of target carries in the same way.
fn decorate(
    target: FrozenTarget,
    host: Option<&str>,
    selector: Option<&str>,
    place: Option<&str>,
) -> FrozenTarget {
    let mut target = target;
    if let Some(host) = host {
        target = target.on_host(host);
    }
    if let Some(selector) = selector {
        target = target.resolved_from(selector);
    }
    if let Some(place) = place {
        target = target.at_place(place);
    }
    target
}

/// The path as text, refusing anything that has not been canonicalised (§7.1).
fn canonical_text(path: &Path, selector: Option<&str>) -> Result<String, ErrorValue> {
    let shown = path.display().to_string();
    let reference = selector.unwrap_or(&shown);
    let Some(text) = path.to_str() else {
        return Err(error::target_unresolved(
            reference,
            "§7.1 stores a canonical path, and this one is not valid UTF-8, so it cannot be \
             written to a plan record.",
        ));
    };
    if !path.is_absolute() {
        return Err(error::target_unresolved(
            reference,
            "§7.1 stores a canonical path, and a relative path depends on where the shell \
             happened to be standing.",
        ));
    }
    // `Path::components` folds `.` away, so the check is over the text the caller actually wrote:
    // a path carrying `.` or `..` has not been resolved, whatever it would normalise to.
    if text
        .split('/')
        .any(|segment| segment == "." || segment == "..")
    {
        return Err(error::target_unresolved(
            reference,
            "§7.1 stores a canonical path, and `.` or `..` in one means it has not been resolved \
             yet.",
        ));
    }
    Ok(text.to_owned())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use std::path::PathBuf;

    use ono_core::ErrorCode;

    use super::*;

    fn path(text: &str) -> PathBuf {
        PathBuf::from(text)
    }

    #[test]
    fn should_make_the_same_path_in_two_persistence_domains_two_targets() {
        let here = path("/etc/nginx/nginx.conf");
        let rpool = FileTarget::at(&here)
            .in_domain("rpool/etc")
            .freeze()
            .expect("a canonical path freezes");
        let tank = FileTarget::at(&here)
            .in_domain("tank/etc")
            .freeze()
            .expect("a canonical path freezes");
        assert_ne!(
            rpool.identity(),
            tank.identity(),
            "§7.1: a file identity includes the containing persistence domain"
        );
    }

    #[test]
    fn should_make_the_same_path_at_two_inodes_two_targets() {
        let here = path("/etc/nginx/nginx.conf");
        let first = FileTarget::at(&here)
            .in_domain("rpool/etc")
            .with_inode(41)
            .freeze()
            .expect("a canonical path freezes");
        let replaced = FileTarget::at(&here)
            .in_domain("rpool/etc")
            .with_inode(42)
            .freeze()
            .expect("a canonical path freezes");
        assert_ne!(
            first.digest_text(),
            replaced.digest_text(),
            "§7.1: relevant inode information is part of a file identity, so a replaced file is a \
             different object"
        );
    }

    #[test]
    fn should_record_the_persistence_domain_beside_the_identity() {
        let here = path("/etc/nginx/nginx.conf");
        let target = FileTarget::at(&here)
            .in_domain("rpool/etc")
            .freeze()
            .expect("a canonical path freezes");
        assert_eq!(
            target.persistence_domain(),
            Some("rpool/etc"),
            "Appendix B: the domain a target's state lives in travels with the target"
        );
        assert_eq!(target.label(), "/etc/nginx/nginx.conf");
    }

    #[test]
    fn should_refuse_a_relative_path_rather_than_freeze_a_selector() {
        let relative = path("etc/nginx/nginx.conf");
        let refusal = FileTarget::at(&relative)
            .freeze()
            .expect_err("§7.1 requires a canonical path");
        assert_eq!(
            refusal.code(),
            ErrorCode::ChangeTargetUnresolved,
            "§7.1: a plan stores stable object identities, not unresolved selectors"
        );
    }

    #[test]
    fn should_refuse_a_path_that_still_carries_a_parent_component() {
        let unresolved = path("/etc/nginx/../nginx/nginx.conf");
        let refusal = FileTarget::at(&unresolved)
            .freeze()
            .expect_err("§7.1 requires a canonical path");
        assert_eq!(refusal.code(), ErrorCode::ChangeTargetUnresolved);
    }

    #[test]
    fn should_refuse_a_path_that_still_carries_a_current_directory_component() {
        let unresolved = path("/etc/./nginx.conf");
        let refusal = FileTarget::at(&unresolved)
            .freeze()
            .expect_err("§7.1 requires a canonical path");
        assert_eq!(refusal.code(), ErrorCode::ChangeTargetUnresolved);
    }

    #[test]
    fn should_make_one_unit_name_in_two_provider_namespaces_two_targets() {
        let systemd = ServiceTarget::new("systemd", "nginx.service")
            .freeze()
            .expect("a namespaced unit freezes");
        let container = ServiceTarget::new("podman", "nginx.service")
            .freeze()
            .expect("a namespaced unit freezes");
        assert_ne!(
            systemd.identity(),
            container.identity(),
            "§7.1: a service identity includes the provider namespace"
        );
    }

    #[test]
    fn should_carry_the_service_generation_into_the_identity() {
        let before = ServiceTarget::new("systemd", "nginx.service")
            .with_generation("inv-1")
            .freeze()
            .expect("a namespaced unit freezes");
        let after = ServiceTarget::new("systemd", "nginx.service")
            .with_generation("inv-2")
            .freeze()
            .expect("a namespaced unit freezes");
        assert_ne!(
            before.digest_text(),
            after.digest_text(),
            "§7.2: a service generation is what tells a restarted unit from the one that was frozen"
        );
    }

    #[test]
    fn should_refuse_a_service_without_a_provider_namespace() {
        let refusal = ServiceTarget::new("", "nginx.service")
            .freeze()
            .expect_err("§7.1 requires a provider namespace");
        assert_eq!(refusal.code(), ErrorCode::ChangeTargetUnresolved);
    }

    #[test]
    fn should_make_one_object_on_two_hosts_two_targets() {
        let here = RemoteTarget::new(SERVICE_SCHEMA, "nginx.service", "api-04")
            .freeze()
            .expect("a host freezes");
        let there = RemoteTarget::new(SERVICE_SCHEMA, "nginx.service", "api-05")
            .freeze()
            .expect("a host freezes");
        assert_ne!(
            here.identity(),
            there.identity(),
            "§7.1: host identity is part of a remote target"
        );
        assert_eq!(here.host(), Some("api-04"));
    }

    #[test]
    fn should_make_two_links_to_one_host_two_targets() {
        let direct = RemoteTarget::new(SERVICE_SCHEMA, "nginx.service", "api-04")
            .over_link("ssh")
            .freeze()
            .expect("a host freezes");
        let brokered = RemoteTarget::new(SERVICE_SCHEMA, "nginx.service", "api-04")
            .over_link("kuang")
            .freeze()
            .expect("a host freezes");
        assert_ne!(
            direct.identity(),
            brokered.identity(),
            "§7.1: link identity is part of a remote target where the transport has one"
        );
    }

    #[test]
    fn should_refuse_a_remote_target_without_a_host() {
        let refusal = RemoteTarget::new(SERVICE_SCHEMA, "nginx.service", "  ")
            .freeze()
            .expect_err("§7.1 requires host identity");
        assert_eq!(refusal.code(), ErrorCode::ChangeTargetUnresolved);
    }

    #[test]
    fn should_keep_the_selector_as_provenance_and_out_of_the_seal() {
        let target = ServiceTarget::new("systemd", "nginx.service")
            .resolved_from("get service | where state == failed")
            .freeze()
            .expect("a namespaced unit freezes");
        assert_eq!(
            target.selector(),
            Some("get service | where state == failed"),
            "§4.3 keeps the selector so `explain` can show where the frozen set came from"
        );
        assert!(
            !target.digest_text().contains("where"),
            "§2.6: the selector is provenance and is never re-run, so it is not part of identity"
        );
    }

    #[test]
    fn should_carry_the_spatial_identity_of_the_same_object() {
        let target = ServiceTarget::new("systemd", "nginx.service")
            .at_place("place/9f21")
            .freeze()
            .expect("a namespaced unit freezes");
        assert_eq!(target.spatial_id(), Some("place/9f21"));
    }
}
