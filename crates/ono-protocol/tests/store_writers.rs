//! The two files that hold a trust decision are written with the same care (issue #195).
//!
//! `trusted_hosts` says which machines this shell will link to; `authorized_clients` says which
//! clients its listening agent will serve. Both are key material in the sense that matters — an
//! account that can choose their contents chooses who this machine trusts — and v0.4.1 §9.8 fixes
//! how such a file is replaced: a temporary, created exclusively and owner-only, synced, renamed
//! over the store, then the directory synced. A store is therefore always the old file or the new
//! one, never a prefix of the new one and never a file somebody else chose.
//!
//! Every case here runs against **both** writers through one table, so neither can fall behind
//! the other again. A writer that dies part-way leaves a temporary behind; the cases plant one —
//! partial contents, or a symbolic link pointing somewhere else — and require that the next update
//! neither trusts it nor writes through it. Two updates that start from the same store must both
//! land, because an update that silently undoes another one re-pins a host somebody forgot.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use common::{impostor_key, scratch, server_key};
use ono_protocol::{AuthorizedClient, AuthorizedClients, Fingerprint, HostKey, TrustStore};

/// One of the two stores, driven only through its public API.
#[derive(Clone, Copy, Debug)]
enum Store {
    Trust,
    Authorization,
}

const STORES: [Store; 2] = [Store::Trust, Store::Authorization];

fn key(index: usize) -> HostKey {
    HostKey::new(
        "ed25519",
        format!("key-material-number-{index:012}").into_bytes(),
    )
}

impl Store {
    fn file_name(self) -> &'static str {
        match self {
            Self::Trust => "trusted_hosts",
            Self::Authorization => "authorized_clients",
        }
    }

    /// Records entry `index` in the store at `path`, through a fresh handle — the way a second
    /// `ono` process would.
    fn add(self, path: &Path, index: usize) -> Result<(), ono_value::ErrorValue> {
        match self {
            Self::Trust => TrustStore::open(path)?.pin(&format!("host-{index}"), &key(index)),
            Self::Authorization => ono_protocol::update_store(path, |entries| {
                entries.push(AuthorizedClient::observing(key(index).fingerprint()));
                Ok(((), true))
            }),
        }
    }

    /// The entries the store at `path` holds, as fingerprints, read fresh from the file.
    fn fingerprints(self, path: &Path) -> Vec<Fingerprint> {
        match self {
            Self::Trust => TrustStore::open(path)
                .expect("the store loads")
                .entries()
                .iter()
                .map(ono_protocol::TrustEntry::fingerprint)
                .collect(),
            Self::Authorization => AuthorizedClients::open(path)
                .expect("the store loads")
                .entries()
                .iter()
                .map(AuthorizedClient::fingerprint)
                .collect(),
        }
    }

    fn temporary(self, path: &Path) -> PathBuf {
        path.with_extension("tmp")
    }
}

fn mode(path: &Path) -> u32 {
    std::fs::symlink_metadata(path)
        .expect("the file exists")
        .permissions()
        .mode()
        & 0o777
}

#[test]
fn should_create_the_store_owner_only_whatever_the_umask_allows() {
    for store in STORES {
        let scratch = scratch();
        let path = scratch.path().join(store.file_name());
        store.add(&path, 1).expect("the first entry is written");
        store.add(&path, 2).expect("the second entry is written");

        assert_eq!(
            mode(&path),
            0o600,
            "{store:?}: §9.8 creates the replacement 0600 — an account that can rewrite the \
             store decides who this machine trusts"
        );
    }
}

#[test]
fn should_neither_trust_nor_reuse_a_temporary_an_interrupted_update_left_behind() {
    for store in STORES {
        let scratch = scratch();
        let path = scratch.path().join(store.file_name());
        store.add(&path, 1).expect("the first entry is written");
        let before = std::fs::read(&path).expect("the store exists");

        // An update that died after writing half its temporary.
        let temporary = store.temporary(&path);
        std::fs::write(&temporary, b"# half a store\nhost-9 ed25519 sha256:00").expect("planted");

        assert_eq!(
            std::fs::read(&path).expect("the store is still there"),
            before,
            "{store:?}: a reader sees the old store, never the temporary"
        );
        store.add(&path, 2).expect("the next update succeeds");

        assert_eq!(
            store.fingerprints(&path),
            vec![key(1).fingerprint(), key(2).fingerprint()],
            "{store:?}: the store holds exactly what was written through it, and nothing the \
             dead update left in its temporary"
        );
        assert!(
            !temporary.exists(),
            "{store:?}: no temporary outlives a successful update"
        );
        assert_eq!(
            mode(&path),
            0o600,
            "{store:?}: and the replacement is owner-only"
        );
    }
}

#[test]
fn should_never_write_through_a_temporary_that_is_a_link_to_somewhere_else() {
    for store in STORES {
        let scratch = scratch();
        let path = scratch.path().join(store.file_name());
        let victim = scratch.write("victim", "somebody else's file\n");
        std::os::unix::fs::symlink(&victim, store.temporary(&path)).expect("planted");

        store.add(&path, 1).expect("the update succeeds");

        assert_eq!(
            std::fs::read_to_string(&victim).expect("the victim is still there"),
            "somebody else's file\n",
            "{store:?}: the temporary is created exclusively, so a planted link is never \
             followed and the file it points at is never truncated"
        );
        assert!(
            !std::fs::symlink_metadata(&path)
                .expect("the store exists")
                .file_type()
                .is_symlink(),
            "{store:?}: the store is a file this process created, not the planted link"
        );
        assert_eq!(store.fingerprints(&path), vec![key(1).fingerprint()]);
    }
}

#[test]
fn should_leave_the_previous_store_intact_when_the_temporary_cannot_be_created() {
    for store in STORES {
        let scratch = scratch();
        let directory = scratch.path().join("config");
        std::fs::create_dir(&directory).expect("the directory is created");
        let path = directory.join(store.file_name());
        store.add(&path, 1).expect("the first entry is written");
        let before = std::fs::read(&path).expect("the store exists");

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500))
            .expect("the directory is made read-only");
        let refused = store.add(&path, 2);
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .expect("the directory is made writable again");

        assert!(
            refused.is_err(),
            "{store:?}: an update that could not be made says so"
        );
        assert_eq!(
            std::fs::read(&path).expect("the store is still there"),
            before,
            "{store:?}: §9.8 — a failed update leaves the previous store intact, byte for byte"
        );
    }
}

#[test]
fn should_keep_both_updates_when_two_writers_started_from_the_same_store() {
    for store in STORES {
        let scratch = scratch();
        let path = scratch.path().join(store.file_name());
        let writers: Vec<_> = (0..4)
            .map(|writer| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for entry in 0..12 {
                        store
                            .add(&path, writer * 100 + entry)
                            .expect("every update succeeds");
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().expect("no writer panicked");
        }

        let mut written = store.fingerprints(&path);
        written.sort_by_key(ToString::to_string);
        let mut expected: Vec<Fingerprint> = (0..4)
            .flat_map(|writer| (0..12).map(move |entry| key(writer * 100 + entry).fingerprint()))
            .collect();
        expected.sort_by_key(ToString::to_string);
        assert_eq!(
            written, expected,
            "{store:?}: concurrent updates are serialised, and none of them is lost"
        );
    }
}

// --- the trust store's own handle ---------------------------------------------------------------
//
// A `TrustStore` is opened once and kept, so its handle can be older than the file. An update
// through an old handle must start from the file as it is now, or it rewrites the file as it was.

#[test]
fn should_not_undo_a_pin_another_handle_wrote_since_this_one_was_opened() {
    let scratch = scratch();
    let path = scratch.path().join("trusted_hosts");
    let first = TrustStore::open(&path).expect("an absent store opens empty");
    let second = TrustStore::open(&path).expect("an absent store opens empty");

    first.pin("db.example.com", &server_key()).expect("pinned");
    second
        .pin("web.example.com", &impostor_key())
        .expect("pinned");

    let reopened = TrustStore::open(&path).expect("the store reads back");
    assert_eq!(
        reopened.fingerprint("db.example.com"),
        Some(server_key().fingerprint()),
        "the second handle's update started from the file, so the first pin survives it"
    );
    assert_eq!(
        reopened.fingerprint("web.example.com"),
        Some(impostor_key().fingerprint())
    );
}

#[test]
fn should_not_bring_back_a_host_another_handle_forgot() {
    let scratch = scratch();
    let path = scratch.path().join("trusted_hosts");
    TrustStore::open(&path)
        .expect("opens")
        .pin("db.example.com", &server_key())
        .expect("pinned");
    let forgetting = TrustStore::open(&path).expect("opens");
    let stale = TrustStore::open(&path).expect("opens");

    forgetting.forget("db.example.com").expect("forgotten");
    stale
        .pin("web.example.com", &impostor_key())
        .expect("pinned");

    assert_eq!(
        TrustStore::open(&path)
            .expect("the store reads back")
            .fingerprint("db.example.com"),
        None,
        "a host somebody forgot stays forgotten: an update through an older handle must not \
         quietly trust it again"
    );
}

#[test]
fn should_refuse_to_pin_rather_than_overwrite_a_store_it_cannot_read() {
    let scratch = scratch();
    let path = scratch.path().join("trusted_hosts");
    let store = TrustStore::open(&path).expect("an absent store opens empty");
    store.pin("db.example.com", &server_key()).expect("pinned");
    let before = std::fs::read(&path).expect("the store exists");

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
        .expect("the store is made unreadable");
    let reopened = TrustStore::open(&path);
    let pinned = store.pin("web.example.com", &impostor_key());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .expect("the store is made readable again");

    assert!(
        reopened.is_err(),
        "a store that cannot be read is an error, never an empty store that pins anew"
    );
    assert!(
        pinned.is_err(),
        "an update that cannot read the store it would replace refuses rather than writing \
         a store that has lost every pin it could not see"
    );
    assert_eq!(
        std::fs::read(&path).expect("the store is still there"),
        before,
        "and the store it could not read is left exactly as it was"
    );
}

#[test]
fn should_refuse_an_update_within_a_bound_while_another_account_holds_the_lock() {
    // Review SEC-A: the update lock is taken on the store's directory, which any account that can
    // open the directory can lock too. Holding it must not hang its owner's every update: past a
    // bound the update is refused, naming the path, and nothing is written.
    for store in [Store::Trust, Store::Authorization] {
        let home = scratch();
        let path = home.path().join(store.file_name());
        store.add(&path, 0).expect("the first update lands");
        let held = std::fs::File::open(home.path()).expect("the directory opens");
        held.lock().expect("this test holds the lock");

        let started = std::time::Instant::now();
        let refused = store
            .add(&path, 1)
            .expect_err("an update does not wait for ever on a lock somebody else holds");
        let waited = started.elapsed();
        assert_eq!(
            refused.code(),
            ono_core::ErrorCode::IoBusy,
            "got {refused:?}"
        );
        assert!(
            refused
                .message()
                .contains(&home.path().display().to_string()),
            "the refusal names the directory whose lock is held, got {}",
            refused.message()
        );
        assert!(
            waited < ono_protocol::STORE_LOCK_WAIT * 3,
            "refused within the bound, waited {waited:?}"
        );
        drop(held);
        assert_eq!(
            store.fingerprints(&path).len(),
            1,
            "nothing was written while the lock was held"
        );
    }
}
