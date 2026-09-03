//! The identity this shell proves it holds on a direct link, and the file it lives in
//! (v0.4.1 spec §8.1, §8.2, §8.3, §8.4; issues #33, #34).
//!
//! §8.1 fixes the path — `~/.config/ono/link_identity.pem`, under whatever the ordinary
//! configuration-directory resolution answers — and §8.2 fixes what happens on a machine that
//! already ran a listening agent and therefore already has a `host_key.pem`: that identity is
//! reused, not silently replaced by a second unrelated one. §8.3 then refuses an identity file
//! anyone else on the machine can read.
//!
//! Everything runs against a scratch directory, so no suite here needs a network, a real home
//! directory or a key that matters.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::os::unix::fs::PermissionsExt as _;

use ono_cli::trust::{HOST_KEY_FILE, LINK_IDENTITY_FILE, link_identity};
use ono_remote::PeerIdentity;
use ono_testkit::scratch;

fn mode_of(path: &std::path::Path) -> u32 {
    std::fs::metadata(path)
        .expect("the identity file exists")
        .permissions()
        .mode()
        & 0o777
}

#[test]
fn should_generate_one_identity_and_keep_it_across_calls() {
    let home = scratch();

    let first = link_identity(home.path()).expect("an identity is generated on first use");
    let again = link_identity(home.path()).expect("the written identity is read back");

    assert_eq!(
        first.fingerprint(),
        again.fingerprint(),
        "v0.4.1 §8.1: a direct-link client MUST have a *persistent* peer identity, or the server \
         cannot authorize it consistently across connections"
    );
    let path = home.path().join(LINK_IDENTITY_FILE);
    assert!(
        path.is_file(),
        "§8.1 names the file `link_identity.pem` under the configuration directory"
    );
    assert_eq!(
        mode_of(&path),
        0o600,
        "§8.3: private identity files MUST be created with owner-read/write permissions only"
    );
}

#[test]
fn should_reuse_a_legacy_host_key_rather_than_generate_a_second_unrelated_identity() {
    let home = scratch();
    let legacy = home.path().join(HOST_KEY_FILE);
    let previous = PeerIdentity::open_or_create(&legacy).expect("the legacy identity is written");
    let expected = previous.fingerprint();
    let legacy_before = std::fs::read_to_string(&legacy).expect("the legacy file is readable");

    let identity = link_identity(home.path()).expect("the legacy identity is migrated");

    assert_eq!(
        identity.fingerprint(),
        expected,
        "v0.4.1 §8.2: an installation that already has `host_key.pem` MUST reuse that identity \
         rather than silently generate a second unrelated identity"
    );
    let canonical = home.path().join(LINK_IDENTITY_FILE);
    assert_eq!(
        mode_of(&canonical),
        0o600,
        "§8.2 rule 2 migrates the legacy file `while preserving mode 0600`"
    );
    assert_eq!(
        std::fs::read_to_string(&legacy).expect("the legacy file is still readable"),
        legacy_before,
        "§8.2 rule 4: never delete the legacy file automatically in v0.4.1 — a listening agent \
         started with the old flag must keep the identity its peers pinned"
    );
}

#[test]
fn should_prefer_an_existing_link_identity_over_the_legacy_file() {
    let home = scratch();
    let canonical = PeerIdentity::open_or_create(&home.path().join(LINK_IDENTITY_FILE))
        .expect("the canonical identity is written");
    let expected = canonical.fingerprint();
    let legacy = PeerIdentity::open_or_create(&home.path().join(HOST_KEY_FILE))
        .expect("an unrelated legacy identity is written");
    assert_ne!(
        expected,
        legacy.fingerprint(),
        "the fixture needs the two files to differ for the preference to mean anything"
    );

    let identity = link_identity(home.path()).expect("the canonical identity is read");

    assert_eq!(
        identity.fingerprint(),
        expected,
        "v0.4.1 §8.2 rule 1: if `link_identity.pem` exists, use it"
    );
}

#[test]
fn should_generate_a_fresh_identity_when_the_legacy_file_does_not_parse() {
    let home = scratch();
    let legacy = home.path().join(HOST_KEY_FILE);
    std::fs::write(&legacy, "this was never a key\n").expect("the unreadable legacy file exists");
    // Owner-only, so this test is about the file not parsing and not about §8.3's refusal.
    std::fs::set_permissions(&legacy, std::fs::Permissions::from_mode(0o600))
        .expect("the fixture file is owner-only");

    let identity = link_identity(home.path()).expect("a fresh identity is generated instead");

    assert!(
        home.path().join(LINK_IDENTITY_FILE).is_file(),
        "v0.4.1 §8.2 rule 2 migrates the legacy file only `if it exists and parses`; rule 3 \
         generates one otherwise, got {identity:?}"
    );
    assert!(
        legacy.is_file(),
        "§8.2 rule 4 holds for a file that could not be read too: nothing is deleted"
    );
}

/// v0.4.1 §59.6, the security acceptance scenario: "a world-readable `link_identity.pem` MUST be
/// refused. The diagnostic identifies the path and required permissions without printing key
/// material." §2.9 makes it non-interactive, so what a script sees is a stable code.
#[test]
fn should_refuse_a_group_or_world_readable_identity_and_establish_no_link() {
    let home = scratch();
    let path = home.path().join(LINK_IDENTITY_FILE);
    link_identity(home.path()).expect("an identity is generated 0600");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
        .expect("the file is made world-readable");

    let refusal = link_identity(home.path()).expect_err("an exposed private key is refused");

    assert_eq!(
        refusal.code(),
        ono_core::ErrorCode::RemoteIdentityPermissions,
        "§8.3 refuses rather than warns, and §2.9 forbids asking the user about it"
    );
    let said = format!(
        "{} {}",
        refusal.message(),
        refusal.help().unwrap_or_default()
    );
    assert!(
        said.contains(&path.display().to_string()) && said.contains("600"),
        "§59.6: the diagnostic identifies the path and the required permissions, got {said}"
    );
}

#[test]
fn should_refuse_an_exposed_legacy_host_key_rather_than_generate_a_second_identity() {
    let home = scratch();
    let legacy = home.path().join(HOST_KEY_FILE);
    PeerIdentity::open_or_create(&legacy).expect("the legacy identity is written");
    std::fs::set_permissions(&legacy, std::fs::Permissions::from_mode(0o644))
        .expect("the legacy file is made world-readable");

    let refusal = link_identity(home.path()).expect_err("an exposed legacy key is refused");

    assert_eq!(
        refusal.code(),
        ono_core::ErrorCode::RemoteIdentityPermissions,
        "stepping over an exposed legacy key would generate the second unrelated identity §8.2 \
         forbids, out of a security problem the operator was never shown"
    );
    assert!(
        !home.path().join(LINK_IDENTITY_FILE).exists(),
        "nothing is written on the way to a refusal"
    );
}

/// The non-secret way to print the local peer fingerprint (v0.4.1 §8.5).
///
/// > Canonical global invocation: `ono --print-peer-key`. Existing: `ono --agent
/// > --print-host-key` MUST remain accepted in v0.4.1 and MUST print the same identity
/// > fingerprint when the default identity path is used.
fn ono(home: &ono_testkit::Scratch, arguments: &[&str]) -> ono_testkit::Run {
    // `ONO_CONFIG_DIR` so the scratch directory *is* the configuration directory: §8.1 says the
    // identity follows the ordinary resolution, and this is the outermost layer of it.
    ono_testkit::Shell::new()
        .env("HOME", home.path().to_string_lossy().into_owned())
        .env("ONO_CONFIG_DIR", home.path().to_string_lossy().into_owned())
        .args(arguments.iter().map(|argument| (*argument).to_owned()))
        .run()
}

#[test]
fn should_print_the_same_fingerprint_however_it_is_asked_for() {
    let home = scratch();

    let canonical = ono(&home, &["--print-peer-key"]);
    let existing = ono(&home, &["--agent", "--print-host-key"]);

    canonical.assert_success();
    existing.assert_success();
    assert!(
        canonical.stdout().trim().starts_with("sha256:"),
        "§8.5 asks for the fingerprint, which is the non-secret half of the identity, got {:?}",
        canonical.stdout()
    );
    assert_eq!(
        canonical.stdout().trim(),
        existing.stdout().trim(),
        "§8.5: `--agent --print-host-key` MUST print the same identity fingerprint when the \
         default identity path is used, so an operator who learned the old spelling is not \
         quietly given a different machine's key"
    );
}

/// The product-level form of §8.2's exit criterion: a machine that already had a `host_key.pem`
/// prints the fingerprint it always printed, and no second unrelated identity appears.
#[test]
fn should_print_the_identity_a_machine_already_had() {
    let home = scratch();
    let legacy = home.path().join(HOST_KEY_FILE);
    let previous = PeerIdentity::open_or_create(&legacy).expect("the legacy identity is written");
    let expected = previous.fingerprint().to_string();

    let printed = ono(&home, &["--print-peer-key"]);

    printed.assert_success();
    assert_eq!(
        printed.stdout().trim(),
        expected,
        "v0.4.1 §8.2: the identity peers already pinned is reused, not replaced"
    );
    assert!(
        legacy.is_file() && home.path().join(LINK_IDENTITY_FILE).is_file(),
        "the canonical file now exists and the legacy one was not deleted (§8.2 rule 4)"
    );
}

/// §59.6 at the product: nothing prints, and the refusal is the same stable code the library
/// gives, because a fingerprint read out of an exposed key file is a fingerprint of a key that is
/// no longer only yours.
#[test]
fn should_refuse_to_print_from_an_identity_file_anyone_can_read() {
    let home = scratch();
    ono(&home, &["--print-peer-key"]).assert_success();
    std::fs::set_permissions(
        home.path().join(LINK_IDENTITY_FILE),
        std::fs::Permissions::from_mode(0o644),
    )
    .expect("the file is made world-readable");

    let refused = ono(&home, &["--print-peer-key"]);

    assert!(
        !refused.status().is_success(),
        "§8.3 refuses to use an exposed identity, and printing its fingerprint is using it"
    );
    assert!(
        !refused.stdout().contains("sha256:"),
        "nothing is printed on the way to the refusal, got {:?}",
        refused.stdout()
    );
    assert!(
        refused.stderr().contains("600"),
        "§59.6: the diagnostic identifies the required permissions, got {:?}",
        refused.stderr()
    );
}

#[test]
fn should_point_new_users_at_the_canonical_spelling_in_the_help() {
    let help = ono_testkit::Shell::new().args(["--help"]).run();

    help.assert_success();
    assert!(
        help.stdout().contains("--print-peer-key"),
        "§8.5: the help text SHOULD direct new users to `--print-peer-key`, got {:?}",
        help.stdout()
    );
}
