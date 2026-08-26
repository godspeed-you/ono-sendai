//! The graph as a value: it travels a pipeline like any other value and serializes without a
//! renderer (spec §22.1, §13.6).

mod common;

use std::sync::Arc;

use common::{FixtureProvider, ProcFixture, endpoint, node, process, registry, socket, trace_with};
use ono_graph::{ProcessSockets, TraceOptions};
use ono_pipeline::ValueStream;
use ono_provider_api::Provider;
use ono_value::{SchemaId, Value, builtin_schemas, to_json_string};

async fn nginx_graph() -> ono_graph::Graph {
    let proc = ProcFixture::new();
    proc.process(921).fd(5, "socket:[8323]", 2);
    let sockets: Vec<Arc<dyn Provider>> = vec![Arc::new(FixtureProvider::new(
        "fixture.socket",
        &["socket"],
        vec![socket(
            8323,
            "tcp",
            endpoint(Some("0.0.0.0"), Some(443)),
            endpoint(None, None),
            "listen",
        )],
    ))];
    let registry = registry(sockets);
    let subject = process(921, Some(1), "nginx");
    trace_with(
        vec![Arc::new(
            ProcessSockets::new(Arc::clone(&registry)).rooted(proc.root()),
        )],
        node(&subject),
        TraceOptions::new().depth(1),
    )
    .await
}

#[tokio::test]
async fn should_describe_itself_as_a_record_of_the_graph_contract() {
    let graph = nginx_graph().await;

    let value = graph.to_value().expect("a graph is a value");
    let record = value.as_record().expect("the graph is a record").clone();
    assert_eq!(record.schema_id(), &SchemaId::new("ono.graph", 1));
    builtin_schemas()
        .validate(&record)
        .expect("the graph satisfies its own contract");

    let Some(Value::List(nodes)) = record.get("nodes").cloned() else {
        panic!("a graph carries its nodes as a list");
    };
    assert_eq!(nodes.len(), 2, "the process and the socket it listens on");
    let Some(Value::List(edges)) = record.get("edges").cloned() else {
        panic!("a graph carries its edges as a list");
    };
    let edge = edges
        .first()
        .and_then(|value| value.as_record().ok())
        .expect("one edge")
        .clone();
    assert_eq!(edge.get("relation"), Some(&Value::String("listens".into())));
    assert_eq!(
        edge.get("confidence"),
        Some(&Value::String("exact".into())),
        "spec §22.2 makes confidence part of the edge, never a rendering decision"
    );
    assert_eq!(
        edge.get("direction"),
        Some(&Value::String("directed".into()))
    );
    assert_eq!(
        edge.get("provider"),
        Some(&Value::String("linux.process-sockets".into())),
        "an edge says which provider asserted it"
    );
}

#[tokio::test]
async fn should_travel_a_pipeline_and_survive_being_serialized_as_json() {
    let graph = nginx_graph().await;
    let value = graph.to_value().expect("a graph is a value");

    let collected = ValueStream::from_values([value]).collect().await;

    assert!(collected.errors().is_empty());
    let carried = collected
        .values()
        .first()
        .expect("the graph arrived at the end of the pipeline");
    let json = to_json_string(carried).expect("a graph serializes");
    assert!(
        json.contains("\"relation\":\"listens\"") && json.contains("\"confidence\":\"exact\""),
        "the confidence survives the serialization a script reads: {json}"
    );
}

#[tokio::test]
async fn should_report_a_truncated_walk_in_the_value_as_well_as_in_the_type() {
    let root = process(1, Some(0), "root");
    let child = process(2, Some(1), "child");
    let grandchild = process(3, Some(2), "grandchild");
    let provider = common::StatedRelationships::new("fixture.chain")
        .about("ono.process/1")
        .exact("process/1 root", "child", node(&child))
        .exact("process/2 child", "child", node(&grandchild));

    let graph = trace_with(
        vec![Arc::new(provider)],
        node(&root),
        TraceOptions::new().depth(1),
    )
    .await;

    let value = graph.to_value().expect("a graph is a value");
    let record = value.as_record().expect("the graph is a record").clone();
    let truncation = record
        .extra()
        .get("ono.graph.truncation")
        .and_then(|value| value.as_map().ok())
        .cloned()
        .expect("a truncated walk says so in the value, not only in the rendering");
    assert_eq!(truncation.get("depth_limit"), Some(&Value::Int(1)));
    assert_eq!(truncation.get("unexpanded"), Some(&Value::Int(1)));
}

#[test]
fn should_carry_a_relationship_that_holds_in_neither_direction_as_undirected() {
    let (left, right) = (process(1, Some(0), "left"), process(2, Some(0), "right"));
    let (left, right) = (node(&left), node(&right));
    let mut graph = ono_graph::Graph::new();
    graph.insert_node(left.clone());
    graph.insert_node(right.clone());
    graph.insert_edge(
        ono_graph::Edge::exact(
            left.id().clone(),
            right.id().clone(),
            "shares-namespace",
            "fixture.peers",
        )
        .undirected(),
    );

    let value = graph.to_value().expect("a graph is a value");
    let record = value.as_record().expect("the graph is a record").clone();
    let Some(Value::List(edges)) = record.get("edges").cloned() else {
        panic!("a graph carries its edges as a list");
    };
    let edge = edges
        .first()
        .and_then(|value| value.as_record().ok())
        .expect("one edge")
        .clone();
    assert_eq!(
        edge.get("direction"),
        Some(&Value::String("undirected".into())),
        "spec §22.1 gives an edge a direction, and both of its values have to be reachable"
    );
}
