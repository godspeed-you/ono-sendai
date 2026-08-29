//! Package signatures: "did a key sign these bytes?" (spec §31.36).
//!
//! Spec §31.36 keeps four questions apart, and this module answers exactly one of them. The
//! content hash answers whether the bytes are the ones referenced; publisher trust answers
//! whether the operator accepts the key; runtime isolation answers what the code can do anyway.
//! What is here is only the middle question — a detached Ed25519 signature over a canonical
//! description of the package, and the rules that decide whether it belongs to the artifact on
//! disk (ADR-0311).
//!
//! Nothing here touches the filesystem. The caller walks the package, hashes each file it is
//! made of and hands the digests in; that keeps the wire contract testable without a directory
//! and keeps the walk in the one place that already knows what a package artifact consists of.
//!
//! # Example
//!
//! ```
//! use ono_kuang_protocol::{FileDigest, SecretKey, SignedPackage};
//!
//! let key = SecretKey::from_bytes(&[7; 32]);
//! let files = vec![FileDigest::of_bytes("manifest.yaml", b"format: kuang-package/1\n")];
//! let described = SignedPackage::new("dev.example.demo", "1.0.0", "dev.example", files)?;
//! let signature = key.sign(&described);
//! assert!(signature.to_yaml().contains("algorithm: ed25519"));
//! # Ok::<(), ono_kuang_protocol::KuangError>(())
//! ```

use std::fmt;

use ed25519_dalek::ed25519::signature::Signer as _;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::error::{KuangError, KuangErrorCode};
use crate::manifest::Manifest;

/// The file a signed package carries beside its manifest.
pub const SIGNATURE_FILE: &str = "signature.yaml";

/// The `format:` line every signature document must open with.
pub const SIGNATURE_FORMAT: &str = "kuang-signature/1";

/// The only signature algorithm this build implements.
pub const SIGNATURE_ALGORITHM: &str = "ed25519";

/// The prefix a public key is spelled with, as spec §31.36's example prints it.
const KEY_PREFIX: &str = "ed25519:";

/// The prefix a secret key is spelled with, so the two can never be confused in a file.
const SECRET_PREFIX: &str = "ed25519-secret:";

fn invalid(message: impl Into<String>) -> KuangError {
    KuangError::new(KuangErrorCode::PackageSignatureInvalid, message)
}

/// Lowercase hex, the spelling `sha256:` digests already use in this product.
fn hex(bytes: &[u8]) -> String {
    use fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// Reads `text` as exactly `N` bytes of lowercase or uppercase hex.
fn unhex<const N: usize>(text: &str, what: &str) -> Result<[u8; N], KuangError> {
    if text.len() != N * 2 {
        return Err(invalid(format!(
            "the {what} is {} hex characters and an {SIGNATURE_ALGORITHM} {what} is {}",
            text.len(),
            N * 2
        )));
    }
    let mut bytes = [0_u8; N];
    for (slot, pair) in bytes.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        let digit = |byte: u8| match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            b'A'..=b'F' => Ok(byte - b'A' + 10),
            _ => Err(invalid(format!(
                "the {what} carries `{}`, which is not a hex digit",
                char::from(byte)
            ))),
        };
        *slot = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Ok(bytes)
}

/// The public half of a signing key, as `ed25519:<64 hex characters>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey(VerifyingKey);

impl PublicKey {
    /// Reads a key from the `ed25519:<hex>` form this product prints.
    ///
    /// # Errors
    ///
    /// `package.signature_invalid` when the prefix, the length or the point is not a key.
    pub fn parse(text: &str) -> Result<Self, KuangError> {
        let Some(digits) = text.strip_prefix(KEY_PREFIX) else {
            return Err(invalid(format!(
                "`{text}` names no algorithm; a key is written `{KEY_PREFIX}<64 hex characters>`"
            )));
        };
        let bytes = unhex::<32>(digits, "key")?;
        VerifyingKey::from_bytes(&bytes)
            .map(Self)
            .map_err(|error| invalid(format!("the key is not a point on the curve: {error}")))
    }

    /// The 32 raw bytes of the key.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{KEY_PREFIX}{}", hex(&self.0.to_bytes()))
    }
}

/// The private half of a signing key: what a package author holds and no host ever needs.
#[derive(Clone)]
pub struct SecretKey(SigningKey);

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The public half identifies the key without printing the secret one.
        write!(f, "SecretKey({})", self.public_key())
    }
}

impl SecretKey {
    /// A key from 32 raw bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(SigningKey::from_bytes(bytes))
    }

    /// A fresh key from the operating system's entropy.
    ///
    /// # Errors
    ///
    /// `package.signature_invalid` when the system offers no randomness to build a key from.
    pub fn generate() -> Result<Self, KuangError> {
        use std::io::Read as _;

        let mut bytes = [0_u8; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut source| source.read_exact(&mut bytes))
            .map_err(|error| {
                invalid(format!(
                    "no key can be made without entropy: /dev/urandom: {error}"
                ))
            })?;
        Ok(Self::from_bytes(&bytes))
    }

    /// Reads a key from the `ed25519-secret:<hex>` form [`SecretKey::to_secret_string`] writes.
    ///
    /// # Errors
    ///
    /// `package.signature_invalid` when the prefix or the length is not a key's.
    pub fn parse(text: &str) -> Result<Self, KuangError> {
        let Some(digits) = text.trim().strip_prefix(SECRET_PREFIX) else {
            return Err(invalid(format!(
                "a signing key is written `{SECRET_PREFIX}<64 hex characters>`"
            )));
        };
        Ok(Self::from_bytes(&unhex::<32>(digits, "signing key")?))
    }

    /// The key in the form [`SecretKey::parse`] reads. Secret: it belongs in a file only the
    /// author can read.
    #[must_use]
    pub fn to_secret_string(&self) -> String {
        format!("{SECRET_PREFIX}{}", hex(&self.0.to_bytes()))
    }

    /// The public half, which is what a signature document and a trust store carry.
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.0.verifying_key())
    }

    /// Signs the canonical description of a package.
    #[must_use]
    pub fn sign(&self, package: &SignedPackage) -> PackageSignature {
        let signature = self.0.sign(&package.canonical_bytes());
        PackageSignature::new(self.public_key(), package.clone(), signature.to_bytes())
    }
}

/// One file of a package artifact, with the digest of its contents.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FileDigest {
    /// The path relative to the package directory, as the manifest names it.
    pub path: String,
    /// The SHA-256 of the file's bytes, lowercase hex without an algorithm prefix.
    pub sha256: String,
}

impl FileDigest {
    /// The digest of `bytes` under `path`.
    #[must_use]
    pub fn of_bytes(path: impl Into<String>, bytes: &[u8]) -> Self {
        use sha2::Digest as _;

        let digest = sha2::Sha256::digest(bytes);
        Self {
            path: path.into(),
            sha256: hex(&digest),
        }
    }
}

/// What a signature commits to: this package, this version, this publisher, these files.
///
/// The description is canonical — the files are sorted by path and no path may carry a
/// character that could forge a line of the serialized form — so two walkers of the same
/// directory sign identical bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedPackage {
    /// The package id the signature is about.
    pub package: String,
    /// The version the signature is about.
    pub version: String,
    /// The publisher the signing key claims to be.
    pub publisher: String,
    /// Every file of the artifact, sorted by path.
    pub files: Vec<FileDigest>,
}

impl SignedPackage {
    /// Describes a package for signing.
    ///
    /// # Errors
    ///
    /// `package.signature_invalid` when a path is empty, carries a control character, or
    /// appears twice — each of which would make the canonical form ambiguous.
    pub fn new(
        package: impl Into<String>,
        version: impl Into<String>,
        publisher: impl Into<String>,
        files: impl IntoIterator<Item = FileDigest>,
    ) -> Result<Self, KuangError> {
        let mut files: Vec<FileDigest> = files.into_iter().collect();
        files.sort();
        for file in &files {
            if file.path.is_empty() || file.path.chars().any(char::is_control) {
                return Err(invalid(format!(
                    "`{}` cannot be described in a signature: a path carries no control \
                     characters and is never empty",
                    file.path.escape_debug()
                )));
            }
            let _: [u8; 32] = unhex(&file.sha256, "file digest")?;
        }
        if let Some(pair) = files.windows(2).find(|pair| pair[0].path == pair[1].path) {
            return Err(invalid(format!(
                "`{}` appears twice in the file list, so what is signed is ambiguous",
                pair[0].path
            )));
        }
        Ok(Self {
            package: package.into(),
            version: version.into(),
            publisher: publisher.into(),
            files,
        })
    }

    /// The exact bytes a key signs.
    ///
    /// A line-oriented form rather than re-serialized YAML: what is signed must not depend on a
    /// serializer's quoting, key order or line wrapping, and a reader in another language must
    /// be able to reproduce it from the fields alone.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut text = format!(
            "{SIGNATURE_FORMAT}\npackage {}\nversion {}\npublisher {}\n",
            self.package, self.version, self.publisher
        );
        for file in &self.files {
            text.push_str(&format!("file {} {}\n", file.sha256, file.path));
        }
        text.into_bytes()
    }
}

/// The document a signed package carries in `signature.yaml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSignature {
    key: PublicKey,
    signed: SignedPackage,
    bytes: [u8; 64],
}

/// The `signature.yaml` document as it is written and read.
#[derive(Debug, Serialize, Deserialize)]
struct SignatureDocument {
    format: String,
    algorithm: String,
    key: String,
    signed: SignedPackage,
    signature: String,
}

impl PackageSignature {
    /// A signature document from its parts. [`SecretKey::sign`] is how one is normally made.
    #[must_use]
    pub fn new(key: PublicKey, signed: SignedPackage, bytes: [u8; 64]) -> Self {
        Self { key, signed, bytes }
    }

    /// The key the document says made the signature. Whether that key is trusted is a separate
    /// question, answered by the operator's trust store (spec §31.36).
    #[must_use]
    pub const fn key(&self) -> &PublicKey {
        &self.key
    }

    /// What the signature commits to.
    #[must_use]
    pub const fn signed(&self) -> &SignedPackage {
        &self.signed
    }

    /// The publisher the signature claims.
    #[must_use]
    pub fn publisher(&self) -> &str {
        &self.signed.publisher
    }

    /// The raw signature bytes.
    #[must_use]
    pub const fn bytes(&self) -> &[u8; 64] {
        &self.bytes
    }

    /// Reads a `signature.yaml` document.
    ///
    /// # Errors
    ///
    /// `package.signature_invalid` for anything that is not a `kuang-signature/1` document this
    /// build can check — an unknown format, an algorithm it does not implement, a key or
    /// signature of the wrong shape. A document it cannot read is never treated as absent.
    pub fn parse(text: &str) -> Result<Self, KuangError> {
        let document: SignatureDocument = serde_yaml_ng::from_str(text).map_err(|error| {
            invalid(format!("the signature document does not parse: {error}")).with_help(
                "a package signature is a `kuang-signature/1` document beside the manifest \
                 (spec §31.36)",
            )
        })?;
        if document.format != SIGNATURE_FORMAT {
            return Err(invalid(format!(
                "`{}` is not a signature format this build reads; it reads `{SIGNATURE_FORMAT}`",
                document.format
            )));
        }
        if document.algorithm != SIGNATURE_ALGORITHM {
            return Err(invalid(format!(
                "`{}` is not a signature algorithm this build implements; it implements \
                 `{SIGNATURE_ALGORITHM}`",
                document.algorithm
            )));
        }
        let key = PublicKey::parse(&document.key)?;
        let bytes = unhex::<64>(&document.signature, "signature")?;
        // Re-describing the package enforces the canonical rules on a document from elsewhere.
        let signed = SignedPackage::new(
            document.signed.package,
            document.signed.version,
            document.signed.publisher,
            document.signed.files,
        )?;
        Ok(Self { key, signed, bytes })
    }

    /// The document as it is written to `signature.yaml`.
    #[must_use]
    pub fn to_yaml(&self) -> String {
        let document = SignatureDocument {
            format: SIGNATURE_FORMAT.to_owned(),
            algorithm: SIGNATURE_ALGORITHM.to_owned(),
            key: self.key.to_string(),
            signed: self.signed.clone(),
            signature: hex(&self.bytes),
        };
        let body = serde_yaml_ng::to_string(&document)
            .unwrap_or_else(|error| format!("# the signature does not serialise: {error}\n"));
        format!(
            "# A KUANG/11 package signature (spec §31.36). What is signed is the canonical\n\
             # description below, not this file: see `signed` for the exact commitment.\n{body}"
        )
    }

    /// Whether the signature bytes were made by the key the document names, over the
    /// description it carries.
    ///
    /// # Errors
    ///
    /// `package.signature_invalid` when they were not.
    pub fn verify_self(&self) -> Result<(), KuangError> {
        let signature = Signature::from_bytes(&self.bytes);
        self.key
            .0
            .verify_strict(&self.signed.canonical_bytes(), &signature)
            .map_err(|_| {
                invalid(format!(
                    "the signature was not made by {} over this package's description",
                    self.key
                ))
            })
    }

    /// The whole question: does this signature belong to this artifact?
    ///
    /// Three things must hold, and each failure names what broke: the signature was made by the
    /// key it carries; it is about this package and version; and it covers exactly the files
    /// the artifact is made of, byte for byte.
    ///
    /// # Errors
    ///
    /// `package.signature_invalid`, naming the file, the package or the key that did not match.
    pub fn check(&self, manifest: &Manifest, files: &[FileDigest]) -> Result<(), KuangError> {
        self.verify_self()?;
        if self.signed.package != manifest.package.id {
            return Err(invalid(format!(
                "the signature covers `{}` and this package is `{}`",
                self.signed.package, manifest.package.id
            )));
        }
        if self.signed.version != manifest.package.version {
            return Err(invalid(format!(
                "the signature covers `{}` version {} and this package is version {}",
                self.signed.package, self.signed.version, manifest.package.version
            )));
        }
        if self.signed.publisher != manifest.package.publisher {
            return Err(invalid(format!(
                "the signature claims publisher `{}` and the manifest declares `{}`",
                self.signed.publisher, manifest.package.publisher
            )));
        }
        for actual in files {
            match self
                .signed
                .files
                .iter()
                .find(|signed| signed.path == actual.path)
            {
                None => {
                    return Err(invalid(format!(
                        "`{}` is part of this package and the signature does not cover it",
                        actual.path
                    )));
                }
                Some(signed) if signed.sha256 != actual.sha256 => {
                    return Err(invalid(format!(
                        "`{}` is not the file that was signed",
                        actual.path
                    )));
                }
                Some(_) => {}
            }
        }
        for signed in &self.signed.files {
            if !files.iter().any(|actual| actual.path == signed.path) {
                return Err(invalid(format!(
                    "the signature covers `{}` and this package does not have it",
                    signed.path
                )));
            }
        }
        Ok(())
    }
}
