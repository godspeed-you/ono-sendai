//! The example package ships signed, and the signature is checked here so it cannot rot.
//!
//! Spec §31.36 asks a package author to sign; the SDK's example package is what an author reads
//! to see how, so it carries a real signature made with the demo key beside it. An edit to the
//! example without a re-signing turns this red rather than shipping a package whose signature
//! says the wrong thing.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::path::PathBuf;

use ono_kuang_protocol::{Manifest, PackageSignature, SIGNATURE_FILE, SecretKey, artifact_files};

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples")
}

fn package() -> PathBuf {
    examples().join("adapter-package/dev.example.users")
}

const RESIGN: &str = "re-sign it with `cargo run -p ono-kuang-sdk --bin kuang-sign -- sign \
                      crates/ono-kuang-sdk/examples/adapter-package/dev.example.users --key \
                      crates/ono-kuang-sdk/examples/keys/dev.example.key`";

#[test]
fn should_ship_a_signature_that_covers_the_example_package_as_it_is() {
    let directory = package();
    let manifest = Manifest::parse(
        &std::fs::read_to_string(directory.join("manifest.yaml")).expect("the example manifest"),
    )
    .expect("the example manifest is valid");
    let text = std::fs::read_to_string(directory.join(SIGNATURE_FILE))
        .unwrap_or_else(|error| panic!("the example package ships signed: {error}; {RESIGN}"));
    let signature = PackageSignature::parse(&text).expect("the shipped signature is a document");
    signature
        .check(&manifest, &artifact_files(&directory))
        .unwrap_or_else(|error| panic!("{error}\nthe example package changed; {RESIGN}"));
}

#[test]
fn should_ship_the_key_the_example_package_was_signed_with() {
    let text = std::fs::read_to_string(examples().join("keys/dev.example.key"))
        .expect("the demo signing key ships beside the example package");
    let key = SecretKey::parse(&text).expect("the demo key is a signing key");
    let signature = PackageSignature::parse(
        &std::fs::read_to_string(package().join(SIGNATURE_FILE)).expect("the shipped signature"),
    )
    .expect("the shipped signature is a document");
    assert_eq!(
        signature.key(),
        &key.public_key(),
        "the shipped signature was made by the key that ships beside it, so a reader can \
         reproduce it; {RESIGN}"
    );
}
