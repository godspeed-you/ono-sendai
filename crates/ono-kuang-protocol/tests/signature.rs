//! What a package signature promises (spec §31.36): "did a key sign these bytes?".
//!
//! The questions here are the ones an operator asks of a downloaded package, and every one of
//! them is answered without running any of its code: does the signature belong to this package,
//! does it cover every file the package is made of, and was it made by the key it names.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use ono_kuang_protocol::{
    FileDigest, KuangErrorCode, Manifest, PackageSignature, PublicKey, SecretKey, SignedPackage,
};

const MANIFEST: &str = r#"
format: kuang-package/1
package:
  id: dev.example.users
  name: users
  version: 0.1.0
  description: Accounts from the name service.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64]
roles: [adapter]
network:
  outbound: none
"#;

/// A key whose bytes are fixed, so a test never depends on the machine's entropy.
fn key(seed: u8) -> SecretKey {
    SecretKey::from_bytes(&[seed; 32])
}

fn manifest() -> Manifest {
    Manifest::parse(MANIFEST).expect("the fixture manifest is valid")
}

fn files() -> Vec<FileDigest> {
    vec![
        FileDigest::of_bytes("manifest.yaml", MANIFEST.as_bytes()),
        FileDigest::of_bytes("adapters.yaml", b"adapters: []\n"),
    ]
}

fn signed(files: Vec<FileDigest>) -> SignedPackage {
    SignedPackage::new("dev.example.users", "0.1.0", "dev.example", files)
        .expect("the fixture package is describable")
}

#[test]
fn should_accept_a_signature_when_the_named_key_made_it_over_these_files() {
    let signature = key(1).sign(&signed(files()));
    signature
        .check(&manifest(), &files())
        .expect("the key that signed these files verifies them");
}

#[test]
fn should_refuse_a_signature_when_one_file_byte_changed() {
    let signature = key(1).sign(&signed(files()));
    let tampered = vec![
        FileDigest::of_bytes("manifest.yaml", MANIFEST.as_bytes()),
        FileDigest::of_bytes("adapters.yaml", b"adapters: [evil]\n"),
    ];
    let error = signature
        .check(&manifest(), &tampered)
        .expect_err("a changed file is not the file that was signed");
    assert_eq!(
        error.code(),
        KuangErrorCode::PackageSignatureInvalid,
        "spec §31.36: a signature that does not cover the bytes on disk is K11004, got {error}"
    );
    assert!(
        error.message().contains("adapters.yaml"),
        "the file that broke the signature is named, got {error}"
    );
}

#[test]
fn should_refuse_a_signature_when_the_package_carries_a_file_it_does_not_cover() {
    let signature = key(1).sign(&signed(vec![FileDigest::of_bytes(
        "manifest.yaml",
        MANIFEST.as_bytes(),
    )]));
    let error = signature
        .check(&manifest(), &files())
        .expect_err("an uncovered file is an unsigned file");
    assert_eq!(
        error.code(),
        KuangErrorCode::PackageSignatureInvalid,
        "a package file outside the signature is K11004, got {error}"
    );
}

#[test]
fn should_refuse_a_signature_naming_a_file_the_package_does_not_have() {
    let mut listed = files();
    listed.push(FileDigest::of_bytes("gone.yaml", b"gone\n"));
    let signature = key(1).sign(&signed(listed));
    let error = signature
        .check(&manifest(), &files())
        .expect_err("a signed file that is missing means the artifact is not the signed one");
    assert_eq!(error.code(), KuangErrorCode::PackageSignatureInvalid);
    assert!(
        error.message().contains("gone.yaml"),
        "the missing file is named, got {error}"
    );
}

#[test]
fn should_refuse_a_signature_made_by_another_key() {
    let honest = key(1).sign(&signed(files()));
    let forged = PackageSignature::new(
        key(2).public_key(),
        honest.signed().clone(),
        *honest.bytes(),
    );
    let error = forged
        .check(&manifest(), &files())
        .expect_err("the signature bytes do not belong to the key the document names");
    assert_eq!(error.code(), KuangErrorCode::PackageSignatureInvalid);
}

#[test]
fn should_refuse_a_signature_written_for_a_different_package() {
    let other = SignedPackage::new("dev.example.other", "0.1.0", "dev.example", files())
        .expect("a second package is describable");
    let signature = key(1).sign(&other);
    let error = signature
        .check(&manifest(), &files())
        .expect_err("a valid signature over another package proves nothing about this one");
    assert_eq!(error.code(), KuangErrorCode::PackageSignatureInvalid);
    assert!(
        error.message().contains("dev.example.other"),
        "the package the signature actually covers is named, got {error}"
    );
}

#[test]
fn should_refuse_a_signature_written_for_a_different_version() {
    let other = SignedPackage::new("dev.example.users", "0.2.0", "dev.example", files())
        .expect("a second version is describable");
    let error = key(1)
        .sign(&other)
        .check(&manifest(), &files())
        .expect_err("a signature over another version is not a signature over this one");
    assert_eq!(error.code(), KuangErrorCode::PackageSignatureInvalid);
}

#[test]
fn should_read_back_a_signature_document_it_wrote() {
    let signature = key(1).sign(&signed(files()));
    let text = signature.to_yaml();
    let read = PackageSignature::parse(&text).expect("the document it writes is one it reads");
    assert_eq!(read, signature, "the document round-trips without loss");
    read.check(&manifest(), &files())
        .expect("a round-tripped signature still verifies");
}

#[test]
fn should_report_a_signature_document_that_is_not_a_document_as_k11004() {
    for text in [
        "",
        "not: a signature",
        "format: kuang-signature/1\n",
        "\u{0}\u{1}\u{2}",
        "format: kuang-signature/2\nalgorithm: ed25519\n",
    ] {
        let error = PackageSignature::parse(text)
            .expect_err("a document that is not a signature cannot be treated as one");
        assert_eq!(
            error.code(),
            KuangErrorCode::PackageSignatureInvalid,
            "an unreadable signature document is K11004, not a panic, for {text:?}"
        );
    }
}

#[test]
fn should_report_an_algorithm_this_build_cannot_check_as_k11004() {
    let text = key(1).sign(&signed(files())).to_yaml();
    let swapped = text.replace("algorithm: ed25519", "algorithm: rsa-1024");
    let error = PackageSignature::parse(&swapped)
        .expect_err("an algorithm the build does not implement is not silently accepted");
    assert_eq!(error.code(), KuangErrorCode::PackageSignatureInvalid);
    assert!(
        error.message().contains("rsa-1024"),
        "the algorithm that was asked for is named, got {error}"
    );
}

#[test]
fn should_reject_a_key_or_signature_whose_hex_is_the_wrong_length() {
    assert!(
        PublicKey::parse("ed25519:00").is_err(),
        "32 bytes or nothing"
    );
    assert!(
        PublicKey::parse("ed25519:zz").is_err(),
        "hex digits or nothing"
    );
    assert!(
        PublicKey::parse(&"00".repeat(32)).is_err(),
        "the algorithm prefix is part of the key's spelling"
    );
    let text = key(1).sign(&signed(files())).to_yaml();
    let truncated = PackageSignature::parse(&text.replace("signature: ", "signature: 00"));
    assert!(
        truncated.is_err(),
        "a signature of the wrong length is refused before it is checked"
    );
}

#[test]
fn should_describe_the_same_package_the_same_way_whatever_order_its_files_arrive_in() {
    let forwards = signed(files());
    let mut backwards_files = files();
    backwards_files.reverse();
    let backwards = signed(backwards_files);
    assert_eq!(
        forwards.canonical_bytes(),
        backwards.canonical_bytes(),
        "spec §31.36: what is signed is the package, not the order a walker found it in"
    );
}

#[test]
fn should_refuse_to_describe_a_package_whose_file_list_is_ambiguous() {
    let duplicated = vec![
        FileDigest::of_bytes("a.yaml", b"one"),
        FileDigest::of_bytes("a.yaml", b"two"),
    ];
    assert!(
        SignedPackage::new("dev.example.users", "0.1.0", "dev.example", duplicated).is_err(),
        "one path cannot carry two digests"
    );
    let newline = vec![FileDigest::of_bytes("a\nb.yaml", b"one")];
    assert!(
        SignedPackage::new("dev.example.users", "0.1.0", "dev.example", newline).is_err(),
        "a path that could forge a line of the canonical form is refused"
    );
}

#[test]
fn should_render_a_public_key_the_way_it_reads_one() {
    let key = key(1).public_key();
    let rendered = key.to_string();
    assert!(
        rendered.starts_with("ed25519:"),
        "spec §31.36 prints `key ed25519:AB12...`, got {rendered}"
    );
    assert_eq!(
        PublicKey::parse(&rendered).expect("what it prints it reads"),
        key
    );
}
