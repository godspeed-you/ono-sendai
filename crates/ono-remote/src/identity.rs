//! The identity this end of a link proves it holds (v0.4.1 spec §7.2, §8; ADR-0434).
//!
//! Before v0.4.1 only the listening side had an identity, and the type was named for that:
//! `HostIdentity`, a host's certificate and key. v0.4.1 §7.1 makes the direct transport
//! symmetric — "both endpoints MUST present a certificate and prove possession of the
//! corresponding private key during TLS 1.3 negotiation" — so the same object now stands on both
//! ends of the same wire, and §7.2 asks for it under a name that does not claim otherwise:
//!
//! > The implementation SHOULD generalize the current host-only identity abstraction into a
//! > transport-neutral `PeerIdentity` or equivalent concept.
//!
//! # What it carries, and what it says
//!
//! §7.2 lists algorithm, public material, private key, fingerprint, storage location and
//! creation metadata, and then fixes what may leave the object:
//!
//! > The public contract is the fingerprint. The private key MUST never be serialized into
//! > ordinary structured pipeline output, logs, diagnostics or crash messages.
//!
//! So [`PeerIdentity`] writes its own `Debug` rather than deriving one. A derived `Debug` is a
//! promise about every field a later maintainer adds, and this type is one where that promise
//! has to be made by hand.
//!
//! # What it is not
//!
//! It is not the runtime [`Identity`](ono_protocol::Identity) of §7.3 — the user, uid and
//! elevation the far side reports about itself. That is context and grants nothing; this is the
//! key a peer proves it holds, and the only thing an authorization decision may rest on.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ono_core::ErrorCode;
use ono_protocol::{Fingerprint, HostKey};
use ono_value::ErrorValue;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

/// The algorithm name the trust store records for a key proved through TLS.
///
/// The material is the peer's end-entity certificate, so the name says so: what was pinned is a
/// certificate, and a re-issued certificate is a new key to Ono even when the key inside it is
/// the same one. That is the strict reading, and the one a person can check by hand.
pub(crate) const ALGORITHM: &str = "tls-x509";

/// One end's own identity: the certificate it presents and the key it proves it holds.
///
/// Held by the listening agent and by the connecting client alike (v0.4.1 §7.1), which is why it
/// is named for the *peer* the far side sees rather than for a host.
pub struct PeerIdentity {
    certificate: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    path: Option<PathBuf>,
    created: Option<SystemTime>,
}

/// What an identity says about itself: the fingerprint that is its public contract, and enough
/// context to tell two of them apart. Never the key (v0.4.1 §7.2).
impl fmt::Debug for PeerIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PeerIdentity")
            .field("algorithm", &ALGORITHM)
            .field("fingerprint", &self.fingerprint().to_string())
            .field(
                "path",
                &self.path.as_ref().map(|path| path.display().to_string()),
            )
            .field("private_key", &"<withheld>")
            .finish()
    }
}

impl PeerIdentity {
    /// The identity recorded in `path`, generating and writing one when the file is not there.
    ///
    /// The file holds both PEM blocks and is written with owner-only permissions, because it
    /// contains the private key that *is* this end's identity to everyone who pinned it.
    ///
    /// # Errors
    ///
    /// `io.permission_denied` when the file cannot be read or written, and `parse.syntax` when it
    /// is not the two PEM blocks this format defines.
    pub fn open_or_create(path: &Path) -> Result<Self, ErrorValue> {
        refuse_exposed_permissions(path)?;
        match std::fs::read_to_string(path) {
            Ok(pem) => Self::from_file(&pem, path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let pem = generate_pem()?;
                write_private(path, &pem)?;
                Self::from_file(&pem, path)
            }
            Err(error) => Err(io_error(path, &error)),
        }
    }

    /// An identity that lives only as long as this process, stored nowhere.
    ///
    /// # Errors
    ///
    /// `io.permission_denied` when a key cannot be generated.
    pub fn generate() -> Result<Self, ErrorValue> {
        let pem = generate_pem()?;
        let (certificate, key) = parse_pem(&pem, Path::new("<generated>"))?;
        Ok(Self {
            certificate,
            key,
            path: None,
            created: Some(SystemTime::now()),
        })
    }

    fn from_file(pem: &str, path: &Path) -> Result<Self, ErrorValue> {
        let (certificate, key) = parse_pem(pem, path)?;
        let created = std::fs::metadata(path)
            .and_then(|metadata| metadata.created().or_else(|_| metadata.modified()))
            .ok();
        Ok(Self {
            certificate,
            key,
            path: Some(path.to_path_buf()),
            created,
        })
    }

    /// How this identity is proved, as the trust store spells it.
    #[must_use]
    pub const fn algorithm(&self) -> &'static str {
        ALGORITHM
    }

    /// The public material: the end-entity certificate a peer sees.
    #[must_use]
    pub fn certificate(&self) -> &[u8] {
        self.certificate.as_ref()
    }

    /// The key a peer will see this end prove it holds.
    #[must_use]
    pub fn peer_key(&self) -> HostKey {
        HostKey::new(ALGORITHM, self.certificate.as_ref().to_vec())
    }

    /// The fingerprint a person pins this end by — the public contract of §7.2.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        self.peer_key().fingerprint()
    }

    /// The file this identity was read from, or `None` for one that lives only in this process.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// When the identity came into being, where the filesystem records it.
    #[must_use]
    pub const fn created(&self) -> Option<SystemTime> {
        self.created
    }

    /// The certificate and key, for the one caller that has to hand them to a TLS configuration.
    pub(crate) fn material(&self) -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
        (self.certificate.clone(), self.key.clone_key())
    }
}

fn parse_pem(
    pem: &str,
    path: &Path,
) -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>), ErrorValue> {
    let malformed = |detail: &str| {
        ErrorValue::new(
            ErrorCode::ParseSyntax,
            format!("{}: {detail}", path.display()),
        )
        .with_help(
            "a peer identity is one CERTIFICATE block and one PRIVATE KEY block; remove the \
             file to have a new identity generated, which every peer that pinned this one will \
             then refuse until it is re-pinned",
        )
    };
    let mut reader = io::BufReader::new(pem.as_bytes());
    let certificate = rustls_pemfile::certs(&mut reader)
        .next()
        .transpose()
        .map_err(|error| malformed(&format!("the certificate is unreadable: {error}")))?
        .ok_or_else(|| malformed("no CERTIFICATE block"))?;
    let mut reader = io::BufReader::new(pem.as_bytes());
    let key = rustls_pemfile::private_key(&mut reader)
        .map_err(|error| malformed(&format!("the private key is unreadable: {error}")))?
        .ok_or_else(|| malformed("no PRIVATE KEY block"))?;
    Ok((certificate, key))
}

/// A self-signed certificate and its key, as two PEM blocks.
///
/// The randomness is `ring`'s, reached through `rcgen`, which is what v0.4.1 §8.4 asks for: "
/// identity generation MUST use a cryptographically secure RNG through the selected TLS/key
/// library."
fn generate_pem() -> Result<String, ErrorValue> {
    // The name in the certificate is not what identifies the peer — the pinned key is (see
    // `tls`'s module documentation) — so it is a fixed, obviously non-resolvable name rather
    // than something that looks like a claim about DNS.
    let generated =
        rcgen::generate_simple_self_signed(vec!["ono.invalid".to_owned()]).map_err(|error| {
            ErrorValue::new(
                ErrorCode::IoPermissionDenied,
                format!("this peer identity could not be generated: {error}"),
            )
        })?;
    Ok(format!(
        "{}{}",
        generated.cert.pem(),
        generated.key_pair.serialize_pem()
    ))
}

/// Refuses an identity file anyone but its owner can read or write (v0.4.1 §8.3, §59.6).
///
/// This is a refusal and not a warning, in both directions and for the same reason in each: a
/// private key another account can read is a key another account can *be* this peer with, and a
/// private key another account can write is an identity another account chooses. §2.9 forbids
/// asking about it, so nothing here offers a way past.
///
/// A file that does not exist yet is not exposed; [`write_private`] creates it `0600`.
fn refuse_exposed_permissions(path: &Path) -> Result<(), ErrorValue> {
    use std::os::unix::fs::PermissionsExt as _;

    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(path, &error)),
    };
    let mode = metadata.permissions().mode() & 0o777;
    let readable = mode & 0o044 != 0;
    let writable = mode & 0o022 != 0;
    if !readable && !writable {
        return Ok(());
    }
    // Named exactly, because "wrong permissions" sends a person to `ls -l` to work out which of
    // the two problems they have, and the two have different consequences.
    let exposure = match (readable, writable) {
        (true, true) => "readable and writable",
        (true, false) => "readable",
        (false, true) => "writable",
        (false, false) => unreachable!("the owner-only case returned above"),
    };
    Err(ErrorValue::new(
        ErrorCode::RemoteIdentityPermissions,
        format!(
            "the peer identity at {} is {exposure} by group or others (mode {mode:04o}); it must \
             be 0600",
            path.display()
        ),
    )
    .with_retryable(false)
    .with_help(format!(
        "the file holds the private key that is this machine's identity on a direct link \
         (v0.4.1 §8.3). Run `chmod 600 {}`. Nothing was read from inside it; if another account \
         may already have read it, treat the identity as disclosed and rotate it deliberately \
         (§8.6).",
        path.display()
    )))
}

/// Writes `contents` to `path` so that only its owner can read it (v0.4.1 §8.3).
fn write_private(path: &Path, contents: &str) -> Result<(), ErrorValue> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| io_error(path, &error))?;
    file.write_all(contents.as_bytes())
        .map_err(|error| io_error(path, &error))?;
    file.sync_all().map_err(|error| io_error(path, &error))
}

pub(crate) fn io_error(path: &Path, error: &io::Error) -> ErrorValue {
    let code = match error.kind() {
        io::ErrorKind::NotFound => ErrorCode::IoNotFound,
        _ => ErrorCode::IoPermissionDenied,
    };
    ErrorValue::new(
        code,
        format!(
            "the peer identity at {} is not usable: {error}",
            path.display()
        ),
    )
}
