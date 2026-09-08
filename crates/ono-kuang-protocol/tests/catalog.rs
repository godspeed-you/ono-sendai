//! Catalog documents under the acquisition addendum (K11A §6; ADR-0606, ADR-0607): what a
//! release may name as its artifact, and what the sidecar beside a system payload says.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md §16)"
)]

use ono_kuang_protocol::{
    Artifact, Catalog, CatalogVerification, KuangErrorCode, SourceKind, SystemOrigin,
};

fn document(insecure: bool, artifact: &str, digest: Option<&str>) -> String {
    let digest = digest.map_or(String::new(), |digest| {
        format!("          digest: \"{digest}\"\n")
    });
    format!(
        "format: kuang-catalog/1\ncatalog:\n  name: test\n  insecure_http: {insecure}\nentries:\n  - id: dev.example.echo\n    name: echo\n    publisher: dev.example\n    releases:\n      - version: 0.1.0\n        platforms: [linux-amd64]\n        kuang_api: \">=11.1 <12\"\n        artifact: \"{artifact}\"\n{digest}"
    )
    .replace("\n          digest", "\n        digest")
}

const DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

#[test]
fn should_read_a_network_artifact_only_with_the_digest_it_is_verified_against() {
    // K11A §6.1, §6.2: a network artifact is checked against the catalog's digest before
    // anything else is read from it, so a release without one cannot be fetched at all.
    let with = Catalog::parse(
        &document(false, "https://example.invalid/echo.kuang", Some(DIGEST)),
        CatalogVerification::Operator,
    )
    .expect("a network artifact with a digest reads");
    assert_eq!(
        with.entries[0].releases[0].artifact_kind(),
        Artifact::Network("https://example.invalid/echo.kuang".to_owned())
    );
    let without = Catalog::parse(
        &document(false, "https://example.invalid/echo.kuang", None),
        CatalogVerification::Operator,
    )
    .expect_err("a network artifact without a digest is refused");
    assert_eq!(without.code(), KuangErrorCode::PackageInvalid);
    assert!(without.message().contains("digest"), "{without:?}");
}

#[test]
fn should_allow_plain_http_only_in_an_operator_catalog_that_says_so() {
    // K11A §6.3: plain HTTP is never the default, and the built-in catalog cannot enable it.
    let refused = Catalog::parse(
        &document(false, "http://127.0.0.1:1/echo.kuang", Some(DIGEST)),
        CatalogVerification::Operator,
    )
    .expect_err("plain http without the declaration is refused");
    assert!(refused.message().contains("insecure_http"), "{refused:?}");
    let allowed = Catalog::parse(
        &document(true, "http://127.0.0.1:1/echo.kuang", Some(DIGEST)),
        CatalogVerification::Operator,
    )
    .expect("an operator catalog may allow it");
    assert!(allowed.insecure_http);
    let built_in = Catalog::parse(
        &document(true, "http://127.0.0.1:1/echo.kuang", Some(DIGEST)),
        CatalogVerification::BuiltIn,
    )
    .expect_err("the built-in catalog never allows it");
    assert!(built_in.message().contains("operator"), "{built_in:?}");
}

#[test]
fn should_read_the_other_artifact_forms_as_local_references() {
    let path = Catalog::parse(
        &document(false, "path:/srv/packages/echo", None),
        CatalogVerification::Operator,
    )
    .expect("a path artifact reads");
    assert!(matches!(
        path.entries[0].releases[0].artifact_kind(),
        Artifact::Path(_)
    ));
    let git = Catalog::parse(
        &document(false, "git:https://example.invalid/echo#v0.1.0", None),
        CatalogVerification::BuiltIn,
    )
    .expect("a git reference reads");
    assert!(matches!(
        git.entries[0].releases[0].artifact_kind(),
        Artifact::Reference(_)
    ));
}

#[test]
fn should_read_a_system_origin_sidecar_and_nothing_else_as_one() {
    // K11A §10.1: the sidecar names the outer package and the manager, as data.
    let origin = SystemOrigin::parse(
        "format: kuang-system-origin/1\npackage: ono-plugin-kubernetes\nmanager: apt/dpkg\n",
    )
    .expect("a sidecar reads");
    assert_eq!(origin.package, "ono-plugin-kubernetes");
    assert_eq!(origin.manager, "apt/dpkg");
    assert!(
        SystemOrigin::parse("format: kuang-system-origin/2\npackage: x\nmanager: y\n").is_err()
    );
    assert!(SystemOrigin::parse("package: x\n").is_err());
}

#[test]
fn should_name_the_three_source_kinds_by_their_words() {
    for kind in [
        SourceKind::CatalogNetwork,
        SourceKind::LocalPath,
        SourceKind::SystemPackage,
    ] {
        assert_eq!(SourceKind::from_id(kind.id()), Some(kind));
    }
    assert_eq!(SourceKind::SystemPackage.human(), "system package");
    assert_eq!(SourceKind::from_id("apt"), None);
}
