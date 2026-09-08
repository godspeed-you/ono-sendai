//! A keyless package signature, against a real bundle (ADR-0609).
//!
//! The fixture is not written to pass: it is the Sigstore bundle this project's own `v0.4.3`
//! release published over its `SHA256SUMS`, with a certificate Fulcio issued to the release
//! workflow, a real entry in the public transparency log, and a real signature. A verifier that
//! accepts a fixture somebody invented proves nothing about the one case that matters.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "AGENTS.md §16: a test states its preconditions directly"
)]

use ono_kuang_protocol::{KuangErrorCode, verify_keyless};

const BUNDLE: &str = include_str!("fixtures/release-SHA256SUMS.sigstore.json");
const PAYLOAD: &[u8] = include_bytes!("fixtures/release-SHA256SUMS");

const ISSUER: &str = "https://token.actions.githubusercontent.com";

#[test]
fn should_verify_the_bundle_this_project_published_and_name_who_signed_it() {
    let identity = verify_keyless(BUNDLE, PAYLOAD).expect("the release bundle verifies");
    assert_eq!(identity.issuer, ISSUER);
    assert!(
        identity.subject.starts_with(
            "https://github.com/godspeed-you/ono-sendai/.github/workflows/release.yml@"
        ),
        "the subject names the workflow that signed: {}",
        identity.subject
    );
    assert!(
        identity.subject.ends_with("refs/tags/v0.4.3"),
        "and the tag it ran on: {}",
        identity.subject
    );
    assert!(
        identity.signed_at > 1_700_000_000,
        "and the log's own time is carried out: {}",
        identity.signed_at
    );
}

#[test]
fn should_refuse_the_same_bundle_over_other_bytes() {
    // The first thing verified, before anything cryptographic: a bundle vouches for one thing.
    let mut tampered = PAYLOAD.to_vec();
    tampered[0] ^= 0x01;
    let refused = verify_keyless(BUNDLE, &tampered).expect_err("other bytes are refused");
    assert_eq!(refused.code(), KuangErrorCode::PackageSignatureInvalid);
    assert!(
        refused.message().contains("other bytes"),
        "and says so: {}",
        refused.message()
    );
}

#[test]
fn should_refuse_a_bundle_whose_signature_was_replaced() {
    // The signature is swapped for another well-formed one; the certificate and the log entry
    // stay as they are, so only step 3 can catch it.
    let other = "MEUCIQCh3Hi/CI4XngzTzGN0zQjS+CC/syVa+OZ9P+7TmWAYqAIgPxIohRaKhIz+AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    let bundle: serde_json::Value = serde_json::from_str(BUNDLE).expect("the fixture reads");
    let mut bundle = bundle;
    bundle["messageSignature"]["signature"] = serde_json::Value::String(other.to_owned());
    let refused = verify_keyless(&bundle.to_string(), PAYLOAD)
        .expect_err("a signature the certificate did not make is refused");
    assert_eq!(refused.code(), KuangErrorCode::PackageSignatureInvalid);
}

#[test]
fn should_refuse_a_bundle_whose_log_timestamp_was_replaced() {
    // Step 4: the log's own signature over its entry. Moving the recorded time would move the
    // certificate's ten-minute window, which is the whole reason the log is consulted.
    let mut bundle: serde_json::Value = serde_json::from_str(BUNDLE).expect("the fixture reads");
    bundle["verificationMaterial"]["tlogEntries"][0]["integratedTime"] =
        serde_json::Value::String("1788871999".to_owned());
    let refused = verify_keyless(&bundle.to_string(), PAYLOAD)
        .expect_err("an entry the log did not sign is refused");
    assert_eq!(refused.code(), KuangErrorCode::PackageSignatureInvalid);
}

#[test]
fn should_refuse_a_bundle_with_no_transparency_log_entry() {
    // Without the log nothing says *when* the signature was made, and a leaked ephemeral key
    // would be usable for ever.
    let mut bundle: serde_json::Value = serde_json::from_str(BUNDLE).expect("the fixture reads");
    bundle["verificationMaterial"]["tlogEntries"] = serde_json::Value::Array(Vec::new());
    let refused = verify_keyless(&bundle.to_string(), PAYLOAD)
        .expect_err("a bundle with no log entry is refused");
    assert!(
        refused.message().contains("transparency-log"),
        "and says what is missing: {}",
        refused.message()
    );
}
