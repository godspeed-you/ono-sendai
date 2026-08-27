//! The ASCII rendering of spec §22.4, through the tree renderer the shell already has.

mod common;

use std::sync::Arc;

use common::{
    FixtureProvider, ProcFixture, StatedRelationships, endpoint, file, node, process, registry,
    service, socket, trace_with,
};
use ono_graph::{
    OpenFiles, ProcessSockets, RelationshipProvider, ServiceProcesses, TraceOptions, Tracer,
};
use ono_provider_api::Provider;
use ono_render::Layout;

fn drawn(graph: &ono_graph::Graph) -> Vec<String> {
    graph
        .trees()
        .iter()
        .flat_map(|tree| Layout::new(120).render_tree(tree))
        .collect()
}

#[tokio::test]
async fn should_draw_the_tree_of_the_specification() {
    let proc = ProcFixture::new();
    proc.process(921)
        .fd(3, "/etc/nginx/nginx.conf", 0)
        .fd(4, "/var/log/nginx/access.log", 1)
        .fd(5, "socket:[8323]", 2)
        .cgroup("0::/system.slice/nginx.service");
    let objects: Vec<Arc<dyn Provider>> = vec![
        Arc::new(FixtureProvider::new(
            "fixture.process",
            &["process"],
            vec![process(921, Some(1), "nginx")],
        )),
        Arc::new(FixtureProvider::new(
            "fixture.socket",
            &["socket"],
            vec![socket(
                8323,
                "tcp",
                endpoint(Some("0.0.0.0"), Some(443)),
                endpoint(None, None),
                "listen",
            )],
        )),
        Arc::new(FixtureProvider::new(
            "fixture.file",
            &["file"],
            vec![
                file("/etc/nginx/nginx.conf", "file", 2049, 17),
                file("/var/log/nginx/access.log", "file", 2049, 18),
            ],
        )),
    ];
    let registry = registry(objects);
    let target = service("network-online.target", None);
    let unit = service("nginx.service", Some(921));

    let providers: Vec<Arc<dyn RelationshipProvider>> = vec![
        Arc::new(ServiceProcesses::new(Arc::clone(&registry)).rooted(proc.root())),
        Arc::new(ProcessSockets::new(Arc::clone(&registry)).rooted(proc.root())),
        Arc::new(OpenFiles::new(Arc::clone(&registry)).rooted(proc.root())),
        Arc::new(
            StatedRelationships::new("fixture.units")
                .about("ono.service/1")
                .exact("nginx.service", "requires", node(&target)),
        ),
    ];
    let graph = trace_with(providers, node(&unit), TraceOptions::new().depth(2)).await;

    assert_eq!(
        drawn(&graph),
        [
            "nginx.service",
            "+-- owns -> process/921 nginx",
            "|   +-- listens -> tcp/:443",
            "|   +-- reads -> /etc/nginx/nginx.conf",
            "|   +-- writes -> /var/log/nginx/access.log",
            "+-- requires -> network-online.target",
        ],
        "spec §22.4 draws exactly this"
    );
}

#[tokio::test]
async fn should_draw_an_inferred_edge_differently_from_an_observed_one() {
    let host = socket(
        9002,
        "tcp",
        endpoint(Some("10.4.2.9"), Some(52345)),
        endpoint(None, None),
        "established",
    );
    let root = process(812, Some(1), "postgres");
    let provider = StatedRelationships::new("fixture.mixed")
        .about("ono.process/1")
        .inferred("process/812 postgres", "talks-to", node(&host));

    let graph = trace_with(
        vec![Arc::new(provider)],
        node(&root),
        TraceOptions::new().depth(1),
    )
    .await;

    let lines = drawn(&graph);
    assert!(
        lines.iter().any(|line| line.starts_with("+~~ talks-to ->")),
        "an inference must not be drawn as an observation (spec §22.2): {lines:?}"
    );
}

#[tokio::test]
async fn should_draw_a_repeated_object_once_and_mark_the_repetition() {
    let (root, child) = (process(1, Some(0), "root"), process(2, Some(1), "child"));
    let provider = StatedRelationships::new("fixture.cycle")
        .about("ono.process/1")
        .exact("process/1 root", "child", node(&child))
        .exact("process/2 child", "parent", node(&root));

    let graph = trace_with(
        vec![Arc::new(provider)],
        node(&root),
        TraceOptions::new().depth(8),
    )
    .await;

    assert_eq!(
        drawn(&graph),
        [
            "process/1 root",
            "+-- child -> process/2 child",
            "|   +-- parent -> process/1 root (already shown)",
        ],
        "a cycle is drawn once, and the repetition says what it is"
    );
}

#[tokio::test]
async fn should_say_in_the_drawing_that_the_walk_was_cut_short() {
    let (root, child, grandchild) = (
        process(1, Some(0), "root"),
        process(2, Some(1), "child"),
        process(3, Some(2), "grandchild"),
    );
    let provider = StatedRelationships::new("fixture.chain")
        .about("ono.process/1")
        .exact("process/1 root", "child", node(&child))
        .exact("process/2 child", "child", node(&grandchild));

    let graph = trace_with(
        vec![Arc::new(provider)],
        node(&root),
        TraceOptions::new().depth(1),
    )
    .await;

    let lines = drawn(&graph);
    assert!(
        lines.iter().any(|line| line.contains("truncated")),
        "a trace that stopped early says so where the user is looking: {lines:?}"
    );
}

#[tokio::test]
async fn should_show_the_failure_beside_the_object_it_concerns() {
    let proc = ProcFixture::new();
    proc.process(921).unreadable_fds();
    let registry = registry(vec![Arc::new(FixtureProvider::new(
        "fixture.file",
        &["file"],
        Vec::new(),
    ))]);
    let root = process(921, Some(1), "nginx");

    let tracer = Tracer::new()
        .with(Arc::new(
            OpenFiles::new(Arc::clone(&registry)).rooted(proc.root()),
        ))
        .with_options(TraceOptions::new().depth(1));
    let graph = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tracer.trace([node(&root)]),
    )
    .await
    .expect("the trace finished");
    common::make_readable(proc.root());

    let lines = drawn(&graph);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("io.permission_denied")),
        "what could not be read is shown, not omitted: {lines:?}"
    );
}
