//! `kuang-sign`: what a package author runs to sign a KUANG/11 package (spec §31.36, ADR-0311).
//!
//! ```text
//! kuang-sign keygen --out <file>     write a signing key, print its public half
//! kuang-sign sign <directory> --key <file>
//! kuang-sign verify <directory>
//! ```
//!
//! Signing is the author's side of the check `verify plugin` performs, and this is the tool the
//! SDK ships so that side exists. It writes `signature.yaml` beside the manifest, covering every
//! file the package is made of.

#![allow(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "a command-line tool speaks on stdout and stderr"
)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ono_kuang_protocol::{
    KuangError, Manifest, PackageSignature, SIGNATURE_FILE, SecretKey, SignedPackage,
    artifact_files,
};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let outcome = match words.split_first() {
        Some((&"keygen", rest)) => keygen(rest),
        Some((&"sign", rest)) => sign(rest),
        Some((&"verify", rest)) => verify(rest),
        _ => {
            eprintln!(
                "kuang-sign: one of\n  \
                 kuang-sign keygen --out <file>\n  \
                 kuang-sign sign <directory> --key <file>\n  \
                 kuang-sign verify <directory>"
            );
            return ExitCode::from(2);
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kuang-sign: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The value of `--<name>`, if it is there.
fn option(words: &[&str], name: &str) -> Option<PathBuf> {
    words
        .iter()
        .position(|word| *word == name)
        .and_then(|at| words.get(at + 1))
        .map(PathBuf::from)
}

/// The first word that is not an option or an option's value.
fn positional(words: &[&str]) -> Option<PathBuf> {
    let mut iter = words.iter();
    while let Some(word) = iter.next() {
        if word.starts_with("--") {
            let _ = iter.next();
        } else {
            return Some(PathBuf::from(word));
        }
    }
    None
}

fn keygen(words: &[&str]) -> Result<(), KuangError> {
    let key = SecretKey::generate()?;
    let public = key.public_key();
    match option(words, "--out") {
        Some(path) => {
            write_private(&path, &key.to_secret_string())?;
            eprintln!("kuang-sign: wrote the signing key to {}", path.display());
        }
        None => println!("{}", key.to_secret_string()),
    }
    // The public half on stdout: it is what goes into a trust store.
    println!("{public}");
    Ok(())
}

fn sign(words: &[&str]) -> Result<(), KuangError> {
    let directory = positional(words).ok_or_else(|| failed("`sign` needs a package directory"))?;
    let key_path = option(words, "--key")
        .ok_or_else(|| failed("`sign` needs `--key <file>`, the signing key to use"))?;
    let key = SecretKey::parse(&read(&key_path)?)?;
    let manifest = manifest_of(&directory)?;
    let files = artifact_files(&directory);
    let described = SignedPackage::new(
        &manifest.package.id,
        &manifest.package.version,
        &manifest.package.publisher,
        files,
    )?;
    let signature = key.sign(&described);
    let path = directory.join(SIGNATURE_FILE);
    std::fs::write(&path, signature.to_yaml())
        .map_err(|error| failed(format!("cannot write {}: {error}", path.display())))?;
    println!(
        "{} {} signed by {} over {} files",
        manifest.package.id,
        manifest.package.version,
        signature.key(),
        described.files.len()
    );
    Ok(())
}

fn verify(words: &[&str]) -> Result<(), KuangError> {
    let directory =
        positional(words).ok_or_else(|| failed("`verify` needs a package directory"))?;
    let manifest = manifest_of(&directory)?;
    let text = read(&directory.join(SIGNATURE_FILE))?;
    let signature = PackageSignature::parse(&text)?;
    signature.check(&manifest, &artifact_files(&directory))?;
    println!(
        "{} {} verifies under {}",
        manifest.package.id,
        manifest.package.version,
        signature.key()
    );
    Ok(())
}

fn manifest_of(directory: &Path) -> Result<Manifest, KuangError> {
    Manifest::parse(&read(&directory.join("manifest.yaml"))?)
}

fn read(path: &Path) -> Result<String, KuangError> {
    std::fs::read_to_string(path)
        .map_err(|error| failed(format!("cannot read {}: {error}", path.display())))
}

/// Writes a file only its owner can read: a signing key is a secret.
fn write_private(path: &Path, text: &str) -> Result<(), KuangError> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|error| failed(format!("cannot write {}: {error}", path.display())))
}

fn failed(message: impl Into<String>) -> KuangError {
    KuangError::new(
        ono_kuang_protocol::KuangErrorCode::PackageInvalid,
        message.into(),
    )
}
