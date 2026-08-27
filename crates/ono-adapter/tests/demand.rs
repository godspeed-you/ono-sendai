//! The output demand a consumer places on a child's stdout (spec v0.3 §1.4, §1.5).
//!
//! Demand is decided by what is attached downstream, never by what the command could produce:
//! a byte consumer keeps Unix semantics, a native transform asks for values, and the terminal of
//! an interactive session invites the renderer.

use ono_adapter::{Consumer, OutputDemand};

#[test]
fn should_demand_raw_bytes_when_the_consumer_is_another_process() {
    assert_eq!(
        OutputDemand::for_consumer(Consumer::Process),
        OutputDemand::RawBytes,
        "spec v0.3 §1.4: `ss -tunap | grep ':443'` keeps classic external-pipeline semantics"
    );
}

#[test]
fn should_demand_structure_when_the_consumer_is_a_native_transform_over_objects() {
    assert_eq!(
        OutputDemand::for_consumer(Consumer::Native {
            input: "stream<any>"
        }),
        OutputDemand::Structured { schema: None },
        "`where` requires structured values (spec v0.3 §1.4)"
    );
}

#[test]
fn should_carry_the_schema_when_the_consumer_declares_a_concrete_element() {
    assert_eq!(
        OutputDemand::for_consumer(Consumer::Native {
            input: "null | stream<ono.process/1>"
        }),
        OutputDemand::Structured {
            schema: Some("ono.process/1".to_owned())
        },
        "a consumer declared over one schema constrains the demand to it (spec v0.3 §1.5)"
    );
}

#[test]
fn should_demand_raw_bytes_when_the_native_consumer_reads_bytes() {
    assert_eq!(
        OutputDemand::for_consumer(Consumer::Native {
            input: "string | bytes"
        }),
        OutputDemand::RawBytes,
        "`ip -j a | from json` is the user decoding bytes; nothing is adapted"
    );
}

#[test]
fn should_demand_text_when_the_native_consumer_reads_only_strings() {
    assert_eq!(
        OutputDemand::for_consumer(Consumer::Native { input: "string" }),
        OutputDemand::Text
    );
}

#[test]
fn should_demand_raw_bytes_when_stdout_is_redirected_to_a_file() {
    assert_eq!(
        OutputDemand::for_consumer(Consumer::File {
            path: "sockets.txt"
        }),
        OutputDemand::RawBytes,
        "spec v0.3 §1.4: redirection preserves raw external output by default"
    );
}

#[test]
fn should_discard_when_stdout_is_redirected_to_dev_null() {
    assert_eq!(
        OutputDemand::for_consumer(Consumer::File { path: "/dev/null" }),
        OutputDemand::Discard
    );
}

#[test]
fn should_demand_raw_bytes_when_stdout_is_duplicated_onto_another_descriptor() {
    assert_eq!(
        OutputDemand::for_consumer(Consumer::Descriptor),
        OutputDemand::RawBytes
    );
}

#[test]
fn should_demand_interactive_rendering_at_the_terminal_of_an_interactive_session() {
    assert_eq!(
        OutputDemand::for_consumer(Consumer::Terminal),
        OutputDemand::Interactive,
        "spec v0.3 §1.4: at a terminal a high-confidence adapter may let the renderer display"
    );
}

#[test]
fn should_demand_raw_bytes_when_stdout_is_an_inherited_stream() {
    assert_eq!(
        OutputDemand::for_consumer(Consumer::Stream),
        OutputDemand::RawBytes,
        "a script or `ono -c` whose stdout is a pipe must see what bash would (spec v0.3 §1.4)"
    );
}

#[test]
fn should_render_each_demand_as_one_word_a_plan_can_print() {
    let rendered: Vec<String> = [
        OutputDemand::RawBytes,
        OutputDemand::Text,
        OutputDemand::Structured { schema: None },
        OutputDemand::Structured {
            schema: Some("ono.socket/1".to_owned()),
        },
        OutputDemand::Interactive,
        OutputDemand::Discard,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        rendered,
        [
            "bytes",
            "text",
            "structured",
            "structured<ono.socket/1>",
            "interactive",
            "discard"
        ]
    );
}
