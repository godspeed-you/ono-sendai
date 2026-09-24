//! What travels inside a `.kuang` archive (spec §31.36; ADR-0311, ADR-0609).
//!
//! A signature is not one of the files it covers, so the walk that computes a package's digest
//! leaves it out. The archive must carry it anyway: a package that arrives without its signature
//! installs under local-development semantics rather than as the signed release it is, and it
//! does so silently. Both forms of signature are checked here because only one of them was
//! carried for the length of a release, and nothing said so.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "AGENTS.md §16: a test states its preconditions directly"
)]

use std::path::Path;
use std::process::Command;

const SIGN: &str = env!("CARGO_BIN_EXE_kuang-sign");

const MANIFEST: &str = "\
format: kuang-package/1
package:
  id: dev.example.packed
  name: packed
  version: 0.1.0
  description: A package that exists to be packed.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: \">=11.1 <12\"
  ono_language: \">=0.2\"
  platforms: [linux-amd64]
roles: [adapter]
network:
  outbound: none
";

/// A package directory carrying `manifest.yaml`, one contributed file, and each signature named.
fn package(directory: &Path, signatures: &[&str]) {
    std::fs::create_dir_all(directory.join("runtime")).expect("the package directory");
    std::fs::write(directory.join("manifest.yaml"), MANIFEST).expect("the manifest");
    std::fs::write(directory.join("runtime/plugin"), b"not a real program").expect("the runtime");
    for name in signatures {
        std::fs::write(directory.join(name), "{}\n").expect("the signature");
    }
}

/// The paths inside the archive `pack` writes for `directory`.
fn packed(directory: &Path, out: &Path) -> Vec<String> {
    let run = Command::new(SIGN)
        .args(["pack", &directory.display().to_string()])
        .arg("--out")
        .arg(out)
        .output()
        .expect("kuang-sign runs");
    assert!(
        run.status.success(),
        "pack: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let archive = std::fs::File::open(out).expect("the archive was written");
    tar::Archive::new(archive)
        .entries()
        .expect("the archive reads")
        .map(|entry| {
            entry
                .expect("an entry")
                .path()
                .expect("its path")
                .display()
                .to_string()
        })
        .collect()
}

#[test]
fn should_carry_every_signature_the_package_holds_into_the_archive() {
    let home = tempfile::tempdir().expect("a working directory");
    for signatures in [
        &["signature.yaml"][..],
        &["signature.sigstore.json"][..],
        &["signature.yaml", "signature.sigstore.json"][..],
    ] {
        let directory = home.path().join(signatures.join("-and-"));
        package(&directory, signatures);
        let names = packed(&directory, &home.path().join("out.kuang"));
        assert!(
            names.contains(&"manifest.yaml".to_owned())
                && names.contains(&"runtime/plugin".to_owned()),
            "the artifact travels: {names:?}"
        );
        for name in signatures {
            assert!(
                names.contains(&(*name).to_owned()),
                "a package signed as {name} arrives signed, not as an unsigned candidate: {names:?}"
            );
        }
    }
}

#[test]
fn should_carry_no_signature_for_a_package_that_was_never_signed() {
    // The other direction: `pack` adds a name because the file is there, never because the name
    // is known to it.
    let home = tempfile::tempdir().expect("a working directory");
    let directory = home.path().join("unsigned");
    package(&directory, &[]);
    let names = packed(&directory, &home.path().join("out.kuang"));
    assert_eq!(
        names,
        vec!["manifest.yaml".to_owned(), "runtime/plugin".to_owned()],
        "an unsigned package packs as exactly its artifact"
    );
}

#[test]
fn should_pack_a_sparse_file_as_the_regular_file_a_host_unpacks() {
    // A program copied with `cp` or produced by a linker can have holes. The host unpacks regular
    // files only (K11A §7), so the archive must carry the bytes rather than a sparse map.
    use std::io::{Seek, SeekFrom, Write};
    let home = tempfile::tempdir().expect("a working directory");
    let directory = home.path().join("sparse");
    package(&directory, &[]);
    let mut runtime = std::fs::File::create(directory.join("runtime/plugin")).expect("the runtime");
    runtime.write_all(b"head").expect("the head");
    runtime
        .seek(SeekFrom::Start(1 << 20))
        .expect("a hole of a mebibyte");
    runtime.write_all(b"tail").expect("the tail");
    drop(runtime);
    let expected = std::fs::read(directory.join("runtime/plugin")).expect("the runtime reads");

    let out = home.path().join("out.kuang");
    packed(&directory, &out);
    let mut archive = tar::Archive::new(std::fs::File::open(&out).expect("the archive"));
    for entry in archive.entries().expect("the archive reads") {
        let mut entry = entry.expect("an entry");
        let name = entry.path().expect("its path").display().to_string();
        assert_eq!(
            entry.header().entry_type(),
            tar::EntryType::Regular,
            "{name} is packed as a regular file, the only kind a host unpacks"
        );
        if name == "runtime/plugin" {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes).expect("the payload");
            assert_eq!(
                bytes, expected,
                "the payload travels byte for byte, holes included"
            );
        }
    }
}
