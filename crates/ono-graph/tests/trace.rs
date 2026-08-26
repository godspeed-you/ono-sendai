//! The walk itself: identity, termination and the bounds spec §22.3 needs a trace to respect.

mod common;

use std::sync::Arc;

use common::{FixtureProvider, StatedRelationships, edges, node, process, registry, trace_with};
use ono_core::ErrorCode;
use ono_graph::{RelationshipProvider, TraceOptions, Tracer, roots};
use ono_provider_api::{Provider, Query, Selector};
use ono_value::Value;

/// `a`, `b`, `c` … as processes, so a test about the shape of a walk is not also a test about
/// procfs.
fn letter(pid: i64, name: &str) -> ono_value::RecordValue {
    process(pid, Some(1), name)
}

#[tokio::test]
async fn should_reach_one_object_once_when_two_paths_lead_to_it() {
    let (root, left, right, shared) = (
        letter(1, "root"),
        letter(2, "left"),
        letter(3, "right"),
        letter(4, "shared"),
    );
    let provider = StatedRelationships::new("fixture.shape")
        .about("ono.process/1")
        .exact("process/1 root", "child", node(&left))
        .exact("process/1 root", "child", node(&right))
        .exact("process/2 left", "child", node(&shared))
        .exact("process/3 right", "child", node(&shared));

    let graph = trace_with(
        vec![Arc::new(provider)],
        node(&root),
        TraceOptions::new().depth(4),
    )
    .await;

    assert_eq!(
        graph.nodes().len(),
        4,
        "the same object reached twice is one node, because a node is keyed by object identity"
    );
    assert_eq!(
        graph.edges().len(),
        4,
        "both paths to it remain visible as separate edges"
    );
}

#[tokio::test]
async fn should_finish_when_the_relationships_form_a_cycle() {
    let (first, second) = (letter(1, "first"), letter(2, "second"));
    let provider = StatedRelationships::new("fixture.cycle")
        .about("ono.process/1")
        .exact("process/1 first", "child", node(&second))
        .exact("process/2 second", "child", node(&first));

    let graph = trace_with(
        vec![Arc::new(provider)],
        node(&first),
        TraceOptions::new().depth(64),
    )
    .await;

    assert_eq!(graph.nodes().len(), 2);
    assert_eq!(graph.edges().len(), 2, "the cycle is data, not a hang");
    assert!(
        !graph.truncation().is_truncated(),
        "a walk that closed a cycle finished; nothing was left out"
    );
}

#[tokio::test]
async fn should_stop_at_the_depth_limit_and_say_that_it_did() {
    let (first, second, third, fourth) = (
        letter(1, "first"),
        letter(2, "second"),
        letter(3, "third"),
        letter(4, "fourth"),
    );
    let provider = StatedRelationships::new("fixture.chain")
        .about("ono.process/1")
        .exact("process/1 first", "child", node(&second))
        .exact("process/2 second", "child", node(&third))
        .exact("process/3 third", "child", node(&fourth));

    let graph = trace_with(
        vec![Arc::new(provider)],
        node(&first),
        TraceOptions::new().depth(2),
    )
    .await;

    assert_eq!(
        graph.nodes().len(),
        3,
        "two hops from the root, and no further"
    );
    let truncation = graph.truncation();
    assert!(truncation.is_truncated());
    assert_eq!(truncation.depth_limit(), Some(2));
    assert_eq!(
        truncation.unexpanded(),
        1,
        "the object that was reached but not expanded is counted"
    );
    assert!(
        truncation
            .message()
            .is_some_and(|message| message.contains("depth")),
        "a trace that stopped must say so"
    );
}

#[tokio::test]
async fn should_stop_at_the_node_limit_and_say_that_it_did() {
    let root = letter(1, "root");
    let children: Vec<ono_value::RecordValue> = (2..=6).map(|pid| letter(pid, "child")).collect();
    let mut provider = StatedRelationships::new("fixture.wide").about("ono.process/1");
    for child in &children {
        provider = provider.exact("process/1 root", "child", node(child));
    }

    let graph = trace_with(
        vec![Arc::new(provider)],
        node(&root),
        TraceOptions::new().depth(4).max_nodes(3),
    )
    .await;

    assert_eq!(graph.nodes().len(), 3, "the root and two of its children");
    let truncation = graph.truncation();
    assert_eq!(truncation.node_limit(), Some(3));
    assert!(
        truncation
            .message()
            .is_some_and(|message| message.contains("3")),
        "the limit that stopped the walk is named in what the user is told"
    );
}

#[tokio::test]
async fn should_follow_only_the_relations_the_caller_asked_for() {
    let (root, parent, child) = (letter(1, "root"), letter(2, "parent"), letter(3, "child"));
    let provider = StatedRelationships::new("fixture.filter")
        .about("ono.process/1")
        .exact("process/1 root", "parent", node(&parent))
        .exact("process/1 root", "child", node(&child));

    let graph = trace_with(
        vec![Arc::new(provider)],
        node(&root),
        TraceOptions::new().depth(2).only_relations(["child"]),
    )
    .await;

    assert_eq!(edges(&graph), ["child -> process/3 child"]);
}

#[tokio::test]
async fn should_start_from_every_object_a_query_resolves() {
    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.process",
        &["process"],
        vec![
            process(812, Some(1), "postgres"),
            process(4419, Some(1), "rustc"),
        ],
    ))];
    let registry = registry(providers);

    let found = roots(
        &registry,
        &Query::target("process").with(Selector::field("pid", Value::Int(812))),
    )
    .await
    .expect("the process exists");

    assert_eq!(
        found.iter().map(ono_graph::Node::label).collect::<Vec<_>>(),
        ["process/812 postgres"]
    );
}

#[tokio::test]
async fn should_refuse_to_trace_an_object_that_does_not_exist() {
    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.process",
        &["process"],
        vec![process(812, Some(1), "postgres")],
    ))];
    let registry = registry(providers);

    let error = roots(
        &registry,
        &Query::target("process").with(Selector::field("pid", Value::Int(9999))),
    )
    .await
    .expect_err("nothing matched, and an empty graph would say the object has no relationships");

    assert_eq!(error.code(), ErrorCode::ResolveTargetNotFound);
}

#[tokio::test]
async fn should_produce_an_empty_graph_when_no_provider_knows_the_object() {
    let root = letter(1, "lonely");
    let tracer = Tracer::new();

    let graph = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tracer.trace([node(&root)]),
    )
    .await
    .expect("a trace with no providers still finishes");

    assert_eq!(graph.nodes().len(), 1, "the object itself is still a node");
    assert!(graph.edges().is_empty());
}

#[tokio::test]
async fn should_ask_only_the_providers_that_answer_about_the_object() {
    let root = letter(1, "root");
    let other = letter(2, "other");
    let elsewhere: Arc<dyn RelationshipProvider> = Arc::new(
        StatedRelationships::new("fixture.sockets")
            .about("ono.socket/1")
            .exact("process/1 root", "listens", node(&other)),
    );

    let graph = trace_with(vec![elsewhere], node(&root), TraceOptions::new()).await;

    assert!(
        graph.edges().is_empty(),
        "a provider that answers about sockets is not asked about a process"
    );
}
