//! `trace` (spec §22): known relationships become a graph value that travels the pipeline.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod fixture;

use fixture::{FixtureProvider, providers, run};
use ono_value::Value;

#[tokio::test]
async fn should_answer_a_trace_with_a_graph_rooted_at_the_named_object() {
    let ran = run("trace process 2", &providers(FixtureProvider::new()))
        .await
        .expect("the pipeline runs");

    let record = ran.only().as_record().expect("one ono.graph/1 record");
    assert_eq!(record.schema_id().to_string(), "ono.graph/1");

    let root = record.get("root").expect("a root reference");
    let label = root
        .as_map()
        .expect("a reference map")
        .get("label")
        .and_then(|label| label.as_str().ok())
        .expect("a label");
    assert!(
        label.contains("beta") || label.contains('2'),
        "the root names the traced object, got {label:?}"
    );

    let nodes = record
        .get("nodes")
        .expect("nodes")
        .as_list()
        .expect("a list");
    assert!(
        !nodes.is_empty(),
        "the graph holds at least the object itself (spec §22.1)"
    );
}

#[tokio::test]
async fn should_report_a_trace_of_nothing_rather_than_an_empty_graph() {
    let error = run("trace process 99", &providers(FixtureProvider::new()))
        .await
        .expect_err("process 99 does not exist in the fixture");
    assert_eq!(error.code(), ono_core::ErrorCode::ResolveTargetNotFound);
    assert!(
        error.message().contains("99"),
        "the refusal names what was asked for: {}",
        error.message()
    );
    let _ = Value::Null;
}

// --- an unidentifiable subject is skipped, not fatal (spec §22.3, §27.3, §35.3) ---------------

/// A provider answering `connection` with two sockets: a `time-wait` one whose inode the kernel
/// has already released, and a live one. This is what the kernel actually offers — spec §35.3
/// forbids inventing an inode for the first — and the order is the order `ss` reports.
#[derive(Debug)]
struct SocketFixture;

fn socket_schema() -> std::sync::Arc<ono_value::Schema> {
    ono_value::builtin_schemas()
        .get(&ono_value::SchemaId::new("ono.socket", 1))
        .expect("`ono.socket/1` is built in")
}

fn socket_record(inode: Option<i128>, port: u16) -> ono_value::RecordValue {
    let schema = socket_schema();
    let endpoint = |port: u16| {
        let mut map = ono_value::MapValue::new();
        map.insert("address".into(), Value::Ip("127.0.0.1".parse().unwrap()));
        map.insert("port".into(), Value::Port(port));
        map.insert("path".into(), Value::Null);
        map.insert("host".into(), Value::Null);
        Value::Map(std::sync::Arc::new(map))
    };
    let provenance =
        ono_value::Provenance::local("test.sockets", schema.id().clone()).from_source("memory");
    let mut builder = ono_value::RecordValue::builder(schema, provenance);
    for (field, value) in [
        ("protocol", Value::string("tcp")),
        ("family", Value::string("inet")),
        ("local", endpoint(port)),
        ("remote", endpoint(9999)),
        (
            "state",
            Value::string(if inode.is_some() {
                "established"
            } else {
                "time-wait"
            }),
        ),
        ("process", Value::Null),
        ("user", Value::Null),
        ("inode", inode.map_or(Value::Null, Value::Int)),
    ] {
        builder = builder.set(field, value).expect("the socket fields exist");
    }
    builder.build()
}

#[async_trait::async_trait]
impl ono_provider_api::Provider for SocketFixture {
    fn id(&self) -> &str {
        "test.sockets"
    }

    fn targets(&self) -> &[&str] {
        &["connection", "socket"]
    }

    fn schemas(&self) -> Vec<std::sync::Arc<ono_value::Schema>> {
        vec![socket_schema()]
    }

    fn capabilities(&self) -> Vec<ono_provider_api::Capability> {
        vec![
            ono_provider_api::Capability::new("connection.list", ono_provider_api::Risk::Read),
            ono_provider_api::Capability::new("socket.trace", ono_provider_api::Risk::Read),
        ]
    }

    fn snapshot(
        &self,
        _query: &ono_provider_api::Query,
    ) -> Result<ono_pipeline::ValueStream, ono_value::ErrorValue> {
        let records = vec![
            Value::Record(std::sync::Arc::new(socket_record(None, 4001))),
            Value::Record(std::sync::Arc::new(socket_record(Some(4242), 4002))),
        ];
        Ok(ono_pipeline::ValueStream::spawn(
            ono_pipeline::PipelineConfig::new(),
            ono_pipeline::Boundedness::Bounded,
            move |sink| async move {
                for record in records {
                    if sink.send(record).await.is_err() {
                        return;
                    }
                }
            },
        ))
    }

    async fn resolve(
        &self,
        _selector: &ono_provider_api::Selector,
    ) -> Result<Vec<ono_provider_api::ObjectRef>, ono_value::ErrorValue> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn should_trace_the_first_identifiable_connection_when_an_earlier_one_has_no_identity() {
    let mut registry = ono_provider_api::ProviderRegistry::new();
    registry.register(std::sync::Arc::new(SocketFixture));

    let ran = run("trace connection", &registry)
        .await
        .expect("a time-wait socket ahead of a live one does not make the whole trace impossible");

    let record = ran.only().as_record().expect("one ono.graph/1 record");
    let root = record.get("root").expect("a root reference");
    let label = root
        .as_map()
        .expect("a reference map")
        .get("label")
        .and_then(|label| label.as_str().ok())
        .expect("a label");
    assert!(
        label.contains("4242"),
        "spec §27.3: a record whose identity field is null is a value, not an object, so the \
         trace roots at the next socket that is one — the one carrying inode 4242, got {label:?}"
    );
}

#[tokio::test]
async fn should_refuse_a_trace_when_no_candidate_carries_an_identity() {
    #[derive(Debug)]
    struct OnlyUnidentifiable;

    #[async_trait::async_trait]
    impl ono_provider_api::Provider for OnlyUnidentifiable {
        fn id(&self) -> &str {
            "test.sockets"
        }
        fn targets(&self) -> &[&str] {
            &["connection", "socket"]
        }
        fn schemas(&self) -> Vec<std::sync::Arc<ono_value::Schema>> {
            vec![socket_schema()]
        }
        fn capabilities(&self) -> Vec<ono_provider_api::Capability> {
            vec![ono_provider_api::Capability::new(
                "socket.trace",
                ono_provider_api::Risk::Read,
            )]
        }
        fn snapshot(
            &self,
            _query: &ono_provider_api::Query,
        ) -> Result<ono_pipeline::ValueStream, ono_value::ErrorValue> {
            let records = vec![Value::Record(std::sync::Arc::new(socket_record(
                None, 4001,
            )))];
            Ok(ono_pipeline::ValueStream::spawn(
                ono_pipeline::PipelineConfig::new(),
                ono_pipeline::Boundedness::Bounded,
                move |sink| async move {
                    for record in records {
                        if sink.send(record).await.is_err() {
                            return;
                        }
                    }
                },
            ))
        }
        async fn resolve(
            &self,
            _selector: &ono_provider_api::Selector,
        ) -> Result<Vec<ono_provider_api::ObjectRef>, ono_value::ErrorValue> {
            Ok(Vec::new())
        }
    }

    let mut registry = ono_provider_api::ProviderRegistry::new();
    registry.register(std::sync::Arc::new(OnlyUnidentifiable));

    let error = run("trace connection", &registry)
        .await
        .expect_err("nothing here can be a node");
    assert_eq!(error.code(), ono_core::ErrorCode::TypeMismatch);
}
