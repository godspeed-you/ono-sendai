//! Outcome tests for native commands: the object pipeline of spec §5, run by the shell itself.
//!
//! These assert what a user sees on stdout and in the exit status. Nothing here knows how a stage
//! is scheduled or which crate implements it (AGENTS.md §11).

use ono_testkit::Shell;
use ono_testkit::ono;

/// Runs a one-liner and returns the finished run.
#[test]
fn should_run_a_native_pipeline_and_serialise_the_result() {
    let run = ono("get process | count | to json");
    run.assert_success();

    // The document is the stream's shape, so a one-value stream is a one-element array: a script
    // whose output shape depended on how many rows the machine had would break on a quiet day.
    let text = run.stdout().trim().to_owned();
    let counted: i64 = text
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .unwrap_or_else(|_| panic!("`count | to json` must emit a JSON number, got {text:?}"));
    assert!(
        counted >= 1,
        "the shell counting processes must at least see itself, got {counted}"
    );
}

#[test]
fn should_filter_provider_objects_by_a_field_expression() {
    let run = ono("get process | where pid == 1 | select pid | to json");
    run.assert_success();

    // Spec §33.5: the data, with no Ono envelope around it. An external tool reads this.
    assert_eq!(
        run.stdout().trim(),
        r#"[{"pid":1}]"#,
        "`to json` serialises canonical data values, not the internal record envelope (§33.5)"
    );
}

#[test]
fn should_report_a_native_command_that_does_not_exist_as_not_found() {
    let run = ono("get definitely-not-a-target");
    assert!(
        !run.status().is_success(),
        "an undeclared target must not succeed silently, got {:?}",
        run.output()
    );
}

#[test]
fn should_parse_a_representation_piped_in_from_an_external_program() {
    let run = ono(
        "echo '[{\"name\":\"a\",\"size\":1},{\"name\":\"b\",\"size\":9}]' \
                   | from json | where size > 5 | select name | to json",
    );
    run.assert_success();

    assert_eq!(
        run.stdout().trim(),
        r#"[{"name":"b"}]"#,
        "the filter and the projection must both apply to the parsed objects"
    );
}

#[test]
fn should_write_the_same_bytes_whether_the_output_is_a_pipe_or_a_file() {
    let directory = ono_testkit::scratch();
    let target = directory.path().join("out.json");
    let script = format!(
        "get process | take 1 | select pid | to json > {}",
        target.display()
    );

    let piped = ono("get process | take 1 | select pid | to json");
    piped.assert_success();
    ono(&script).assert_success();

    let written = std::fs::read_to_string(&target).expect("the redirected file");
    assert_eq!(
        written.trim(),
        piped.stdout().trim(),
        "spec §50: redirected output must be the same bytes as piped output"
    );
}

#[test]
fn should_reject_a_misspelled_field_before_anything_runs() {
    let run = ono("get process | where cpy > 20");
    assert!(
        !run.status().is_success(),
        "a field the schema does not declare cannot succeed, got {:?}",
        run.output()
    );

    let stderr = run.stderr();
    assert!(
        stderr.contains("perhaps: cpu"),
        "spec §15.4: the near miss is suggested, got {stderr:?}"
    );
    assert_eq!(
        stderr.matches("cpy").count(),
        1,
        "spec §11.3: the typo is caught before enumeration begins, once — not once per \
         process. Got {stderr:?}"
    );
}

#[test]
fn should_read_the_shells_own_standard_input_into_a_parsing_stage() {
    // Spec §12.4's own example: `curl -s https://example/api | from json | where status == "open"`.
    // The bytes arrive on the shell's stdin, not from a stage inside the pipeline.
    let run = Shell::new()
        .args([
            "-c",
            r#"from json | where size > 5 | select name | to json"#,
        ])
        .stdin(r#"[{"name":"a","size":1},{"name":"b","size":9}]"#)
        .run();
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"name":"b"}]"#,
        "bytes piped into the shell reach the first parsing stage (spec §12.4)"
    );
}

#[test]
fn should_stream_a_value_that_starts_a_pipeline() {
    // ADR-0019: a list splices because it *is* several values, so `$xs | count` counts them.
    let run = ono("let xs = [1, 2, 3]; $xs | count | to json");
    run.assert_success();
    assert_eq!(run.stdout().trim(), "[3]");
}

#[test]
fn should_reuse_the_previous_result_without_rerunning_it() {
    // Spec §20.2: `@-1 | where …` reuses the retained structured result — no screen scraping,
    // and no second enumeration.
    let run = ono("get process | where pid == 1 | select pid; @-1 | count | to json");
    run.assert_success();
    let last = run.stdout().lines().last().unwrap_or_default().to_owned();
    assert_eq!(
        last, "[1]",
        "the retained result has exactly the one row that was shown"
    );
}

#[test]
fn should_pick_one_item_of_the_current_result_by_position() {
    let run = ono("let xs = [\"a\", \"b\", \"c\"]; $xs | take 3; @2 | to json");
    run.assert_success();
    let last = run.stdout().lines().last().unwrap_or_default().to_owned();
    assert_eq!(
        last, r#"["b"]"#,
        "spec §6.4: `@2` is item 2 of the shown result"
    );
}

#[test]
fn should_keep_a_secret_out_of_the_retained_result_as_well_as_out_of_history() {
    // Spec §20.2: "Retention policy must protect secrets"; §17.5: a secret must not reach
    // history through renderer output. The retention applies the same policy history does, so
    // the shell cannot redact the command that read a token and keep the token (ADR-0262).
    let run = Shell::new()
        .args(["-c", "from json; @-1 | to json"])
        .stdin("[{\"cmd\":\"psql --password=hunter2\"}]")
        .run();
    run.assert_success();
    let replayed = run.stdout().lines().last().unwrap_or_default().to_owned();
    assert!(
        !replayed.contains("hunter2"),
        "the secret is not replayed by `@-1`: {replayed}"
    );
    assert!(
        replayed.contains("--password=<redacted>"),
        "the command stays readable and only the value is gone: {replayed}"
    );
}

#[test]
fn should_keep_an_assignment_s_value_out_of_the_retained_result() {
    let run = Shell::new()
        .args(["-c", "from json; @-1 | to json"])
        .stdin("[{\"line\":\"AWS_SECRET_ACCESS_KEY=abc123\"}]")
        .run();
    run.assert_success();
    let replayed = run.stdout().lines().last().unwrap_or_default().to_owned();
    assert!(
        !replayed.contains("abc123") && replayed.contains("<redacted>"),
        "an assignment's value is a secret wherever it is kept: {replayed}"
    );
}

#[test]
fn should_leave_ordinary_text_in_a_retained_result_alone() {
    // A redaction that fires on ordinary text teaches people to turn it off (ADR-0033's own
    // reasoning, carried over from history).
    let run = Shell::new()
        .args(["-c", "from json; @-1 | to json"])
        .stdin("[{\"a\":\"hello world\"}]")
        .run();
    run.assert_success();
    assert_eq!(
        run.stdout().lines().last(),
        Some("[{\"a\":\"hello world\"}]"),
        "nothing about this is a secret: {:?}",
        run.output()
    );
}

#[test]
fn should_say_there_is_nothing_to_reuse_when_no_result_was_retained() {
    let run = ono("@-1 | count");
    assert!(!run.status().is_success());
    assert!(
        run.stderr().contains("Ono-Sendai-E"),
        "a missing result is a structured error, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_draw_a_trace_as_a_tree_rather_than_a_table() {
    // Spec §13.6: a graph never renders as a table. PID 1 exists everywhere and always has
    // relationships — children at least.
    let run = ono("trace process 1");
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains("+--") || text.contains("└") || text.contains("├"),
        "the graph draws as a tree (spec §13.6), got {text:?}"
    );
    assert!(
        text.contains("1") && (text.contains("systemd") || text.contains("init")),
        "the root names the traced process, got {text:?}"
    );
}

#[test]
fn should_carry_a_trace_through_the_pipeline_as_a_graph_value() {
    let run = ono("trace process 1 | type");
    run.assert_success();
    run.assert_stdout_contains("ono.graph/1");
}

#[test]
fn should_walk_a_wide_tree_without_hoarding_descriptors() {
    // ADR-0015 (F11): the walk used to hold one open descriptor per *pending* directory, so a
    // tree wider than the descriptor table killed it. Under a 64-descriptor limit, five hundred
    // sibling directories must still be walkable — the walk may hold the root and the one
    // directory it is reading, never the frontier.
    let scratch = ono_testkit::scratch();
    for i in 0..500 {
        scratch.write(format!("wide/dir-{i:03}/leaf.txt"), "x");
    }

    let run = Shell::program("/bin/bash")
        .args([
            "-c".to_owned(),
            format!(
                "ulimit -n 64 2>/dev/null; exec {} --no-config -c 'find file {}/wide | count | to json'",
                ono_testkit::ono_binary().display(),
                scratch.path().display(),
            ),
        ])
        .run();
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[1001]",
        "the root, five hundred directories and five hundred leaves, all reached"
    );
}

#[test]
fn should_sort_descending_with_the_specs_own_spelling() {
    // Spec §6.3 and §48 write `sort cpu desc` — the direction is a word bound to a string
    // selector, never a field the §11.3 check should reject.
    let run = ono("get process | sort pid desc | take 1 | select pid | to json");
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains("pid") && !text.contains("Ono-Sendai-E"),
        "the highest pid comes first and nothing was refused: {text:?}"
    );
}

#[test]
fn should_not_wait_on_stdin_when_a_seeded_pipeline_starts_with_a_serializer() {
    use std::io::Read as _;
    use std::process::{Command, Stdio};
    // The pipe is held open for the whole test: a shell that read stdin here would never see
    // its end, and a seeded pipeline already has its input.
    let mut child = Command::new(ono_testkit::ono_binary())
        .args(["-c", "let s = \"x\"; $s | to json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the shell starts");
    let held_open = child.stdin.take();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().expect("the shell can be waited for") {
            break status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "a seeded pipeline must not wait on stdin: the shell is still running after 10s"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    drop(held_open);
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("stdout is piped")
        .read_to_string(&mut stdout)
        .expect("stdout is text");
    assert!(status.success(), "the shell failed: {status}");
    assert_eq!(stdout, "[\"x\"]\n");
}

#[test]
fn should_compare_an_enum_field_with_its_bare_value_when_the_spec_example_is_written() {
    // Spec §33.2 / §41.4 spell it `where state == failed`; `ono.process/1`'s `state` is an enum
    // with `running` among its values (ADR-0096). PID 1 is always there and never zombie.
    let run = Shell::new()
        .args([
            "-c",
            "get process | where pid == 1 | where state != zombie | count | to json",
        ])
        .run();
    assert!(
        run.status().is_success(),
        "a bare word naming a value of the enum field is that value, got {:?}",
        run.output()
    );
    assert_eq!(run.stdout().trim(), "[1]");
    let typo = Shell::new()
        .args(["-c", "get process | where state == sleping | count"])
        .run();
    assert!(
        !typo.status().is_success() && typo.stderr().contains("Ono-Sendai-E0202"),
        "a word that is neither a field nor a value is still E0202, got {:?}",
        typo.output()
    );
}

#[test]
fn should_fail_the_run_when_a_serialised_stream_carried_nothing_but_failures() {
    // ADR-0028: only when nothing arrived at all is there no answer, and that is the case the
    // status reports. A serializer is a representation of what arrived, not a source of it —
    // so a stream that failed before its first value has nothing for `to json` to write, and the
    // run fails the way it does without the serializer.
    let run = ono("get file /definitely/not/here | to json");
    assert!(
        !run.status().is_success(),
        "a producer whose only answer is a failure fails the run, serialised or not, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0301"),
        "the failure is reported as the structured error it was, got {:?}",
        run.stderr()
    );
    assert!(
        !run.stdout().contains("[]"),
        "an empty document would claim there was an answer and it was empty, got {:?}",
        run.stdout()
    );

    // A filter that matched nothing is an answer — empty — and still serialises as one.
    let empty = ono("get process | where pid == 2147483647 | to json");
    empty.assert_success();
    assert_eq!(empty.stdout().trim(), "[]");
}

// --- the byte boundary of spec §12.2 and §12.3, both ways (B-split-B8) --------------------------

#[test]
fn should_refuse_to_hand_objects_to_a_program_and_say_which_representation_to_choose() {
    // Spec §12.3: "An object pipeline cannot be silently sent to an arbitrary process." The
    // refusal is only useful if it carries the fix, so the message names a representation the
    // user can actually write. Case `040-object-pipeline` proves this against the real system;
    // nothing in the workspace did, and the workspace is where a regression is caught first.
    let run = ono("get process | grep x");
    assert!(
        !run.status().is_success(),
        "objects aimed at a program that reads bytes must not run, got {:?}",
        run.output()
    );
    let complaint = run.stderr().to_owned();
    assert!(
        complaint.contains("Ono-Sendai-E0201"),
        "spec §43: the refusal is `type.mismatch`, got {complaint:?}"
    );
    assert!(
        complaint.contains("to json"),
        "spec §12.3: the error suggests the serialization to choose, got {complaint:?}"
    );
}

#[test]
fn should_carry_undecodable_bytes_from_a_child_process_into_a_value_without_losing_them() {
    // Spec §12.2: external stdout enters the value system as bytes, and undecodable bytes are
    // never lost. `0xFF` is not valid UTF-8 anywhere, so a shell that decoded lossily would put
    // U+FFFD (`efbfbd`) where the byte was, and one that decoded strictly would drop the value
    // entirely. Bytes serialise as hex (spec §33.5), so the whole sequence is readable back.
    let run = ono(r"printf 'a\xffb' | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"["61ff62"]"#,
        "spec §12.2: the child's three bytes arrive as three bytes — a lossy decode would show \
         `61efbfbd62`, got {:?}",
        run.stdout()
    );
}

// --- the *bounded* half of §20.2's bounded retention (B-split-E5) --------------------------------

#[test]
fn should_evict_the_oldest_retained_result_when_the_seventeenth_arrives() {
    // Spec §20.2: retention is bounded, and a bound nobody drives is a number nobody has
    // checked. Seventeen results are retained in one run; the sixteen most recent answer, and
    // the seventeenth back — the first — is gone with the structured refusal, not with an empty
    // list that a script would read as "no rows".
    let mut script = String::new();
    for n in 1..=17 {
        script.push_str(&format!("let v{n} = [{n}]; $v{n} | take 1; "));
    }

    let newest = ono(&format!("{script} @-1 | to json"));
    newest.assert_success();
    assert_eq!(
        newest.stdout().lines().last().unwrap_or_default(),
        "[17]",
        "`@-1` is the most recent result"
    );

    let oldest_kept = ono(&format!("{script} @-16 | to json"));
    oldest_kept.assert_success();
    assert_eq!(
        oldest_kept.stdout().lines().last().unwrap_or_default(),
        "[2]",
        "sixteen results are kept, so the oldest reachable one is the second that ran — the \
         first was evicted"
    );

    let evicted = ono(&format!("{script} @-17 | to json"));
    assert!(
        !evicted.status().is_success(),
        "the seventeenth result back was evicted, got {:?}",
        evicted.output()
    );
    assert!(
        evicted.stderr().contains("Ono-Sendai-E0102"),
        "spec §43: a result that is no longer retained is a structured refusal, got {:?}",
        evicted.stderr()
    );
}

#[test]
fn should_say_so_when_a_result_is_too_large_to_retain_whole() {
    // Spec §20.2 bounds a result's values as well as the number of results. Truncating in
    // silence is the failure mode this pins: `@-1` would come up short of what the screen just
    // showed and nothing would connect the two. The notice goes to stderr, so stdout is still
    // only the answer (spec §33.2).
    let directory = ono_testkit::scratch();
    let document: String = (0..10_005)
        .map(|n| format!("{{\"n\":{n}}}"))
        .collect::<Vec<String>>()
        .join(",");
    directory.write("big.json", format!("[{document}]"));

    let run = Shell::new()
        .cwd(directory.path())
        .args([
            "-c",
            "read file big.json --encoding utf-8 | from json | take 10005; @-1 | count | to json",
        ])
        .run();
    run.assert_success();

    assert!(
        run.stderr()
            .contains("retained the first 10000 of 10005 values"),
        "spec §20.2: a truncated retention says so, got stderr {:?}",
        run.stderr()
    );
    assert_eq!(
        run.stdout().lines().last().unwrap_or_default(),
        "[10000]",
        "what `@-1` reuses is what was retained, and the notice said how much that is"
    );
}

// --- the other direction of §12.3: bytes reaching a stage defined over objects (B-data-9) -------

#[test]
fn should_refuse_a_program_whose_bytes_cannot_become_the_objects_the_next_stage_needs() {
    // Spec §12.3 makes the boundary explicit in both directions. `count` is declared over a
    // stream of objects and `ls` writes bytes no adapter decodes, so there is nothing for
    // `count` to count. Answering `1` — the whole listing wrapped as a single value — is the
    // silent conversion §12.3 exists to forbid, and it is the wrong number besides.
    let run = ono("ls /etc | count");
    assert!(
        !run.status().is_success(),
        "bytes no adapter can turn into objects must not reach a stage defined over objects, \
         got {:?}",
        run.output()
    );
    let complaint = run.stderr().to_owned();
    assert!(
        complaint.contains("Ono-Sendai-E0911"),
        "spec §43: the refusal is `adapter.required_for_structured_pipeline`, got {complaint:?}"
    );
    assert!(
        complaint.contains("count"),
        "the refusal names the stage that needs objects, got {complaint:?}"
    );
    assert!(
        complaint.contains("raw ls") || complaint.contains("from "),
        "the refusal carries the routes out — run it raw, or decode it yourself, \
         got {complaint:?}"
    );
}

#[test]
fn should_answer_at_once_when_an_endless_program_feeds_a_stage_defined_over_objects() {
    // B-data-9's exit test. `yes` never ends, so a shell that reads its output before deciding
    // whether the next stage can use it never answers at all. The question — can this program
    // give `take` the objects it is declared over? — is answerable from the contracts alone,
    // before anything is spawned, and that is when it is asked.
    let started = std::time::Instant::now();
    let run = ono("yes | take 1");
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "`yes | take 1` must answer without waiting for a producer that never ends, took {elapsed:?}"
    );
    assert!(
        !run.status().is_success(),
        "there is nothing for `take` to take from bytes nothing decodes, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0911"),
        "spec §43: the refusal is `adapter.required_for_structured_pipeline`, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_still_carry_a_whole_document_across_the_boundary_into_a_parser() {
    // The byte carry ADR-0028 buffers is a *document*, and a document is one value: `from json`
    // cannot answer half a document, so the buffering is the semantics rather than a shortcut.
    // Only the stages declared over bytes reach it, which is what the refusals above enforce.
    let run = ono("printf '[{\"a\":1},{\"a\":2}]' | from json | count | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "[2]",
        "a program's bytes reach the parser declared over them, whole, got {:?}",
        run.output()
    );
}

/// The relation names on the edges of a `trace ... | to json` answer.
fn traced_relations(script: &str) -> Vec<String> {
    let run = ono(script);
    run.assert_success();
    let text = run.stdout();
    // The answer is one `ono.graph/1` document; its edges each name the relation they assert.
    text.split("\"relation\":\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next().map(str::to_owned))
        .collect()
}

#[test]
fn should_restrict_a_trace_to_the_relations_a_list_names() {
    // Spec §22.3 declares `--relations` as "the relation names to restrict the trace to", and
    // `docs/spec/commands/process.yaml` types it `list<string>`. A list is how the language
    // spells a list, so writing one must restrict the walk — an option that is read and dropped
    // makes the contract and the answer disagree.
    let unrestricted = traced_relations("trace process 1 --depth 2 | to json");
    assert!(
        unrestricted.iter().any(|relation| relation != "child"),
        "this test needs a trace that reaches more than one relation to restrict, got \
         {unrestricted:?}"
    );

    let restricted =
        traced_relations(r#"trace process 1 --depth 2 --relations ["child"] | to json"#);
    assert!(
        !restricted.is_empty(),
        "restricting to a relation the trace does reach must keep its edges"
    );
    assert!(
        restricted.iter().all(|relation| relation == "child"),
        "`--relations [\"child\"]` must answer child edges and no others, got {restricted:?}"
    );
}

#[test]
fn should_restrict_a_trace_to_the_relations_a_word_names() {
    // The same option written the way a words-mode command usually takes one. Both spellings
    // reach the same walk, so both restrict it.
    let restricted = traced_relations("trace process 1 --depth 2 --relations child | to json");
    assert!(
        !restricted.is_empty() && restricted.iter().all(|relation| relation == "child"),
        "`--relations child` must answer child edges and no others, got {restricted:?}"
    );
}

#[test]
fn should_refuse_a_relation_list_that_names_something_undefined() {
    // `[child]` is a list holding a bare name, and a bare name is a variable the language has
    // never been given. Answering the *unrestricted* graph to that is the worst of the three
    // possible outcomes: the reader asked a narrower question and was handed a wider answer with
    // nothing said about it (spec §10.5's discipline, applied to arguments).
    let run = ono("trace process 1 --depth 2 --relations [child] | to json");
    assert!(
        !run.status().is_success(),
        "an argument that cannot be evaluated must refuse, got {:?}",
        run.stdout()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E"),
        "the refusal is structured (spec §43), got {:?}",
        run.stderr()
    );
}
