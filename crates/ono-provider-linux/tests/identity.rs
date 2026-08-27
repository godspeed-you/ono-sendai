//! What `get user` and `get group` answer (spec §23.6, §28.7).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers shared between the cases below sit outside a `#[test]` function, where a \
              failed precondition should still abort loudly"
)]

mod common;

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{FakeAccounts, drain, find, records};
use ono_provider_api::{Provider, Query, Selector};
use ono_provider_linux::accounts::Accounts;
use ono_provider_linux::{IdentityProvider, NssAccounts, ProcessProvider};
use ono_value::{FieldAccess, Value};

fn fixture() -> Arc<FakeAccounts> {
    Arc::new(
        FakeAccounts::new()
            .with_user(0, 0, "root")
            .with_user(1000, 1000, "ada")
            .with_group(0, "root", &[])
            .with_group(1000, "ada", &["bob", "carol"]),
    )
}

#[tokio::test]
async fn should_report_every_declared_field_of_a_user() {
    let provider = IdentityProvider::over(fixture());
    let collected = drain(
        provider
            .snapshot(&Query::target("user"))
            .expect("a snapshot"),
    )
    .await;
    let records = records(&collected);
    let ada = find(&records, "name", "ada").expect("the fixture declares ada");

    assert_eq!(ada.get("uid"), Some(&Value::Int(1000)));
    assert_eq!(
        ada.get("home"),
        Some(&Value::Path(Arc::from(std::path::Path::new("/home/ada"))))
    );
    assert_eq!(
        ada.get("shell"),
        Some(&Value::Path(Arc::from(std::path::Path::new("/bin/sh"))))
    );
    assert_eq!(ada.get("gecos"), Some(&Value::string("ada the fixture")));
    let group = ada
        .get("primary_group")
        .and_then(|value| value.as_record().ok())
        .expect("the primary group is a group reference");
    assert_eq!(group.get("gid"), Some(&Value::Int(1000)));
    assert_eq!(group.get("name"), Some(&Value::string("ada")));
    assert_eq!(ada.provenance().provider(), "linux.nss");
}

#[tokio::test]
async fn should_report_every_declared_field_of_a_group() {
    let provider = IdentityProvider::over(fixture());
    let collected = drain(
        provider
            .snapshot(&Query::target("group"))
            .expect("a snapshot"),
    )
    .await;
    let records = records(&collected);
    let group = find(&records, "gid", "1000").expect("the fixture declares gid 1000");

    assert_eq!(group.get("name"), Some(&Value::string("ada")));
    assert_eq!(
        group.get("members"),
        Some(&Value::list([Value::string("bob"), Value::string("carol")])),
        "membership is a list of names, because the database stores names"
    );
}

#[tokio::test]
async fn should_resolve_one_account_by_name_without_enumerating() {
    let provider = IdentityProvider::over(fixture());
    let query = Query::target("user").with(Selector::field("name", Value::string("root")));
    let collected = drain(provider.snapshot(&query).expect("a snapshot")).await;
    let records = records(&collected);

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].get("uid"), Some(&Value::Int(0)));
}

#[tokio::test]
async fn should_resolve_one_account_by_numeric_id() {
    let provider = IdentityProvider::over(fixture());
    let query = Query::target("user").with(Selector::field("uid", Value::Int(1000)));
    let collected = drain(provider.snapshot(&query).expect("a snapshot")).await;

    assert_eq!(records(&collected).len(), 1);
    assert_eq!(
        records(&collected)[0].get("name"),
        Some(&Value::string("ada"))
    );
}

#[tokio::test]
async fn should_yield_nothing_rather_than_a_placeholder_when_a_name_resolves_to_no_account() {
    let provider = IdentityProvider::over(fixture());
    let query = Query::target("user").with(Selector::field("name", Value::string("nobody-here")));
    let collected = drain(provider.snapshot(&query).expect("a snapshot")).await;

    assert!(
        records(&collected).is_empty(),
        "an account that does not exist produces no record, never one with invented fields"
    );
}

#[tokio::test]
async fn should_enumerate_the_accounts_the_database_under_a_root_declares() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    fs::create_dir_all(scratch.path().join("etc")).expect("the etc directory");
    fs::write(
        scratch.path().join("etc/passwd"),
        "root:x:0:0:root:/root:/bin/bash\n\
         ada:x:1000:1000:Ada Lovelace,,,:/home/ada:/usr/bin/ono\n",
    )
    .expect("the passwd database");
    fs::write(
        scratch.path().join("etc/group"),
        "root:x:0:\nusers:x:100:ada,bob\n",
    )
    .expect("the group database");

    let accounts = NssAccounts::rooted(scratch.path());
    let users = accounts.users().expect("the database is readable");
    let ada = users
        .iter()
        .find(|user| user.name == "ada")
        .expect("ada is declared");
    assert_eq!(ada.uid, 1000);
    assert_eq!(ada.shell, std::path::Path::new("/usr/bin/ono"));
    assert_eq!(ada.gecos, "Ada Lovelace,,,");

    let groups = accounts.groups().expect("the database is readable");
    let users_group = groups
        .iter()
        .find(|group| group.name == "users")
        .expect("the users group is declared");
    assert_eq!(users_group.gid, 100);
    assert_eq!(users_group.members, ["ada", "bob"]);
}

#[tokio::test]
async fn should_report_the_missing_database_rather_than_an_empty_account_list() {
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let provider = IdentityProvider::over(Arc::new(NssAccounts::rooted(scratch.path())));
    let collected = drain(
        provider
            .snapshot(&Query::target("user"))
            .expect("a snapshot"),
    )
    .await;

    assert!(records(&collected).is_empty());
    assert_eq!(
        collected.errors().first().map(ono_value::ErrorValue::code),
        Some(ono_core::ErrorCode::IoNotFound),
        "an unreadable database is not the same as a machine with no users"
    );
}

#[tokio::test]
async fn should_resolve_a_real_account_through_nss() {
    // Root, not the account running the tests. Every Unix has uid 0 in its account database; the
    // uid a test happens to run under often is not there at all — a container started with
    // `--user 1000` has no passwd entry for it, and the test then failed for a reason that had
    // nothing to do with NSS. What is under test is that a uid the database knows resolves, and
    // round-trips by name.
    let uid = 0;

    let accounts = NssAccounts::new();
    let account = accounts
        .user(uid)
        .await
        .expect("uid 0 resolves through NSS on every Unix");
    assert_eq!(account.uid, uid);
    assert!(!account.name.is_empty());

    let round_trip = accounts
        .user_named(&account.name)
        .await
        .expect("the name resolves back to an account");
    assert_eq!(
        round_trip.uid, uid,
        "resolving by name and by id must agree, or a reference cannot be trusted"
    );

    let group = accounts
        .group(account.gid)
        .await
        .expect("the primary group resolves");
    assert_eq!(group.gid, account.gid);
}

#[tokio::test]
async fn should_keep_the_numeric_identity_when_no_account_resolves_for_a_uid() {
    // An id served by nothing is the everyday case of a container image with a bare
    // `/etc/passwd`, and spec §23.6 requires the number to survive it.
    let nothing = Arc::new(FakeAccounts::new());
    let fixture = common::ProcFixture::new();
    fixture
        .process(77)
        .stat("orphan", common::StatFields::default())
        .status(4242, 4343);

    let provider = ProcessProvider::rooted(fixture.root()).with_accounts(nothing);
    let collected = drain(
        provider
            .snapshot(&Query::target("process"))
            .expect("a snapshot"),
    )
    .await;
    let user = records(&collected)[0]
        .get("user")
        .and_then(|value| value.as_record().ok())
        .cloned()
        .expect("an unresolved id is still a user reference");

    assert_eq!(user.get("uid"), Some(&Value::Int(4242)));
    assert_eq!(
        user.access("name"),
        FieldAccess::Unknown,
        "the name is unknown; the uid is what must not be discarded"
    );
}

#[tokio::test]
async fn should_not_wait_on_lookups_that_will_not_answer_in_time() {
    // A directory server that never answers is the pathological case spec §34 names. A timeout
    // of zero makes every lookup behave like one, without needing a directory server.
    let impatient = Arc::new(NssAccounts::new().with_timeout(Duration::ZERO));
    let provider = ProcessProvider::new().with_accounts(impatient);

    let started = Instant::now();
    let collected = drain(
        provider
            .snapshot(&Query::target("process").limit(50))
            .expect("a snapshot"),
    )
    .await;
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "an enumeration must not wait on lookups that will not answer: took {elapsed:?}"
    );
    let records = records(&collected);
    assert!(!records.is_empty());
    for record in records {
        let user = record
            .get("user")
            .and_then(|value| value.as_record().ok())
            .expect("every process carries a user reference");
        assert!(
            matches!(user.get("uid"), Some(Value::Int(_))),
            "the numeric identity survives however the lookup went (spec §23.6)"
        );
    }
}

#[tokio::test]
async fn should_resolve_a_name_to_a_user_and_a_group_reference() {
    let provider = IdentityProvider::over(fixture());
    let found = provider
        .resolve(&Selector::field("name", Value::string("root")))
        .await
        .expect("root resolves");
    assert_eq!(
        found.len(),
        2,
        "a name can belong to both a user and a group, and both are named"
    );
}

#[test]
fn should_claim_the_user_and_group_targets_with_the_registry_capability_ids() {
    let provider = IdentityProvider::new();
    assert_eq!(provider.targets(), ["user", "group"]);
    let ids: Vec<String> = provider
        .capabilities()
        .iter()
        .map(|capability| capability.id().to_owned())
        .collect();
    assert_eq!(ids, ["user.list", "group.list", "user.manage"]);
}
