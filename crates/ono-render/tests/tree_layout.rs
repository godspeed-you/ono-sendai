//! The ASCII tree renderer of spec §22.4, which is the default view for anything with a shape:
//! a process tree, a `trace` result, a nested record.
//!
//! Spec §22.2 is the requirement that makes this more than decoration: "The UI must not visually
//! imply certainty that the provider does not possess." An inferred edge must look different
//! from an observed one, and must still look different with colour switched off.

#![allow(
    clippy::panic,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use ono_render::{Confidence, Layout, TreeNode};

fn nginx_trace() -> TreeNode {
    TreeNode::new("nginx.service")
        .with_child(
            TreeNode::new("process/921 nginx")
                .relation("owns")
                .with_child(TreeNode::new("tcp/:443").relation("listens"))
                .with_child(TreeNode::new("/etc/nginx/nginx.conf").relation("reads"))
                .with_child(TreeNode::new("/var/log/nginx/access.log").relation("writes")),
        )
        .with_child(TreeNode::new("network-online.target").relation("requires"))
}

#[test]
fn should_draw_the_shape_the_specification_shows_when_rendering_a_trace() {
    let lines = Layout::new(80).render_tree(&nginx_trace());
    let drawn = lines.join("\n");
    assert_eq!(
        drawn,
        "nginx.service\n\
         +-- owns -> process/921 nginx\n\
         |   +-- listens -> tcp/:443\n\
         |   +-- reads -> /etc/nginx/nginx.conf\n\
         |   +-- writes -> /var/log/nginx/access.log\n\
         +-- requires -> network-online.target",
        "got:\n{drawn}"
    );
}

#[test]
fn should_use_only_ascii_so_the_tree_is_readable_everywhere_when_rendered() {
    // Spec §22.4: "A text renderer should produce useful output everywhere."
    for line in Layout::new(80).render_tree(&nginx_trace()) {
        assert!(line.is_ascii(), "non-ASCII in {line:?}");
    }
}

#[test]
fn should_keep_a_deep_tree_inside_the_terminal_when_rendered_narrow() {
    let mut node = TreeNode::new("leaf");
    for depth in 0..30 {
        node = TreeNode::new(format!("node-{depth}"))
            .relation("contains")
            .with_child(node);
    }
    for width in [20usize, 40, 80] {
        for line in Layout::new(width).render_tree(&node) {
            assert!(
                unicode_width::UnicodeWidthStr::width(line.as_str()) <= width,
                "a {width}-column terminal received {line:?}"
            );
        }
    }
}

#[test]
fn should_mark_an_inferred_relationship_so_it_is_not_mistaken_for_an_observed_one() {
    // Spec §22.2. The mark is textual, so it survives a pipe and a monochrome terminal.
    let tree = TreeNode::new("payments.service")
        .with_child(TreeNode::new("process/921").relation("owns"))
        .with_child(
            TreeNode::new("ledger.service")
                .relation("talks-to")
                .confidence(Confidence::Inferred),
        );
    let lines = Layout::new(80).render_tree(&tree);
    let observed = lines
        .iter()
        .find(|line| line.contains("owns"))
        .expect("observed edge");
    let inferred = lines
        .iter()
        .find(|line| line.contains("talks-to"))
        .expect("inferred edge");
    assert_ne!(
        observed.replace("owns", "").replace("process/921", ""),
        inferred
            .replace("talks-to", "")
            .replace("ledger.service", ""),
        "an inferred edge must be drawn differently:\n{observed}\n{inferred}"
    );
    assert!(
        inferred.contains('~'),
        "the inferred marker must be visible without colour, got {inferred:?}"
    );
}

#[test]
fn should_render_a_lone_node_as_one_line_when_it_has_no_relationships() {
    assert_eq!(
        Layout::new(80).render_tree(&TreeNode::new("process/1 systemd")),
        vec!["process/1 systemd".to_owned()]
    );
}

#[test]
fn should_render_identically_for_the_same_tree_and_width_when_rendered_twice() {
    let tree = nginx_trace();
    assert_eq!(
        Layout::new(80).render_tree(&tree),
        Layout::new(80).render_tree(&tree)
    );
}

#[test]
fn should_stop_descending_and_say_so_rather_than_recurse_without_end_when_a_depth_limit_is_set() {
    let lines = Layout::new(80).max_depth(1).render_tree(&nginx_trace());
    let drawn = lines.join("\n");
    assert!(drawn.contains("process/921 nginx"), "got:\n{drawn}");
    assert!(
        !drawn.contains("tcp/:443"),
        "depth 2 must not be drawn:\n{drawn}"
    );
    assert!(
        drawn.contains("3 more"),
        "what was left out must be stated, got:\n{drawn}"
    );
}
