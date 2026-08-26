//! `watch` (spec §18.2): a finite query becomes a stream of updates, and polling is explicit.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod fixture;

use fixture::{FixtureProvider, providers};

// --- watch (spec §18.2, ADR-0024) ------------------------------------------------------------

#[tokio::test]
async fn should_begin_a_watch_with_the_current_state_and_then_the_changes() {
    // ADR-0024: a subscription always begins with a snapshot, and where the provider has no
    // event source the runtime polls — explicitly. The fixture mutates between polls, so the
    // stream carries: three snapshots, then one change, one addition, one removal.
    let provider = FixtureProvider::live();
    let handle = provider.handle();
    let registry = providers(provider);

    let task = tokio::spawn(async move {
        fixture::run("watch process --every 30ms | take 6", &registry).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;
    handle.set_size(1, 999); // beta's size changes
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;
    handle.add(4, "delta", Some(64), "root");
    handle.remove(3); // gamma goes away

    let ran = task
        .await
        .expect("the watch task ran")
        .expect("the pipeline runs");
    let kinds: Vec<String> = ran
        .values()
        .iter()
        .map(|value| {
            value
                .as_record()
                .expect("an event record")
                .get("kind")
                .and_then(|kind| kind.as_str().ok().map(str::to_owned))
                .expect("a kind")
        })
        .collect();

    assert_eq!(
        &kinds[..3],
        ["snapshot", "snapshot", "snapshot"],
        "the stream begins with the current state (ADR-0024), got {kinds:?}"
    );
    assert!(
        kinds[3..].iter().any(|kind| kind == "changed"),
        "a mutation between polls is a `changed` event, got {kinds:?}"
    );

    let events = ran.values();
    let changed = events
        .iter()
        .filter_map(|value| value.as_record().ok())
        .find(|record| {
            record
                .get("kind")
                .and_then(|k| k.as_str().ok().map(str::to_owned))
                == Some("changed".to_owned())
        })
        .expect("one changed event");
    assert_eq!(
        changed
            .get("source")
            .and_then(|source| source.as_str().ok().map(str::to_owned)),
        Some("poll".to_owned()),
        "spec §18.2: polling is explicit, never invisible"
    );
    assert!(
        changed
            .get("changed")
            .and_then(|fields| fields.as_list().ok())
            .is_some_and(|fields| fields.iter().any(|field| {
                field.as_str().ok().map(str::to_owned) == Some("size".to_owned())
            })),
        "the changed event names the fields that moved"
    );
}
