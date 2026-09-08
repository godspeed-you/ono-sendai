//! A package signature nobody keeps a key for (ADR-0609).
//!
//! Ono signs its own releases without a key: a workflow proves its identity with a short-lived
//! token, a certificate authority issues a certificate for that identity which lives about ten
//! minutes, and a transparency log records that it was used. This module lets a KUANG/11 package
//! be signed the same way, so publishing one needs no private key anybody has to keep.
//!
//! What it verifies, offline and in this order (ADR-0609 §2):
//!
//! 1. the bundle vouches for the bytes in hand, and nothing else;
//! 2. the certificate chains to an embedded certificate authority, for code signing;
//! 3. the certificate's key made the signature over those bytes;
//! 4. the transparency log's signed timestamp verifies under an embedded log key, and the moment
//!    it records falls inside the certificate's ten-minute life;
//! 5. the certificate names a subject identity and an issuer, which the caller compares with what
//!    an operator enrolled.
//!
//! Step 5 is where this module stops. *Valid* and *trusted* are two questions (K11P §18.1), and
//! this one answers only the first.

use base64::Engine as _;
use rustls_pki_types::{CertificateDer, TrustAnchor, UnixTime};
use serde::Deserialize;

use ono_kuang_protocol::{KuangError, KuangErrorCode, Manifest, SignedPackage};

/// Sigstore's published trust material, as this release took it (ADR-0609 §4).
///
/// Data, embedded rather than fetched: an install must work on a machine with no route out.
pub const TRUST_ROOT: &str = include_str!("../../../docs/contracts/kuang/sigstore-trust-root.json");

/// The extended key usage a code-signing certificate must carry: 1.3.6.1.5.5.7.3.3.
const CODE_SIGNING: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x03];

/// Fulcio's OIDC issuer extension, 1.3.6.1.4.1.57264.1.8, whose value is a DER `UTF8String`.
const ISSUER_V2: &str = "1.3.6.1.4.1.57264.1.8";
/// The first spelling of the same thing, 1.3.6.1.4.1.57264.1.1, whose value is the raw string.
const ISSUER_V1: &str = "1.3.6.1.4.1.57264.1.1";

/// Who a bundle says signed, once it has verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeylessIdentity {
    /// The OpenID issuer that authenticated the signer, e.g.
    /// `https://token.actions.githubusercontent.com`.
    pub issuer: String,
    /// The subject the certificate names, e.g. a workflow at a tag.
    pub subject: String,
    /// When the transparency log recorded the signature, in seconds since the epoch.
    pub signed_at: i64,
}

impl KeylessIdentity {
    /// Whether an enrolled `subject` pattern names this identity.
    ///
    /// An exact string, or one ending in `*`, which matches a prefix. A repository enrols its
    /// releases once rather than once per tag, and the wildcard is deliberately the only one:
    /// a pattern language in a trust store is a way to enrol more than was meant.
    #[must_use]
    pub fn matches(&self, issuer: &str, subject: &str) -> bool {
        if self.issuer != issuer {
            return false;
        }
        match subject.strip_suffix('*') {
            Some(prefix) => self.subject.starts_with(prefix),
            None => self.subject == subject,
        }
    }
}

/// Verifies the keyless signature a package carries, against the package on disk (ADR-0609 §1).
///
/// The bundle covers exactly the bytes an ed25519 signature covers: the package's own
/// description, recomputed here from the files that are there. So a package signed either way is
/// signed about the same thing, and a bundle that vouches for anything else is refused.
///
/// # Errors
///
/// `package.signature_invalid` when the bundle does not verify, and `package.invalid` when the
/// description cannot be built from the package at all.
pub fn check_package(
    bundle: &str,
    manifest: &Manifest,
    files: Vec<ono_kuang_protocol::FileDigest>,
) -> Result<KeylessIdentity, KuangError> {
    let described = SignedPackage::new(
        &manifest.package.id,
        &manifest.package.version,
        &manifest.package.publisher,
        files,
    )?;
    verify(bundle, &described.canonical_bytes())
}

/// Verifies `bundle` against `payload` under the embedded trust root.
///
/// # Errors
///
/// `package.signature_invalid` with the step that refused. Nothing here reports *why* in terms a
/// forger could use to iterate: the message names the check, not the value that failed it.
pub fn verify(bundle: &str, payload: &[u8]) -> Result<KeylessIdentity, KuangError> {
    let bundle: Bundle = serde_json::from_str(bundle).map_err(|error| {
        invalid(format!(
            "the keyless signature is not a Sigstore bundle this host reads: {error}"
        ))
    })?;
    let root = TrustRoot::embedded()?;

    // 1. The bundle vouches for these bytes (ADR-0609 §2 step 1).
    let digest = bundle.message_signature.message_digest.decoded()?;
    let actual = <sha2::Sha256 as sha2::Digest>::digest(payload);
    if digest != actual.as_slice() {
        return Err(invalid(
            "the keyless signature vouches for other bytes than the package's own description"
                .to_owned(),
        ));
    }

    // 4a. The moment the log recorded, which is what the certificate is judged at.
    let entry = bundle
        .verification_material
        .tlog_entries
        .first()
        .ok_or_else(|| {
            invalid("the keyless signature carries no transparency-log entry".to_owned())
        })?;
    let signed_at = entry
        .integrated_time
        .parse::<i64>()
        .map_err(|_| invalid("the transparency-log entry carries no readable time".to_owned()))?;
    let at = UnixTime::since_unix_epoch(std::time::Duration::from_secs(
        u64::try_from(signed_at).map_err(|_| {
            invalid("the transparency-log entry is dated before the epoch".to_owned())
        })?,
    ));

    // 2. The certificate chains to an authority this host carries, for code signing, at that
    //    moment. Path building and constraints belong to a library that does them for a living.
    let leaf_der = decode(
        &bundle.verification_material.certificate.raw_bytes,
        "certificate",
    )?;
    let leaf = CertificateDer::from(leaf_der.as_slice());
    let end_entity = webpki::EndEntityCert::try_from(&leaf)
        .map_err(|error| invalid(format!("the signing certificate does not parse: {error}")))?;
    let mut chained = Err(invalid(
        "the signing certificate chains to no authority this host carries".to_owned(),
    ));
    for authority in &root.authorities {
        let anchors: Vec<TrustAnchor<'_>> = authority
            .roots
            .iter()
            .filter_map(|der| webpki::anchor_from_trusted_cert(der).ok())
            .collect();
        if anchors.is_empty() {
            continue;
        }
        if end_entity
            .verify_for_usage(
                &[
                    webpki::ring::ECDSA_P256_SHA256,
                    webpki::ring::ECDSA_P384_SHA384,
                ],
                &anchors,
                &authority.intermediates,
                at,
                webpki::KeyUsage::required(CODE_SIGNING),
                None,
                None,
            )
            .is_ok()
        {
            chained = Ok(());
            break;
        }
    }
    chained?;

    // 3. That certificate's key made this signature over these bytes.
    let signature = decode(&bundle.message_signature.signature, "signature")?;
    end_entity
        .verify_signature(webpki::ring::ECDSA_P256_SHA256, payload, &signature)
        .map_err(|_| invalid("the signing certificate did not make this signature".to_owned()))?;

    // 4b. The log's own signature over the entry, under a log key this host carries. Without it
    //     an ephemeral key that leaked from a run would be usable for ever, because nothing else
    //     says *when* the signature was made.
    let promise = entry.inclusion_promise.as_ref().ok_or_else(|| {
        invalid("the transparency-log entry carries no signed timestamp".to_owned())
    })?;
    let log_key = root.log_key(&entry.log_id.key_id).ok_or_else(|| {
        invalid("the transparency log that recorded this is not one this host carries".to_owned())
    })?;
    let set = signed_entry_timestamp(entry)?;
    log_key.verify(
        &set,
        &decode(&promise.signed_entry_timestamp, "signed timestamp")?,
    )?;

    // 5. Who the certificate says signed. The caller decides whether that is trusted.
    let (issuer, subject) = identity(&leaf_der)?;
    Ok(KeylessIdentity {
        issuer,
        subject,
        signed_at,
    })
}

/// The bytes a transparency log signs when it accepts an entry: its canonical JSON, with the keys
/// in the order the log itself writes them.
fn signed_entry_timestamp(entry: &TlogEntry) -> Result<Vec<u8>, KuangError> {
    let log_id = decode(&entry.log_id.key_id, "log id")?;
    let hex: String = log_id.iter().map(|byte| format!("{byte:02x}")).collect();
    let index = entry
        .log_index
        .parse::<i64>()
        .map_err(|_| invalid("the transparency-log entry carries no readable index".to_owned()))?;
    // Written by hand rather than serialized: what a log signed must not depend on this build's
    // serializer, and the field order is part of the definition.
    Ok(format!(
        "{{\"body\":\"{}\",\"integratedTime\":{},\"logID\":\"{hex}\",\"logIndex\":{index}}}",
        entry.canonicalized_body, entry.integrated_time
    )
    .into_bytes())
}

/// The subject identity and the OIDC issuer a Fulcio certificate carries.
fn identity(der: &[u8]) -> Result<(String, String), KuangError> {
    use x509_parser::prelude::*;

    let (_, certificate) = X509Certificate::from_der(der)
        .map_err(|error| invalid(format!("the signing certificate does not parse: {error}")))?;
    let mut issuer = None;
    for extension in certificate.extensions() {
        let oid = extension.oid.to_id_string();
        if oid == ISSUER_V2 {
            // A DER `UTF8String`: tag, length, then the bytes.
            let value = extension.value;
            if value.len() >= 2 && value[0] == 0x0c {
                let start = 2 + usize::from(value[1] > 0x80) * usize::from(value[1] & 0x7f);
                let text = value.get(start..).unwrap_or_default();
                issuer = Some(String::from_utf8_lossy(text).into_owned());
            }
        } else if oid == ISSUER_V1 && issuer.is_none() {
            issuer = Some(String::from_utf8_lossy(extension.value).into_owned());
        }
    }
    let issuer = issuer.ok_or_else(|| {
        invalid("the signing certificate names no OpenID issuer, so nothing says who authenticated the signer".to_owned())
    })?;

    let subject = certificate
        .extensions()
        .iter()
        .find_map(|extension| match extension.parsed_extension() {
            ParsedExtension::SubjectAlternativeName(san) => {
                san.general_names.iter().find_map(|name| match name {
                    GeneralName::URI(uri) => Some((*uri).to_owned()),
                    _ => None,
                })
            }
            _ => None,
        })
        .ok_or_else(|| invalid("the signing certificate names no subject identity".to_owned()))?;
    Ok((issuer, subject))
}

// --- the embedded trust root ---------------------------------------------------------------------

/// One certificate authority of the trust root, as verification needs it.
struct Authority {
    roots: Vec<CertificateDer<'static>>,
    intermediates: Vec<CertificateDer<'static>>,
}

/// A transparency log's key, by the two algorithms Sigstore's logs have used.
enum LogKey {
    EcdsaP256(Vec<u8>),
    Ed25519(Vec<u8>),
}

impl LogKey {
    fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), KuangError> {
        let refused = || invalid("the transparency log did not sign this entry".to_owned());
        match self {
            LogKey::EcdsaP256(key) => ring::signature::UnparsedPublicKey::new(
                &ring::signature::ECDSA_P256_SHA256_ASN1,
                key,
            )
            .verify(message, signature)
            .map_err(|_| refused()),
            LogKey::Ed25519(key) => {
                ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
                    .verify(message, signature)
                    .map_err(|_| refused())
            }
        }
    }
}

/// The authorities and log keys this build carries.
struct TrustRoot {
    authorities: Vec<Authority>,
    logs: Vec<(String, LogKey)>,
}

impl TrustRoot {
    fn embedded() -> Result<Self, KuangError> {
        let root: RawTrustRoot = serde_json::from_str(TRUST_ROOT).map_err(|error| {
            invalid(format!(
                "this build's Sigstore trust root does not read: {error}"
            ))
        })?;
        let mut authorities = Vec::new();
        for authority in &root.certificate_authorities {
            let mut certificates = Vec::new();
            for certificate in &authority.cert_chain.certificates {
                certificates.push(CertificateDer::from(decode(
                    &certificate.raw_bytes,
                    "trust root certificate",
                )?));
            }
            // A chain is written leaf-most first: the last is the root, the rest are on the way.
            let Some(root_cert) = certificates.pop() else {
                continue;
            };
            authorities.push(Authority {
                roots: vec![root_cert],
                intermediates: certificates,
            });
        }
        let mut logs = Vec::new();
        for log in &root.tlogs {
            let key = decode(&log.public_key.raw_bytes, "trust root log key")?;
            let key = match log.public_key.key_details.as_str() {
                // The key is a DER SubjectPublicKeyInfo; both verifiers here take the raw point,
                // which is the trailing bit string.
                "PKIX_ECDSA_P256_SHA_256" => LogKey::EcdsaP256(spki_key(&key)),
                "PKIX_ED25519" | "PKIX_ED25519_PH" => LogKey::Ed25519(spki_key(&key)),
                _ => continue,
            };
            logs.push((log.log_id.key_id.clone(), key));
        }
        Ok(Self { authorities, logs })
    }

    fn log_key(&self, key_id: &str) -> Option<&LogKey> {
        self.logs
            .iter()
            .find(|(id, _)| id == key_id)
            .map(|(_, key)| key)
    }
}

/// The public key inside a DER `SubjectPublicKeyInfo`: the trailing `BIT STRING`'s contents.
///
/// Read positionally rather than parsed, because the shape is fixed for the two algorithms this
/// accepts and a general parser would be more code for one field. A key that does not have this
/// shape yields bytes that verify nothing, which is a refusal rather than an acceptance.
fn spki_key(der: &[u8]) -> Vec<u8> {
    let mut at = 0;
    while at < der.len() {
        if der[at] == 0x03 && at + 2 < der.len() {
            let length = usize::from(der[at + 1]);
            if der[at + 2] == 0x00 && at + 2 + length <= der.len() {
                return der[at + 3..at + 2 + length].to_vec();
            }
        }
        at += 1;
    }
    Vec::new()
}

fn decode(text: &str, what: &str) -> Result<Vec<u8>, KuangError> {
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|_| invalid(format!("the keyless signature's {what} is not base64")))
}

fn invalid(message: String) -> KuangError {
    KuangError::new(KuangErrorCode::PackageSignatureInvalid, message)
}

// --- the wire shapes -----------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bundle {
    verification_material: VerificationMaterial,
    message_signature: MessageSignature,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerificationMaterial {
    certificate: RawCertificate,
    #[serde(default)]
    tlog_entries: Vec<TlogEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCertificate {
    raw_bytes: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TlogEntry {
    log_index: String,
    log_id: LogId,
    integrated_time: String,
    #[serde(default)]
    inclusion_promise: Option<InclusionPromise>,
    canonicalized_body: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogId {
    key_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InclusionPromise {
    signed_entry_timestamp: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageSignature {
    message_digest: MessageDigest,
    signature: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageDigest {
    algorithm: String,
    digest: String,
}

impl MessageDigest {
    fn decoded(&self) -> Result<Vec<u8>, KuangError> {
        if self.algorithm != "SHA2_256" {
            return Err(invalid(format!(
                "the keyless signature digests with `{}`, and this host reads SHA2_256",
                self.algorithm
            )));
        }
        decode(&self.digest, "digest")
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTrustRoot {
    #[serde(default)]
    certificate_authorities: Vec<RawAuthority>,
    #[serde(default)]
    tlogs: Vec<RawLog>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAuthority {
    cert_chain: RawChain,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawChain {
    certificates: Vec<RawCertificate>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLog {
    log_id: LogId,
    public_key: RawLogKey,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLogKey {
    raw_bytes: String,
    key_details: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Sigstore bundle this project's own `v0.4.3` release published over its `SHA256SUMS`,
    /// with the bytes it covers. A real certificate, a real entry in the public transparency log
    /// and a real signature: a verifier that accepts a fixture somebody invented proves nothing
    /// about the one case that matters. They live under `tests/fixtures` because that is where
    /// this crate keeps fixtures, and they are read from here because the logic they exercise is
    /// a module's own rather than the shell's.
    const BUNDLE: &str = include_str!("../tests/fixtures/release-SHA256SUMS.sigstore.json");
    const PAYLOAD: &[u8] = include_bytes!("../tests/fixtures/release-SHA256SUMS");

    #[test]
    fn should_read_the_trust_root_this_build_carries() {
        // The embedded material is what every verification rests on; a build that shipped an
        // unreadable one would refuse every keyless signature for a reason nobody could see.
        let root = TrustRoot::embedded().expect("the embedded trust root reads");
        assert!(
            !root.authorities.is_empty(),
            "it carries at least one certificate authority"
        );
        assert!(
            root.authorities.iter().all(|a| !a.roots.is_empty()),
            "and every authority has a root to chain to"
        );
        assert!(root.logs.len() >= 2, "and the log keys, old and new");
    }

    #[test]
    fn should_match_an_enrolled_subject_exactly_or_by_one_trailing_wildcard() {
        let identity = KeylessIdentity {
            issuer: "https://token.actions.githubusercontent.com".to_owned(),
            subject: "https://github.com/o/r/.github/workflows/release.yml@refs/tags/v1.0.0"
                .to_owned(),
            signed_at: 0,
        };
        let issuer = "https://token.actions.githubusercontent.com";
        assert!(
            identity.matches(issuer, &identity.subject),
            "the exact subject"
        );
        assert!(
            identity.matches(
                issuer,
                "https://github.com/o/r/.github/workflows/release.yml@refs/tags/*"
            ),
            "a repository's releases, enrolled once"
        );
        assert!(
            !identity.matches(
                issuer,
                "https://github.com/other/r/.github/workflows/release.yml@refs/tags/*"
            ),
            "another repository's are not"
        );
        assert!(
            !identity.matches("https://accounts.example.com", &identity.subject),
            "and the issuer is part of the identity"
        );
    }

    #[test]
    fn should_refuse_a_bundle_that_is_not_one() {
        let refused = verify("{}", b"anything").expect_err("an empty object is not a bundle");
        assert_eq!(refused.code(), KuangErrorCode::PackageSignatureInvalid);
    }
    const ISSUER: &str = "https://token.actions.githubusercontent.com";

    #[test]
    fn should_verify_the_bundle_this_project_published_and_name_who_signed_it() {
        let identity = verify(BUNDLE, PAYLOAD).expect("the release bundle verifies");
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
        let refused = verify(BUNDLE, &tampered).expect_err("other bytes are refused");
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
        let refused = verify(&bundle.to_string(), PAYLOAD)
            .expect_err("a signature the certificate did not make is refused");
        assert_eq!(refused.code(), KuangErrorCode::PackageSignatureInvalid);
    }

    #[test]
    fn should_refuse_a_bundle_whose_log_timestamp_was_replaced() {
        // Step 4: the log's own signature over its entry. Moving the recorded time would move the
        // certificate's ten-minute window, which is the whole reason the log is consulted.
        let mut bundle: serde_json::Value =
            serde_json::from_str(BUNDLE).expect("the fixture reads");
        bundle["verificationMaterial"]["tlogEntries"][0]["integratedTime"] =
            serde_json::Value::String("1788871999".to_owned());
        let refused = verify(&bundle.to_string(), PAYLOAD)
            .expect_err("an entry the log did not sign is refused");
        assert_eq!(refused.code(), KuangErrorCode::PackageSignatureInvalid);
    }

    #[test]
    fn should_refuse_a_bundle_with_no_transparency_log_entry() {
        // Without the log nothing says *when* the signature was made, and a leaked ephemeral key
        // would be usable for ever.
        let mut bundle: serde_json::Value =
            serde_json::from_str(BUNDLE).expect("the fixture reads");
        bundle["verificationMaterial"]["tlogEntries"] = serde_json::Value::Array(Vec::new());
        let refused = verify(&bundle.to_string(), PAYLOAD)
            .expect_err("a bundle with no log entry is refused");
        assert!(
            refused.message().contains("transparency-log"),
            "and says what is missing: {}",
            refused.message()
        );
    }
}
