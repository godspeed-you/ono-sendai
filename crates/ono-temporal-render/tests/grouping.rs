//! Event density in the full-screen timeline (spec v0.5 §19.4, §19.5, §43.3).
//!
//! §19.4: "Grouping MUST preserve hidden counts and time span", and §19.5 requires a grouped row
//! to be expandable to the individual retained events. §6.3 forbids inventing an appearance or a
//! disappearance, and hiding one inside a group would do the same damage from the other side, so
//! a lifecycle change is never folded away.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_temporal_render::{RenderOptions, timeline_view};
use ono_value::Value;

mod support;
use support::{event, subject};

/// Seven identical observations of one socket, with a process appearing in the middle of them.
fn churn() -> Vec<Value> {
    let mut events = Vec::new();
    for (index, clock) in [
        "12:18:01.000",
        "12:18:02.000",
        "12:18:03.000",
        "12:18:04.000",
    ]
    .into_iter()
    .enumerate()
    {
        events.push(event(
            &format!("e1800000000000000000000{index}"),
            "object.observed",
            clock,
            "socket/8080",
            &[("subject", subject("socket/8080", "socket"))],
        ));
    }
    events.push(event(
        "e18005000000000000000009",
        "object.appeared",
        "12:18:05.000",
        "process/2741",
        &[("subject", subject("process/2741", "process"))],
    ));
    for (index, clock) in ["12:18:06.000", "12:18:07.000", "12:18:08.000"]
        .into_iter()
        .enumerate()
    {
        events.push(event(
            &format!("e180100000000000000000{index}a"),
            "object.observed",
            clock,
            "socket/8080",
            &[("subject", subject("socket/8080", "socket"))],
        ));
    }
    events
}

fn grouped() -> RenderOptions {
    RenderOptions {
        group_repeats: true,
        ..RenderOptions::default()
    }
}

#[test]
fn should_keep_the_count_and_the_span_when_repeated_rows_are_grouped() {
    // §19.4: repeated provider samples that change no canonical state are grouped, and the group
    // still states how many rows it stands for and how long they took.
    let view = support::timeline_record(churn(), Vec::new());
    let lines = timeline_view(&view, 100, &grouped());
    let rendered = lines.join("\n");

    let group = lines
        .iter()
        .find(|line| line.contains("socket/8080") && line.contains("x4"))
        .unwrap_or_else(|| panic!("§19.4: the four samples read as one row, got {rendered}"));
    assert!(
        group.contains("3s") || group.contains("3.00s"),
        "§19.4: the group preserves its time span, got {group:?}"
    );
    assert!(
        lines
            .iter()
            .filter(|line| line.contains("socket/8080"))
            .count()
            < 7,
        "§19.4: a firehose is not what a grouped view draws, got {rendered}"
    );
}

#[test]
fn should_never_group_away_an_object_lifecycle_change() {
    // §19.4 with §6.3: an appearance is the event a reader is looking for, and a group that
    // swallowed it would make the view lie by omission.
    let view = support::timeline_record(churn(), Vec::new());
    let rendered = timeline_view(&view, 100, &grouped()).join("\n");
    assert!(
        rendered.contains("process/2741") && rendered.contains("appeared"),
        "§19.4: the appearance stands on its own row, got {rendered}"
    );
    assert!(
        !rendered.contains("process/2741  x"),
        "§6.3: a lifecycle change is never folded into a count, got {rendered}"
    );
}

#[test]
fn should_draw_every_member_when_the_group_is_expanded() {
    // §19.5: "A grouped event MUST be expandable to individual retained events where they exist."
    let view = support::timeline_record(churn(), Vec::new());
    let options = RenderOptions {
        group_repeats: true,
        expanded: vec!["@e18000000000000000000000".to_owned()],
        ..RenderOptions::default()
    };
    let rendered = timeline_view(&view, 100, &options).join("\n");
    for clock in [
        "12:18:01.000",
        "12:18:02.000",
        "12:18:03.000",
        "12:18:04.000",
    ] {
        assert!(
            rendered.contains(clock),
            "§19.5: the expanded group shows the event at {clock}, got {rendered}"
        );
    }
}

#[test]
fn should_group_nothing_when_grouping_is_off() {
    // The plain rendering of §11.5 is a row per event. Grouping belongs to the dense full-screen
    // view of §19.4 and never happens behind a reader's back.
    let view = support::timeline_record(churn(), Vec::new());
    let rendered = timeline_view(&view, 100, &RenderOptions::default()).join("\n");
    assert!(
        !rendered.contains("x4"),
        "nothing was grouped, so nothing carries a count, got {rendered}"
    );
    assert_eq!(
        rendered
            .lines()
            .filter(|line| line.contains("socket/8080"))
            .count(),
        7,
        "every sample has its own row, got {rendered}"
    );
}

#[test]
fn should_draw_the_producers_rows_when_the_record_carries_groups() {
    // §19.4: grouping is a judgement about events. The query engine makes it once, and the view
    // draws what it decided — a second rule in the renderer would give a reader two answers.
    let view = support::timeline_of(
        churn(),
        Vec::new(),
        &[(
            "groups",
            Value::list(vec![
                support::group_row(
                    "e18000000000000000000000",
                    &[
                        "e18000000000000000000000",
                        "e18000000000000000000001",
                        "e18000000000000000000002",
                        "e18000000000000000000003",
                    ],
                    3,
                    "12:18:01.000",
                    "12:18:04.000",
                    Some("unchanged_sample"),
                ),
                support::group_row(
                    "e18005000000000000000009",
                    &["e18005000000000000000009"],
                    0,
                    "12:18:05.000",
                    "12:18:05.000",
                    None,
                ),
                support::group_row(
                    "e1801000000000000000000a",
                    &[
                        "e1801000000000000000000a",
                        "e1801000000000000000001a",
                        "e1801000000000000000002a",
                    ],
                    2,
                    "12:18:06.000",
                    "12:18:08.000",
                    Some("unchanged_sample"),
                ),
            ]),
        )],
    );
    let lines = timeline_view(&view, 100, &grouped());
    let rendered = lines.join("\n");

    assert!(
        lines
            .iter()
            .any(|line| line.contains("socket/8080") && line.contains("x4")),
        "§19.4: the producer folded four samples into one row, got {rendered}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("socket/8080") && line.contains("x3")),
        "§19.4: and three into the next, got {rendered}"
    );
    assert!(
        rendered.contains("process/2741") && !rendered.contains("process/2741  x"),
        "§6.3: the appearance the producer left alone stays a row of its own, got {rendered}"
    );
}

#[test]
fn should_state_the_hidden_count_the_producer_gave_when_a_provider_had_already_aggregated() {
    // §19.4's fourth dimension is "cluster-level events already aggregated by a provider": the
    // hidden count is then larger than the members the ledger retained, and the renderer must
    // print what the producer said rather than counting the rows it can see.
    let view = support::timeline_of(
        churn(),
        Vec::new(),
        &[(
            "groups",
            Value::list(vec![support::group_row(
                "e18000000000000000000000",
                &["e18000000000000000000000"],
                417,
                "12:18:01.000",
                "12:18:04.000",
                Some("provider_cluster"),
            )]),
        )],
    );
    let rendered = timeline_view(&view, 120, &grouped()).join("\n");
    assert!(
        rendered.contains("x418"),
        "§19.4: the provider's own count is what the row states, got {rendered}"
    );
}

#[test]
fn should_expand_a_producers_group_to_its_members_when_the_view_asked() {
    // §19.5: "A grouped event MUST be expandable to individual retained events where they exist."
    let view = support::timeline_of(
        churn(),
        Vec::new(),
        &[(
            "groups",
            Value::list(vec![support::group_row(
                "e18000000000000000000000",
                &[
                    "e18000000000000000000000",
                    "e18000000000000000000001",
                    "e18000000000000000000002",
                    "e18000000000000000000003",
                ],
                3,
                "12:18:01.000",
                "12:18:04.000",
                Some("unchanged_sample"),
            )]),
        )],
    );
    let options = RenderOptions {
        group_repeats: true,
        expanded: vec!["@e18000000000000000000000".to_owned()],
        ..RenderOptions::default()
    };
    let rendered = timeline_view(&view, 100, &options).join("\n");
    for clock in [
        "12:18:01.000",
        "12:18:02.000",
        "12:18:03.000",
        "12:18:04.000",
    ] {
        assert!(
            rendered.contains(clock),
            "§19.5: the expanded group shows the event at {clock}, got {rendered}"
        );
    }
}

#[test]
fn should_keep_two_objects_apart_when_they_share_a_label() {
    // §19.4 groups "the same object and the same field in a short interval", and grouping keyed
    // on the label folds two processes both called `nginx` into one row — stating a hidden count
    // for a group whose members are two different things. That hides an object rather than a
    // repetition, which §19.4's last sentence forbids.
    let first = support::event(
        "e00000000000000000000001",
        "object.changed",
        "12:00:00.000",
        "nginx",
        &[(
            "subject",
            support::subject_with_id("nginx", "ono:lifetime:1111111111111111"),
        )],
    );
    let second = support::event(
        "e00000000000000000000002",
        "object.changed",
        "12:00:01.000",
        "nginx",
        &[(
            "subject",
            support::subject_with_id("nginx", "ono:lifetime:2222222222222222"),
        )],
    );
    let view = support::timeline_record(vec![first, second], Vec::new());

    let lines = ono_temporal_render::timeline_view(
        &view,
        120,
        &RenderOptions {
            group_repeats: true,
            ..RenderOptions::default()
        },
    );

    let rendered = lines.join("\n");
    assert!(
        !rendered.contains("x2"),
        "two different processes sharing a name are two rows, not one group of two: {rendered}"
    );
}
