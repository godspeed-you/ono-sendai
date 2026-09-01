//! Outcome tests for the remote family the contract declares:
//! `get host`, `test host`, `connect host`, `trace host`, `watch host`, `add/set/remove host`,
//! `get link | to json`, `detach link`, `add/set/rename/remove link`, `watch link`, `trace link`,
//! the `--agentless` mode of `link host`, and the honesty of mutations across a link.
//!
//! Contract: `docs/spec/commands/remote.yaml`, schemas `ono.link/1`, `ono.graph/1`,
//! `ono.action-result/1`, `ono.context/1`, and the deferred `ono.host/1`, `ono.probe-result/1`,
//! `ono.link-event/1`. Narrative: spec §9.1 (the remote table), §14.4 (the link frame decides
//! where provider calls run), §16.5 (no collapsed failures), §21.2 (the handshake), §21.3
//! (agentless fallback MUST be visible), §33.4 (the link summary), §42.2 (the destructive
//! remote plan), §49 (explicit trust for links). ADR-0036 (a remote is mounted as ordinary
//! providers — nothing above the registry can tell), ADR-0037 (transports; `local` spawns this
//! binary as `ono --agent`), ADR-0006 (a failed ActionResult row fails the run).
//!
//! `crates/ono-cli/tests/remote.rs` proves what already works: link, enter, query, leave, adapt.
//! Nothing here repeats it. The only reachable "host" is the local transport, so every test is
//! offline; hosts that must be unreachable point at a closed port on the loopback interface.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::net::TcpListener;
use std::path::Path;
use std::time::{Duration, Instant};

use ono_testkit::{Scratch, Shell, scratch};
use serde_yaml_ng::Value;

const LINK: &str = "link host testbox --transport local";

fn ono(script: &str) -> ono_testkit::Run {
    Shell::new().args(["-c", script]).run()
}

/// Runs with the scratch directory as the home and config directory, so the host sources the
/// shell consults are the test's own and never the developer machine's.
fn ono_at_home(home: &Scratch, script: &str) -> ono_testkit::Run {
    Shell::new()
        .env("HOME", home.path().to_string_lossy().into_owned())
        .env(
            "XDG_CONFIG_HOME",
            home.path().to_string_lossy().into_owned(),
        )
        .args(["-c", script])
        .run()
}

/// The last non-empty line of stdout: what `to json` wrote for the final statement. Earlier
/// statements (`link host` prints its summary line) are allowed to write before it.
fn last_line(run: &ono_testkit::Run) -> String {
    run.stdout()
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// Parses the last line of stdout as the JSON document `to json` wrote for the final statement.
fn last_json(run: &ono_testkit::Run) -> Value {
    let stderr = run.stderr();
    let line = last_line(run);
    serde_yaml_ng::from_str(&line).unwrap_or_else(|error| {
        panic!("`to json` must emit a JSON document, got {line:?} ({error}); stderr: {stderr:?}")
    })
}

/// The values of the stream the final statement serialised.
fn rows(run: &ono_testkit::Run) -> Vec<Value> {
    let document = last_json(run);
    document
        .as_sequence()
        .unwrap_or_else(|| {
            panic!(
                "spec §33.5: `to json` emits the stream as an array, got {:?}; stderr: {:?}",
                run.stdout(),
                run.stderr()
            )
        })
        .clone()
}

fn single_row(run: &ono_testkit::Run) -> Value {
    let mut rows = rows(run);
    assert_eq!(
        rows.len(),
        1,
        "one record was asked for, got {:?}; stderr: {:?}",
        run.stdout(),
        run.stderr()
    );
    rows.remove(0)
}

fn text(row: &Value, field: &str) -> String {
    row[field]
        .as_str()
        .unwrap_or_else(|| panic!("field `{field}` must be a string, got {row:?}"))
        .to_owned()
}

fn names(rows: &[Value]) -> Vec<String> {
    rows.iter().map(|row| text(row, "name")).collect()
}

fn serialised(value: &Value) -> String {
    serde_yaml_ng::to_string(value).expect("a value serialises")
}

/// A loopback port nothing listens on: bound once to learn the number, then released.
fn closed_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind a loopback port")
        .local_addr()
        .expect("the bound address")
        .port()
}

/// Whether any regular file below `root` mentions `needle`.
fn any_file_mentions(root: &Path, needle: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            any_file_mentions(&path, needle)
        } else {
            std::fs::read_to_string(&path).is_ok_and(|contents| contents.contains(needle))
        }
    })
}

// --- get link as data -------------------------------------------------------------------------

#[test]
fn should_serialise_a_held_link_as_a_typed_record() {
    // The link table is a stream<ono.link/1>, so it must cross `to json` like every other
    // stream (spec §33.5) — a table that exists only as rendered text is not a record.
    let run = ono(&format!("{LINK}; get link | to json"));
    run.assert_success();
    let row = single_row(&run);
    assert_eq!(
        text(&row, "name"),
        "testbox",
        "ono.link/1: `name` is the host as the user named it, got {row:?}"
    );
    assert_eq!(
        text(&row, "transport"),
        "local",
        "ono.link/1: `transport` says how the bytes travel, got {row:?}"
    );
    assert_eq!(
        text(&row, "state"),
        "connected",
        "ono.link/1: a link that just negotiated is `connected`, got {row:?}"
    );
    let targets = row["targets"]
        .as_sequence()
        .unwrap_or_else(|| panic!("ono.link/1: `targets` is a list<string>, got {row:?}"));
    assert!(
        targets
            .iter()
            .any(|target| target.as_str() == Some("process")),
        "spec §21.2: `targets` lists what the remote negotiated, got {targets:?}"
    );
}

#[test]
fn should_serialise_an_empty_link_table_when_nothing_is_linked() {
    let run = ono("get link | to json");
    run.assert_success();
    assert!(
        rows(&run).is_empty(),
        "spec §9.1: `get link` lists active links, and there are none: {:?}",
        run.stdout()
    );
}

// --- get host ---------------------------------------------------------------------------------

#[test]
fn should_list_a_linked_host_among_the_known_hosts() {
    // Spec §9.1: `get host` enumerates known hosts from configured providers and sources. A host
    // this session holds a link to is the best-known host there is.
    let home = scratch();
    let run = ono_at_home(&home, &format!("{LINK}; get host | to json"));
    run.assert_success();
    let row = single_row(&run);
    assert_eq!(
        text(&row, "name"),
        "testbox",
        "ono.host/1: the linked host is listed by name, got {row:?}"
    );
    assert!(
        serialised(&row).contains("local"),
        "ono.host/1: the record says how the host is reached (the link's transport), got {row:?}"
    );
}

#[test]
fn should_list_a_host_from_the_ssh_client_configuration_with_its_source() {
    // The ssh transport of ADR-0037 runs `ssh <host>`, which reads `~/.ssh/config`; a `Host`
    // entry there is therefore a configured host source (spec §9.1), and the record says which.
    let home = scratch();
    home.write(
        ".ssh/config",
        "Host devbox\n    HostName 10.4.2.11\n    User deploy\n",
    );
    let run = ono_at_home(&home, "get host | to json");
    run.assert_success();
    let rows = rows(&run);
    let devbox = rows
        .iter()
        .find(|row| row["name"].as_str() == Some("devbox"))
        .unwrap_or_else(|| panic!("spec §9.1: the configured host is listed, got {rows:?}"));
    assert!(
        devbox["source"]
            .as_str()
            .is_some_and(|source| !source.is_empty()),
        "ono.host/1: a host from a source carries that source's name, got {devbox:?}"
    );
}

#[test]
fn should_resolve_one_configured_host_by_name() {
    let home = scratch();
    home.write(
        ".ssh/config",
        "Host devbox\n    HostName 10.4.2.11\n\nHost other\n    HostName 10.4.2.12\n",
    );
    let run = ono_at_home(&home, "get host devbox | to json");
    run.assert_success();
    assert_eq!(
        names(&rows(&run)),
        ["devbox"],
        "remote.yaml: the `name` selector resolves one host, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_answer_an_empty_host_list_when_nothing_is_configured() {
    let home = scratch();
    let run = ono_at_home(&home, "get host | to json");
    run.assert_success();
    assert!(
        rows(&run).is_empty(),
        "spec §35.3: no configured source means no hosts, never a fabricated one: {:?}",
        run.stdout()
    );
}

// --- test host --------------------------------------------------------------------------------

#[test]
fn should_report_a_linked_host_as_reachable_with_what_it_negotiated() {
    // Spec §21.2 lists what a handshake negotiates: protocol version, agent present, available
    // providers. `test host` reports that for a host the session already holds a link to.
    let run = ono(&format!("{LINK}; test host testbox | to json"));
    run.assert_success();
    let row = single_row(&run);
    assert_eq!(
        row["reachable"].as_bool(),
        Some(true),
        "ono.probe-result/1: a host that answered the handshake is reachable, got {row:?}"
    );
    let keys: Vec<String> = row
        .as_mapping()
        .map(|map| {
            map.keys()
                .map(|key| serialised(key).trim().to_owned())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        keys.iter().any(|key| key.contains("protocol")),
        "spec §21.2: the probe reports the negotiated protocol version, got fields {keys:?}"
    );
    let providers = row["providers"].as_sequence().unwrap_or_else(|| {
        panic!("spec §21.2: the probe lists the available providers, got {row:?}")
    });
    assert!(
        serialised(&Value::Sequence(providers.clone())).contains("proc"),
        "spec §21.2: the process provider the agent offers is among them, got {providers:?}"
    );
}

#[test]
fn should_report_an_unreachable_host_within_the_timeout() {
    // A closed loopback port stands in for a host that does not answer; `~/.ssh/config` is how
    // the ssh transport learns where `nowhere` lives, so no name resolution and no network.
    let home = scratch();
    home.write(
        ".ssh/config",
        format!(
            "Host nowhere\n    HostName 127.0.0.1\n    Port {}\n    ConnectTimeout 2\n",
            closed_port()
        ),
    );
    let started = Instant::now();
    let run = Shell::new()
        .env("HOME", home.path().to_string_lossy().into_owned())
        .env(
            "XDG_CONFIG_HOME",
            home.path().to_string_lossy().into_owned(),
        )
        .args(["-c", "test host nowhere --timeout 2s | to json"])
        .timeout(Duration::from_secs(15))
        .try_run();
    let run = run.unwrap_or_else(|error| {
        panic!("remote.yaml: `--timeout` bounds the probe; the run did not end: {error}")
    });
    assert!(
        started.elapsed() < Duration::from_secs(12),
        "remote.yaml: `--timeout 2s` bounds how long the probe waits, took {:?}",
        started.elapsed()
    );
    assert!(
        !run.status().is_success(),
        "a failed probe fails the run, got {:?}",
        run.output()
    );
    let reported_unreachable =
        run.stdout().contains("\"reachable\":false") || run.stderr().contains("Ono-Sendai-E0601");
    assert!(
        reported_unreachable,
        "errors.yaml E0601 remote.unreachable: `test host` reports where the attempt failed, \
         got {:?}",
        run.output()
    );
}

// --- connect host -----------------------------------------------------------------------------

#[test]
fn should_enter_the_remote_context_when_connecting_to_a_host() {
    // remote.yaml: `connect host` opens a protocol connection and, unlike `link host`, is
    // itself the switch of context (spec §6.1 `connect host prod-db` → `prod-db://~ >`).
    let run = ono("connect host testbox --transport local; get context | to json");
    run.assert_success();
    let frames = rows(&run);
    let link_frame = frames
        .iter()
        .find(|frame| frame["kind"].as_str() == Some("link"))
        .unwrap_or_else(|| {
            panic!("spec §14.4: connecting pushes the remote frame, got {frames:?}")
        });
    assert_eq!(
        text(link_frame, "identity"),
        "testbox",
        "ono.context/1: the frame's identity is the host, got {link_frame:?}"
    );
}

#[test]
fn should_answer_from_the_host_after_connecting_to_it() {
    let run = ono("connect host testbox --transport local; \
         get process | where pid == 1 | inspect | to json");
    run.assert_success();
    assert!(
        run.stdout().contains("\"link\":\"testbox\""),
        "spec §14.4 + ADR-0036: inside the connected frame the answers carry the host as their \
         provenance, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_persist_no_link_when_connecting_to_a_host() {
    // remote.yaml: "Open a protocol connection to a host without persisting a link" — once the
    // frame is left, nothing remains in the link table.
    let run = ono("connect host testbox --transport local; leave; get link | to json");
    run.assert_success();
    assert!(
        rows(&run).is_empty(),
        "remote.yaml: `connect host` is one-shot, `link host` is what persists, got {:?}",
        run.stdout()
    );
}

// --- trace ------------------------------------------------------------------------------------

#[test]
fn should_trace_a_host_to_its_link_and_negotiated_providers() {
    // remote.yaml: "Show a host's links, sessions, addresses and reachable services" as an
    // ono.graph/1 (spec §22.1): the host is a node, its link is a node, the edge joins them.
    let run = ono(&format!("{LINK}; trace host testbox | to json"));
    run.assert_success();
    let graph = single_row(&run);
    let nodes = graph["nodes"]
        .as_sequence()
        .unwrap_or_else(|| panic!("ono.graph/1: `nodes` is a list, got {graph:?}"));
    let edges = graph["edges"]
        .as_sequence()
        .unwrap_or_else(|| panic!("ono.graph/1: `edges` is a list, got {graph:?}"));
    assert!(
        nodes
            .iter()
            .any(|node| serialised(node).contains("testbox")),
        "spec §22.1: the traced host is a node, got {nodes:?}"
    );
    assert!(
        nodes
            .iter()
            .any(|node| node["kind"].as_str() == Some("ono.link/1")),
        "remote.yaml: the host's link is a node of kind ono.link/1, got {nodes:?}"
    );
    assert!(
        !edges.is_empty(),
        "spec §22.1: the link is joined to its host by an edge, got {edges:?}"
    );
    assert!(
        serialised(&graph).contains("linux.procfs"),
        "spec §21.2 + ADR-0036: the negotiated providers keep their ids and appear in the \
         trace, got {graph:?}"
    );
}

#[test]
fn should_trace_a_link_to_its_transport_agent_and_providers() {
    // remote.yaml: "Show a link's transport, agent, negotiated providers and multiplexed
    // streams" — the same facts §33.4's link summary prints, as a graph.
    let run = ono(&format!("{LINK}; trace link testbox | to json"));
    run.assert_success();
    let graph = single_row(&run);
    let text = serialised(&graph);
    assert!(
        text.contains("local"),
        "spec §33.4: the trace names the transport, got {text}"
    );
    assert!(
        text.contains("agent"),
        "spec §33.4: the trace names the agent on the far side, got {text}"
    );
    assert!(
        text.contains("linux.procfs"),
        "spec §21.2 + ADR-0036: the negotiated providers keep their ids, got {text}"
    );
    assert!(
        !graph["nodes"]
            .as_sequence()
            .unwrap_or(&Vec::new())
            .is_empty(),
        "ono.graph/1: the facts are nodes, not prose, got {graph:?}"
    );
}

// --- watch ------------------------------------------------------------------------------------

#[test]
fn should_begin_a_link_watch_with_a_snapshot() {
    // ADR-0024: a native live stream begins with the current state as `snapshot` events.
    let run = ono(&format!(
        "{LINK}; watch link | take 1 | select kind | to json"
    ));
    run.assert_success();
    assert_eq!(
        last_line(&run),
        r#"[{"kind":"snapshot"}]"#,
        "spec §18.2 + ADR-0024: the first ono.link-event/1 is the snapshot, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_list_the_held_link_in_the_watch_snapshot() {
    let run = ono(&format!("{LINK}; watch link | take 1 | to json"));
    run.assert_success();
    assert!(
        last_line(&run).contains("testbox"),
        "ono.link-event/1: the snapshot carries the link it describes, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_begin_a_host_watch_with_a_snapshot() {
    let home = scratch();
    let run = ono_at_home(
        &home,
        &format!("{LINK}; watch host --every 1s | take 1 | select kind | to json"),
    );
    run.assert_success();
    assert_eq!(
        last_line(&run),
        r#"[{"kind":"snapshot"}]"#,
        "spec §18.2 + ADR-0024: the first ono.host-event/1 is the snapshot, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_begin_a_host_watch_with_an_empty_snapshot_when_no_host_is_known() {
    // ADR-0024: the first event is the current state — and "no hosts" is a state. Without it
    // `watch host | take 1` waits forever for a host to appear.
    let home = scratch();
    let run = Shell::new()
        .env("HOME", home.path().to_string_lossy().into_owned())
        .env(
            "XDG_CONFIG_HOME",
            home.path().to_string_lossy().into_owned(),
        )
        .args([
            "-c",
            "watch host --every 1s | take 1 | select kind | to json",
        ])
        .timeout(Duration::from_secs(20))
        .run();
    run.assert_success();
    assert_eq!(
        last_line(&run),
        r#"[{"kind":"snapshot"}]"#,
        "an empty listing still begins with its snapshot (ADR-0024), got {:?}",
        run.stdout()
    );
}

// --- the piped forms: `get link | remove link` and friends (ADR-0118) -----------------------

#[test]
fn should_remove_the_piped_links_when_remove_link_follows_get_link() {
    // remote.yaml `remove link`: input `null | stream<ono.link/1>` — the piped records are the
    // targets, and the shell answers exactly as it does for `remove link <name>`.
    let run = ono(&format!(
        "{LINK}; get link | remove link | select status operation | to json; get link | count"
    ));
    run.assert_success();
    assert!(
        run.stdout()
            .contains(r#"[{"status":"success","operation":"ono.link.remove"}]"#),
        "one ono.action-result/1 per piped link, got {:?}",
        run.output()
    );
    assert!(
        run.stdout().trim_end().ends_with("VALUE\n0"),
        "the piped link is gone afterwards, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_detach_the_piped_link_when_detach_link_follows_get_link() {
    // Outside the link's frame (inside it, spec §14.4 sends `get link` to the other side) there
    // is nothing to detach from — and that is the head form's own answer: a success row that
    // changed nothing, never E0101.
    let run = ono(&format!(
        "{LINK}; get link | detach link | select changed status | to json"
    ));
    run.assert_success();
    assert_eq!(
        last_line(&run),
        r#"[{"changed":false,"status":"success"}]"#,
        "the piped link is answered as `detach link testbox` would be, got {:?}",
        run.output()
    );
}

#[test]
fn should_modify_the_piped_links_when_set_link_follows_get_link() {
    let run = ono("add link devbox --host devbox.example --transport ssh; \
         get link | set link --transport local | select status | to json; \
         get link | select transport | to json");
    run.assert_success();
    assert!(
        run.stdout().contains(r#"[{"status":"success"}]"#),
        "the piped definition is modified, got {:?}",
        run.output()
    );
    assert_eq!(
        last_line(&run),
        r#"[{"transport":"local"}]"#,
        "remote.yaml `set link --transport`: the option applies to the piped link, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_rename_the_piped_link_when_rename_link_follows_get_link() {
    // remote.yaml `rename link`: input `null | ono.link/1` — one record, and the one remaining
    // selector is the new name.
    let run = ono("add link devbox --host devbox.example --transport ssh; \
         get link devbox | rename link prodbox; get link | to json");
    run.assert_success();
    assert_eq!(
        names(&rows(&run)),
        ["prodbox"],
        "only the new name remains, got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_piped_host_with_a_type_error_naming_the_head_form_for_connect_and_link_host() {
    // remote.yaml declares `input: "null"` for `connect host` and `link host`: the honest answer
    // to `get host | connect host` is a type error that says how to spell it — never E0101.
    let home = scratch();
    for verb in ["connect", "link"] {
        let run = ono_at_home(&home, &format!("get host | {verb} host"));
        assert!(
            !run.status().is_success(),
            "`get host | {verb} host` is refused, got {:?}",
            run.output()
        );
        assert!(
            run.stderr().contains("Ono-Sendai-E0201"),
            "the refusal is the type error of spec §43, got {:?}",
            run.stderr()
        );
        assert!(
            run.stderr().contains(&format!("{verb} host <name>")),
            "the refusal names the head form, got {:?}",
            run.stderr()
        );
        assert!(
            !run.stderr().contains("E0101"),
            "a declared, implemented command never claims to be unimplemented, got {:?}",
            run.stderr()
        );
    }
}

// --- link definitions: add, set, rename, remove, detach ---------------------------------------

#[test]
fn should_record_a_link_definition_without_establishing_it() {
    // remote.yaml: "Record a link definition without establishing it" — nothing is spawned, so
    // the record exists and is visibly not connected.
    let run = ono("add link devbox --host devbox.example --transport ssh; get link | to json");
    run.assert_success();
    let row = single_row(&run);
    assert_eq!(
        text(&row, "name"),
        "devbox",
        "ono.link/1: the definition is listed under its name, got {row:?}"
    );
    assert_eq!(
        text(&row, "transport"),
        "ssh",
        "ono.link/1: the definition remembers its transport, got {row:?}"
    );
    assert_ne!(
        text(&row, "state"),
        "connected",
        "ono.link/1 `state`: a definition that was never established is not usable now, got {row:?}"
    );
}

#[test]
fn should_show_which_host_a_link_definition_points_at() {
    // remote.yaml `add link --host`: the definition points at a host that may differ from its
    // name (`add link prod-db --host 10.4.2.11`), and `get link` must not hide where it goes.
    let run = ono("add link devbox --host devbox.example --transport ssh; get link | to json");
    run.assert_success();
    assert!(
        serialised(&single_row(&run)).contains("devbox.example"),
        "remote.yaml: the host the definition points at is part of the record, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_change_a_link_definition_when_set() {
    let run = ono("add link devbox --host devbox.example --transport ssh; \
         set link devbox --transport local; \
         get link | to json");
    run.assert_success();
    let row = single_row(&run);
    assert_eq!(
        text(&row, "transport"),
        "local",
        "remote.yaml `set link --transport`: the definition changed, got {row:?}"
    );
}

#[test]
fn should_report_the_change_when_a_link_definition_is_set() {
    let run = ono("add link devbox --host devbox.example --transport ssh; \
         set link devbox --transport local | to json");
    run.assert_success();
    let row = single_row(&run);
    assert_eq!(
        text(&row, "status"),
        "success",
        "spec §11.5: the mutation reports one ActionResult, got {row:?}"
    );
    assert_eq!(
        row["changed"].as_bool(),
        Some(true),
        "spec §11.5: the definition did change, got {row:?}"
    );
}

#[test]
fn should_rename_a_link_definition() {
    let run = ono("add link devbox --host devbox.example --transport ssh; \
         rename link devbox prodbox; \
         get link | to json");
    run.assert_success();
    assert_eq!(
        names(&rows(&run)),
        ["prodbox"],
        "remote.yaml `rename link`: only the new name remains, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_forget_a_link_definition_when_removed() {
    let run = ono("add link devbox --host devbox.example --transport ssh; \
         remove link devbox; \
         get link | to json");
    run.assert_success();
    assert!(
        rows(&run).is_empty(),
        "remote.yaml `remove link`: the definition is gone, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_report_the_teardown_when_an_established_link_is_removed() {
    // remote.yaml: "Remove a link definition, tearing the link down if it is established."
    let run = ono(&format!("{LINK}; remove link testbox | to json"));
    run.assert_success();
    let row = single_row(&run);
    assert_eq!(
        text(&row, "status"),
        "success",
        "spec §11.5: the teardown is one ActionResult, got {row:?}"
    );
    assert_eq!(
        text(&row, "operation"),
        "ono.link.remove",
        "spec §11.5: `operation` is the command id, got {row:?}"
    );
}

#[test]
fn should_refuse_to_enter_a_link_that_was_removed() {
    let run = ono(&format!("{LINK}; remove link testbox; enter link testbox"));
    assert!(
        !run.status().is_success(),
        "remote.yaml `remove link`: a torn-down link cannot be entered, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E"),
        "spec §43: the refusal is structured, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_pop_the_link_frame_when_detaching() {
    // Spec §9.1: "Detach active link/context." The frame goes; the stack is back on the ground.
    let run = ono(&format!(
        "{LINK}; enter link testbox; detach link testbox; get context | to json"
    ));
    run.assert_success();
    let frames = rows(&run);
    assert!(
        frames
            .iter()
            .all(|frame| frame["kind"].as_str() != Some("link")),
        "spec §9.1 `detach link`: the link's frame is no longer on the stack, got {frames:?}"
    );
}

#[test]
fn should_keep_the_link_when_detaching() {
    // remote.yaml: "Detach from an active link without tearing it down" — unlike `remove link`,
    // the link stays in the table for a later `enter link`.
    let run = ono(&format!(
        "{LINK}; enter link testbox; detach link testbox; get link | to json"
    ));
    run.assert_success();
    assert_eq!(
        names(&rows(&run)),
        ["testbox"],
        "remote.yaml `detach link`: detaching is not removing, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_answer_again_from_a_detached_link_when_it_is_entered_again() {
    let run = ono(&format!(
        "{LINK}; enter link testbox; \
         detach link testbox | select status | to json; \
         enter link testbox; get process | where pid == 1 | select pid | to json"
    ));
    run.assert_success();
    assert!(
        run.stdout().contains(r#"[{"status":"success"}]"#),
        "spec §11.5: detaching reports success, got {:?}",
        run.stdout()
    );
    assert!(
        run.stdout().contains(r#"[{"pid":1}]"#),
        "remote.yaml `detach link`: the link can be entered again afterwards, got {:?}",
        run.stdout()
    );
}

// --- agentless mode ---------------------------------------------------------------------------

#[test]
fn should_keep_the_agentless_mode_visible_in_the_link_table() {
    // Spec §21.3: "Fallback MUST be visible because semantics and performance may differ." A
    // link asked for in agentless mode says so wherever the link is described.
    let run = ono("link host testbox --transport local --agentless; get link | to json");
    run.assert_success();
    assert!(
        serialised(&single_row(&run)).contains("agentless"),
        "spec §21.3: the agentless mode is visible in the link record, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_explain_that_a_query_runs_in_agentless_mode() {
    let run = ono("link host testbox --transport local --agentless; \
         enter link testbox; explain get process");
    run.assert_success();
    assert!(
        run.stdout().contains("agentless"),
        "spec §21.3: the plan says the provider set is the agentless fallback's, got {:?}",
        run.stdout()
    );
}

// --- mutations across a link ------------------------------------------------------------------

#[test]
fn should_answer_a_missing_remote_target_the_way_the_local_side_does() {
    // ADR-0036: a linked machine is the same code path as the local one — "nothing above the
    // registry can tell". Naming a process that does not exist must therefore end the same way
    // on both sides, whatever way the process family settles on.
    let local = ono("kill process 4000000 | to json");
    let remote = ono(&format!(
        "{LINK}; enter link testbox; kill process 4000000 | to json"
    ));
    assert_eq!(
        remote.status().code(),
        local.status().code(),
        "ADR-0036: the link does not change the outcome of a mutation; local {:?}, remote {:?}",
        local.output(),
        remote.output()
    );
    // `duration` is measured wall-clock time (action-result.v1 requires it), so it is the one
    // field two runs can never agree on; everything else in the row must.
    let timeless = |run: &ono_testkit::Run| -> Vec<Value> {
        rows(run)
            .into_iter()
            .map(|mut row| {
                if let Value::Mapping(map) = &mut row {
                    map.remove("duration");
                }
                row
            })
            .collect()
    };
    assert_eq!(
        timeless(&remote),
        timeless(&local),
        "ADR-0036: the same records come back across the link; local {:?}, remote {:?}",
        local.output(),
        remote.output()
    );
}

#[test]
fn should_fail_the_run_when_a_remote_mutation_fails() {
    // Spec §16.5 + ADR-0006: the far side's refusal arrives as a `failed` ActionResult with the
    // remote's own error, and a failed row makes the exit status 1.
    let run = ono(&format!(
        "{LINK}; enter link testbox; stop process 1 | to json"
    ));
    let row = single_row(&run);
    assert_eq!(
        text(&row, "status"),
        "failed",
        "spec §16.5: the remote's refusal is a failed row, got {row:?}"
    );
    assert!(
        serialised(&row).contains("io.permission_denied"),
        "errors.yaml E0302: an unprivileged agent cannot signal pid 1, got {row:?}"
    );
    assert_eq!(
        run.status().code(),
        1,
        "ADR-0006: a failed ActionResult row fails the run, got {:?}",
        run.output()
    );
}

#[test]
fn should_explain_the_remote_context_of_a_mutation() {
    // Spec §42.2: while connected, the plan of a mutation shows the execution context —
    // `link prod-app (remote)` — so the risk of acting on the wrong machine is inspectable.
    let run = ono(&format!(
        "{LINK}; enter link testbox; explain stop process 1"
    ));
    run.assert_success();
    let plan = run.stdout();
    assert!(
        plan.contains("testbox"),
        "spec §42.2: the plan names the link the mutation runs over, got {plan}"
    );
    assert!(
        plan.contains("remote"),
        "spec §42.2 + §17.1: the risk descriptor says the mutation is remote, got {plan}"
    );
}

#[test]
fn should_explain_the_effect_of_a_remote_mutation() {
    // Spec §42.2: `operation signal TERM` — the plan says what the mutation does, not only
    // which capability it needs.
    let run = ono(&format!(
        "{LINK}; enter link testbox; explain stop process 1"
    ));
    run.assert_success();
    let plan = run.stdout();
    assert!(
        plan.contains("TERM"),
        "spec §42.2: the plan names the signal `stop` sends, got {plan}"
    );
    assert!(
        plan.contains("privilege"),
        "spec §42.2: the plan names the privilege the mutation needs, got {plan}"
    );
}

// --- host records: add, set, remove -----------------------------------------------------------

#[test]
fn should_record_a_host_in_the_shells_own_source() {
    // remote.yaml: "Record a host in a configured host source." The shell's own source lives
    // under its config directory (AGENTS.md §3: `~/.config/ono/`), so the record outlives the
    // invocation that made it.
    let home = scratch();
    let added = ono_at_home(&home, "add host devbox --address 10.4.2.11 | to json");
    added.assert_success();
    let row = single_row(&added);
    assert_eq!(
        text(&row, "status"),
        "success",
        "spec §11.5: recording a host is one ActionResult, got {row:?}"
    );
    assert!(
        any_file_mentions(home.path(), "devbox"),
        "remote.yaml `add host`: the host source under the config directory changed"
    );
    let listed = ono_at_home(&home, "get host devbox | to json");
    listed.assert_success();
    let host = single_row(&listed);
    assert!(
        serialised(&host).contains("10.4.2.11"),
        "ono.host/1: the recorded address comes back in a later invocation, got {host:?}"
    );
}

#[test]
fn should_modify_a_recorded_host() {
    let home = scratch();
    ono_at_home(&home, "add host devbox --address 10.4.2.11").assert_success();
    let set = ono_at_home(&home, "set host devbox --address 10.4.2.12 | to json");
    set.assert_success();
    assert_eq!(
        text(&single_row(&set), "status"),
        "success",
        "spec §11.5: modifying a host is one ActionResult, got {:?}",
        set.stdout()
    );
    let listed = ono_at_home(&home, "get host devbox | to json");
    listed.assert_success();
    let host = serialised(&single_row(&listed));
    assert!(
        host.contains("10.4.2.12") && !host.contains("10.4.2.11"),
        "remote.yaml `set host --address`: the new address replaced the old one, got {host}"
    );
}

#[test]
fn should_remove_a_recorded_host() {
    let home = scratch();
    ono_at_home(&home, "add host devbox --address 10.4.2.11").assert_success();
    let removed = ono_at_home(&home, "remove host devbox | to json");
    removed.assert_success();
    assert_eq!(
        text(&single_row(&removed), "status"),
        "success",
        "spec §11.5: removing a host is one ActionResult, got {:?}",
        removed.stdout()
    );
    let listed = ono_at_home(&home, "get host | to json");
    listed.assert_success();
    assert!(
        rows(&listed).is_empty(),
        "remote.yaml `remove host`: the host is gone from its source, got {:?}",
        listed.stdout()
    );
    assert!(
        !any_file_mentions(home.path(), "devbox"),
        "remote.yaml `remove host`: the source file no longer mentions the host"
    );
}
