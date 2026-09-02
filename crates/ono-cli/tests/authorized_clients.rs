//! The `authorized_clients` store: what it holds, and what it refuses to hold
//! (v0.4.1 §9.2, §9.3, §9.5, §9.8, §59.5, §65.2, §65.4).
//!
//! §9.2 is the whole of this suite in two sentences: "malformed non-comment lines MUST fail
//! loading of the store. A malformed authorization store MUST NOT be treated as empty and MUST
//! NOT cause the agent to fall back to permissive access."
//!
//! Those two failure modes have names — §65.2 self-reported authorization, §65.4 fail-open setup
//! — and the way to keep them out is to make the store's *reader* refuse rather than to make
//! every caller remember. So every case here writes a file by hand and asks the shell what it
//! makes of it, which is exactly what an operator does.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::os::unix::fs::PermissionsExt as _;

use ono_testkit::{Scratch, scratch};

mod support;
use support::{last_line, ono_at_home};

/// A fingerprint of the shape the store records, distinguishable by its first digits.
fn fingerprint(marker: char) -> String {
    format!(
        "sha256:{}",
        std::iter::repeat_n(marker, 64).collect::<String>()
    )
}

/// Writes `contents` as this account's authorization store and returns its path.
fn store(home: &Scratch, contents: &str) -> std::path::PathBuf {
    let directory = home.path().join("ono");
    std::fs::create_dir_all(&directory).expect("the configuration directory is creatable");
    let path = directory.join("authorized_clients");
    std::fs::write(&path, contents).expect("the store is writable");
    path
}

// --- §9.3: the entry model ------------------------------------------------------------------

#[test]
fn should_parse_the_documented_entry_model_including_an_empty_action_set() {
    let home = scratch();
    let observer = fingerprint('1');
    let actor = fingerprint('2');
    store(
        &home,
        &format!(
            "# a comment, and a blank line below\n\n\
             {observer} observe=true label=watcher\n\
             {actor} observe=true actions=process.signal,service.manage label=deploy\n"
        ),
    );

    let run = ono_at_home(
        &home,
        "get client-key | select fingerprint label observe actions | to json",
    );
    run.assert_success();

    assert_eq!(
        last_line(&run),
        format!(
            "[{{\"fingerprint\":\"{observer}\",\"label\":\"watcher\",\"observe\":true,\
             \"actions\":[]}},{{\"fingerprint\":\"{actor}\",\"label\":\"deploy\",\
             \"observe\":true,\"actions\":[\"process.signal\",\"service.manage\"]}}]"
        ),
        "§9.3's record is fingerprint, label, observe and an exact action set — and an empty \
         action set is a value, not a missing field, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_reject_an_unknown_field_in_an_authorization_entry() {
    let home = scratch();
    store(
        &home,
        &format!("{} observe=true elevated=true\n", fingerprint('3')),
    );

    let run = ono_at_home(
        &home,
        "try { get client-key } catch e { $e | select code name | to json }",
    );
    run.assert_success();

    let answered = last_line(&run);
    assert!(
        answered.contains("Ono-Sendai-E1204")
            && answered.contains("remote.authorization_store_invalid"),
        "§9.3: an unknown field is rejected rather than ignored — a field this build does not \
         understand may be the one meant to restrict something, got {answered:?}"
    );
}

#[test]
fn should_fail_to_load_the_store_when_one_non_comment_line_is_malformed() {
    let home = scratch();
    let good = fingerprint('4');
    store(
        &home,
        &format!("{good} observe=true\nthis line is not an entry\n"),
    );

    let run = ono_at_home(
        &home,
        "try { get client-key } catch e { $e | select code | to json }",
    );
    run.assert_success();
    assert!(
        last_line(&run).contains("Ono-Sendai-E1204"),
        "§9.2: a malformed non-comment line fails loading of the store, got {:?}",
        run.stdout()
    );

    let message = ono_at_home(
        &home,
        "try { get client-key } catch e { $e | select message }",
    );
    assert!(
        message.stdout().contains("line 2"),
        "the diagnostic names the line to repair, got {:?}",
        message.stdout()
    );
}

#[test]
fn should_never_treat_a_malformed_store_as_an_empty_one() {
    let home = scratch();
    let listed = fingerprint('5');
    store(&home, &format!("{listed} observe=yes\n"));

    // Read as empty, this would answer `[]` and every later command would behave as though the
    // operator had authorized nobody — which reads as safe and is the doorway to §65.4's
    // fail-open: the *next* thing a fail-open reader does is fall back to a default.
    let listing = ono_at_home(
        &home,
        "try { get client-key } catch e { $e | select code | to json }",
    );
    listing.assert_success();
    assert_ne!(
        last_line(&listing),
        "[]",
        "§9.2: a malformed store is not an empty store, got {:?}",
        listing.stdout()
    );
    assert!(last_line(&listing).contains("Ono-Sendai-E1204"));

    // And a mutation does not quietly rewrite the file it could not read, which would destroy
    // the grants an operator wrote and replace them with whatever this command happened to say.
    let added = ono_at_home(
        &home,
        &format!(
            "try {{ add client-key {} }} catch e {{ $e | select code | to json }}",
            fingerprint('6')
        ),
    );
    added.assert_success();
    assert!(
        last_line(&added).contains("Ono-Sendai-E1204"),
        "an unreadable store refuses the write too, got {:?}",
        added.stdout()
    );
    let text = std::fs::read_to_string(home.path().join("ono").join("authorized_clients"))
        .expect("the file is still there");
    assert_eq!(
        text,
        format!("{listed} observe=yes\n"),
        "the operator's file is untouched, byte for byte"
    );
}

#[test]
fn should_distinguish_a_missing_store_from_a_corrupt_one() {
    // Both authorize nobody. They are different conditions and send an operator to different
    // places, so the refusal must not read the same for both (§9.2).
    let absent = scratch();
    let listed = ono_at_home(&absent, "get client-key | to json");
    listed.assert_success();
    assert_eq!(
        last_line(&listed),
        "[]",
        "a store nobody has written yet authorizes nobody, and says so as an empty table"
    );

    let corrupt = scratch();
    store(&corrupt, "not a fingerprint at all\n");
    let refused = ono_at_home(
        &corrupt,
        "try { get client-key } catch e { $e | select code | to json }",
    );
    assert!(
        last_line(&refused).contains("Ono-Sendai-E1204"),
        "a corrupt store is a refusal, where a missing one is an empty answer, got {:?}",
        refused.stdout()
    );
}

// --- §9.5: an action grant is one exact capability id ----------------------------------------

#[test]
fn should_refuse_a_wildcard_or_risk_class_in_an_action_grant() {
    // §9.5: "wildcards MUST NOT be the storage default", and §9.6 forbids an implicit `admin`
    // profile that grows as capabilities are added. The type has no room for either, so each of
    // these is refused at the door rather than normalised into something.
    for pattern in [
        "*",
        "process.*",
        "*.signal",
        "process.**",
        "mutate",
        "destructive",
        "process.",
        "",
    ] {
        let home = scratch();
        let client = fingerprint('7');
        store(&home, &format!("{client} observe=true actions={pattern}\n"));

        let run = ono_at_home(
            &home,
            "try { get client-key } catch e { $e | select code | to json }",
        );
        run.assert_success();
        let answered = last_line(&run);
        // An empty `actions=` is the empty allowlist written out, and grants nothing — which is
        // the one spelling in the list that is a value rather than a pattern.
        if pattern.is_empty() {
            assert!(
                answered.contains("\"observe\":true")
                    || answered.contains("[]")
                    || answered == "[]",
                "an empty allowlist is the empty allowlist, got {answered:?}"
            );
            continue;
        }
        assert!(
            answered.contains("Ono-Sendai-E1204"),
            "§9.5: `{pattern}` is not a capability id and must not be storable as a grant, got \
             {answered:?}"
        );
    }
}

#[test]
fn should_refuse_a_wildcard_from_the_command_that_writes_the_store() {
    let home = scratch();
    let client = fingerprint('8');
    ono_at_home(&home, &format!("add client-key {client}")).assert_success();

    let refused = ono_at_home(
        &home,
        &format!(
            "try {{ set client-key {client} --allow process.* }} catch e {{ $e | select code | to json }}"
        ),
    );
    refused.assert_success();
    assert!(
        last_line(&refused).contains("Ono-Sendai-E1204"),
        "the file is not the only door: a pattern cannot be written through the command either, \
         got {:?}",
        refused.stdout()
    );

    let after = ono_at_home(&home, "get client-key | select actions | to json");
    assert_eq!(
        last_line(&after),
        "[{\"actions\":[]}]",
        "and the refused grant left nothing behind"
    );
}

// --- §9.8: atomic updates ---------------------------------------------------------------------

#[test]
fn should_replace_the_store_atomically_so_a_reader_never_sees_a_partial_file() {
    let home = scratch();
    let first = fingerprint('9');
    ono_at_home(&home, &format!("add client-key {first} --label one")).assert_success();

    let directory = home.path().join("ono");
    for index in 0..8 {
        let client = format!("sha256:{:0>64}", format!("{index:x}"));
        ono_at_home(&home, &format!("add client-key {client}")).assert_success();

        // The write goes to a temporary and is renamed over the store, so the store path is
        // never open for writing and no temporary survives the update (§9.8). A reader that
        // opened the path at any moment saw one whole file or the other.
        let leftovers: Vec<String> = std::fs::read_dir(&directory)
            .expect("the configuration directory is readable")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("authorized_clients") && name != "authorized_clients")
            .collect();
        assert!(
            leftovers.is_empty(),
            "an update left {leftovers:?} beside the store; the next one would refuse it"
        );

        let listed = ono_at_home(&home, "get client-key | count | to json");
        listed.assert_success();
        assert_eq!(
            last_line(&listed),
            format!("[{}]", index + 2),
            "every intermediate state of the store is a complete, loadable store"
        );
    }
}

#[test]
fn should_leave_the_previous_store_intact_when_a_write_is_interrupted() {
    let home = scratch();
    let kept = fingerprint('a');
    ono_at_home(&home, &format!("add client-key {kept} --label keeper")).assert_success();

    let directory = home.path().join("ono");
    let before = std::fs::read_to_string(directory.join("authorized_clients"))
        .expect("the store was written");

    // The update cannot create its temporary: the directory is read-only. §9.8 requires that a
    // failed update leave the previous valid store intact, and this is the point in the sequence
    // where a naive implementation would already have truncated it.
    let mut permissions = std::fs::metadata(&directory)
        .expect("the directory exists")
        .permissions();
    permissions.set_mode(0o500);
    std::fs::set_permissions(&directory, permissions).expect("the directory mode is settable");

    let refused = ono_at_home(
        &home,
        &format!(
            "try {{ add client-key {} }} catch e {{ $e | select kind | to json }}",
            fingerprint('b')
        ),
    );

    let mut restore = std::fs::metadata(&directory)
        .expect("the directory exists")
        .permissions();
    restore.set_mode(0o700);
    std::fs::set_permissions(&directory, restore).expect("the directory mode is settable");

    refused.assert_success();
    assert!(
        last_line(&refused).contains("permission") || last_line(&refused).contains("io"),
        "the failure is reported rather than swallowed, got {:?}",
        refused.stdout()
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("authorized_clients"))
            .expect("the previous store is still there"),
        before,
        "§9.8: a failed update leaves the previous valid store intact, byte for byte"
    );
    let listed = ono_at_home(&home, "get client-key | select label | to json");
    assert_eq!(
        last_line(&listed),
        "[{\"label\":\"keeper\"}]",
        "and it still loads, which is what `intact` has to mean"
    );
}

#[test]
fn should_keep_the_owner_only_permissions_of_the_store_across_an_update() {
    let home = scratch();
    ono_at_home(&home, &format!("add client-key {}", fingerprint('c'))).assert_success();
    let path = home.path().join("ono").join("authorized_clients");

    let mode = |path: &std::path::Path| {
        std::fs::metadata(path)
            .expect("the store exists")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(
        mode(&path),
        0o600,
        "the file says who may reach this machine; an account that can rewrite it can authorize \
         itself"
    );

    ono_at_home(&home, &format!("add client-key {}", fingerprint('d'))).assert_success();
    assert_eq!(
        mode(&path),
        0o600,
        "a rename-based update must not inherit the umask of whoever ran the second command"
    );
}

#[test]
fn should_keep_the_store_in_a_file_a_person_can_read_and_edit() {
    let home = scratch();
    let client = fingerprint('e');
    ono_at_home(
        &home,
        &format!("add client-key {client} --label deploy-bot"),
    )
    .assert_success();
    ono_at_home(
        &home,
        &format!("set client-key {client} --allow service.manage"),
    )
    .assert_success();

    let text = std::fs::read_to_string(home.path().join("ono").join("authorized_clients"))
        .expect("the grants are written where the shell says they are");

    assert!(
        text.contains(&format!(
            "{client} observe=true actions=service.manage label=deploy-bot"
        )),
        "§9.2: line-oriented and human-readable, one line per client, got {text:?}"
    );
    let shown = ono_at_home(&home, "get client-key | select path | to json");
    assert!(
        last_line(&shown).contains("authorized_clients"),
        "the table says which file the grants are kept in, got {:?}",
        shown.stdout()
    );
}
