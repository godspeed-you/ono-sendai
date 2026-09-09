//! §9.5's blast radius and §9.6's unknown boundary.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_render::{Charset, blast_radius, boundaries, impact_block};

mod support;
use support::{bounded_impact, contains, empty_impact, i, index_of, map, nginx_impact, record, s};

#[test]
fn should_count_the_blast_radius_the_graph_holds() {
    let lines = blast_radius(&nginx_impact(), 80, Charset::Ascii);
    for count in [
        "2 direct targets",
        "4 direct dependents",
        "1 known transitive relation",
        "1 external boundary",
    ] {
        assert!(
            contains(&lines, count),
            "§9.5's summary counts what §9.2 classified, and `{count}` is one of those counts"
        );
    }
}

#[test]
fn should_agree_in_singular_and_plural_with_what_it_counted() {
    let graph = record(
        "ono.impact-graph",
        &[
            ("direct_targets", i(1)),
            ("boundary_count", i(0)),
            ("complete", ono_value::Value::Bool(true)),
        ],
    );
    let lines = blast_radius(&graph, 80, Charset::Ascii);
    assert!(
        contains(&lines, "1 direct target") && !contains(&lines, "1 direct targets"),
        "§9.5's example says `1 direct target`, and a count that cannot count reads as a bug"
    );
}

#[test]
fn should_say_when_a_bounded_traversal_stopped_at_its_budget() {
    let lines = blast_radius(&bounded_impact(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "traversal stopped early"),
        "§9.5 and §52.2: a graph cut short by a budget is not a graph that ended"
    );
    assert!(
        contains(&lines, "the interactive budget was reached"),
        "§9.6: the reason travels, because the operator decides what to do about it"
    );
}

#[test]
fn should_say_that_the_counts_of_a_bounded_traversal_are_a_lower_bound() {
    let lines = blast_radius(&bounded_impact(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "lower bound"),
        "§9.5: a summary that reads as a whole graph when it is a bounded one is a lie"
    );
}

#[test]
fn should_never_render_a_bounded_traversal_the_way_it_renders_a_complete_one() {
    let complete = blast_radius(&nginx_impact(), 80, Charset::Ascii);
    let bounded = blast_radius(&bounded_impact(), 80, Charset::Ascii);
    assert_ne!(
        complete, bounded,
        "§9.5: the two graphs say different things and the view may not spell them the same way"
    );
    assert!(
        !contains(&complete, "lower bound"),
        "a complete graph carries no bound, and claiming one would be as wrong as hiding one"
    );
}

#[test]
fn should_draw_the_boundary_in_the_shape_section_nine_six_writes() {
    let lines = boundaries(&nginx_impact(), 80, Charset::Ascii);
    assert_eq!(
        lines.len(),
        3,
        "§9.6's shape is the reach, the boundary under it, and why Ono cannot follow"
    );
    assert!(
        lines[0].contains("nginx") && lines[0].contains("->") && lines[0].contains("external API"),
        "§9.6: the last object Ono can see, the reach, and what lies past it"
    );
    assert!(
        lines[1].contains("? beyond Ono visibility"),
        "§9.6: when the graph ends at an opaque boundary, that boundary MUST be visible"
    );
}

#[test]
fn should_put_the_boundary_mark_under_what_lies_beyond_it() {
    let lines = boundaries(&nginx_impact(), 80, Charset::Ascii);
    let reach = lines[0]
        .find("external API")
        .expect("the far side is drawn");
    let mark = lines[1].find('?').expect("the unknown mark is drawn");
    assert_eq!(
        reach, mark,
        "§9.6's shape puts the mark under the half of the line the evidence does not cover"
    );
}

#[test]
fn should_carry_the_reason_the_graph_could_not_follow_the_edge() {
    let lines = boundaries(&nginx_impact(), 80, Charset::Ascii);
    assert!(
        lines[2].contains("leaves this machine"),
        "§9.6 and §35.1: an operator who knows why the graph stopped can decide what it means"
    );
}

#[test]
fn should_render_nothing_for_a_graph_with_no_boundary() {
    let lines = boundaries(&empty_impact(), 80, Charset::Ascii);
    assert!(
        lines.is_empty(),
        "§9.6 asks for a boundary to be visible, never for one to be invented"
    );
}

#[test]
fn should_keep_object_access_underneath_the_summary() {
    let lines = impact_block(&nginx_impact(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "worker 1") && contains(&lines, ":443"),
        "§9.5: `impact` MAY summarize counts, but MUST preserve object access"
    );
    let summary = index_of(&lines, "blast radius").expect("the summary comes first");
    let object = index_of(&lines, "worker 1").expect("the objects follow");
    assert!(
        summary < object,
        "§9.5's counts stand above the objects they count"
    );
}

#[test]
fn should_keep_the_provenance_of_a_relation_that_reached_a_node() {
    let lines = impact_block(&nginx_impact(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "service.controls_process"),
        "§3.5: impact MUST retain provenance, and a relation nobody can name is not provenance"
    );
    assert!(
        contains(&lines, "exact"),
        "§3.5: the confidence a v0.4 edge carried travels into the view unchanged"
    );
}

#[test]
fn should_say_no_object_was_reached_rather_than_printing_an_empty_summary() {
    let lines = blast_radius(&empty_impact(), 80, Charset::Ascii);
    assert!(
        contains(&lines, "no object was reached"),
        "§10.5: an empty block and a plan that reaches nothing must not look the same"
    );
}

#[test]
fn should_read_the_boundary_count_the_graph_recorded_rather_than_counting_the_list() {
    let graph = record(
        "ono.impact-graph",
        &[
            ("direct_targets", i(1)),
            ("boundary_count", i(2)),
            (
                "boundaries",
                ono_value::Value::list([map(&[
                    ("at", s("nginx")),
                    ("beyond", s("external API")),
                    ("reason", s("it leaves this machine")),
                ])]),
            ),
            ("complete", ono_value::Value::Bool(true)),
        ],
    );
    let lines = blast_radius(&graph, 80, Charset::Ascii);
    assert!(
        contains(&lines, "2 external boundaries"),
        "§50.1 and §9.6: the count is the graph's own, and a renderer that recounted it would \
         eventually disagree with the graph it draws"
    );
}

#[test]
fn should_draw_the_unicode_arrow_when_the_session_chose_unicode() {
    let lines = boundaries(&nginx_impact(), 80, Charset::Unicode);
    assert!(
        lines[0].contains('\u{25b8}'),
        "§20.3 permits a better glyph, and the boundary keeps its meaning in both alphabets"
    );
    assert!(lines[1].contains("beyond Ono visibility"));
}
