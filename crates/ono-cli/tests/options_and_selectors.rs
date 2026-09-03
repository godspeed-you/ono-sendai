//! Outcome tests for the options the shell accepts and ignores, and the selectors that do not
//! resolve — the two silent failure classes of `docs/STATE.md` ("Provider options are silently
//! ignored") and the wiki's *What is not built yet*.
//!
//! Contracts: `docs/spec/commands/process.yaml` (`get process --user/--tree`), `file.yaml`
//! (`find file --name/--depth/--kind/--follow-symlinks`), `storage.yaml` (`get filesystem
//! --mounted`), `network.yaml` (`trace socket --port`, `trace connection --remote`, `get socket`),
//! `data.yaml` (`to --human`), `identity.yaml` (`get user <uid>`, `get group <gid>`). Schemas:
//! `ono.process/1`, `ono.file/1`, `ono.filesystem/1`, `ono.socket/1`, `ono.endpoint/1`,
//! `ono.user/1`, `ono.group/1`, `ono.graph/1`. Narrative: spec §6.1 (selectors resolve by id),
//! §13.4 (human formatting), §22.3 (the useful traces), §28.4/§41.2 (`where local.address …`
//! reaches into an endpoint), §33.5 (canonical values unless a human form is requested).
//!
//! Every test asserts what the user sees through `| to json` or on stdout — never which stage
//! honoured the option (AGENTS.md §11). Each one pins an option the shell honours or a
//! selector that resolves, against sockets and files the test itself created rather than
//! against whatever the host happens to be running.
//!
//! Not covered here: `get route --table/--family` and `format table --max-rows`, which were
//! already honoured when this suite was written, so a test for them would have proved nothing
//! about the two failure classes above.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::net::TcpListener;
use std::time::Duration;

use ono_testkit::{Scratch, Shell, SkipReason, scratch};
use serde_yaml_ng::Value;

mod support;
use support::text;

/// Runs a one-liner and returns the finished run.
fn ono(script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .timeout(Duration::from_secs(60))
        .run()
}

/// Parses the JSON document `to json` wrote as the stream's values.
fn rows(run: &ono_testkit::Run) -> Vec<Value> {
    let text = run.stdout().trim().to_owned();
    let document: Value = serde_yaml_ng::from_str(&text).unwrap_or_else(|error| {
        panic!("`to json` must emit a JSON document, got {text:?}: {error}")
    });
    document
        .as_sequence()
        .unwrap_or_else(|| {
            panic!("spec §33.5: `to json` emits the stream as an array, got {text:?}")
        })
        .clone()
}

/// The single number a `count | to json` pipeline emits.
fn count(script: &str) -> i64 {
    let run = ono(script);
    run.assert_success();
    let mut rows = rows(&run);
    assert_eq!(
        rows.len(),
        1,
        "`count` emits one value, got {:?}",
        run.stdout()
    );
    rows.remove(0).as_i64().unwrap_or_else(|| {
        panic!(
            "`count | to json` must emit an integer, got {:?}",
            run.stdout()
        )
    })
}

/// The login name of the user running the tests, from the system rather than an env var that a
/// harness may not set.
fn current_user() -> String {
    let output = std::process::Command::new("id")
        .arg("-un")
        .output()
        .expect("`id -un` names the current user");
    String::from_utf8(output.stdout)
        .expect("a login name is text")
        .trim()
        .to_owned()
}

/// A small tree for `find file`: two files at the root, one nested, and a symlink to the
/// nested directory.
///
/// ```text
/// <root>/a.md
/// <root>/b.txt
/// <root>/sub/c.md
/// <root>/link -> sub
/// ```
fn tree() -> Scratch {
    let dir = scratch();
    dir.write("a.md", "a\n");
    dir.write("b.txt", "b\n");
    dir.write("sub/c.md", "c\n");
    std::os::unix::fs::symlink(dir.path().join("sub"), dir.path().join("link"))
        .expect("a symlink inside the scratch directory");
    dir
}

/// A listener the test owns, so the sockets under test are the test's own and not the
/// machine's services. Kept alive by the caller for the shell's whole run.
fn listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener on a free port");
    let port = listener.local_addr().expect("a bound address").port();
    (listener, port)
}

/// An established loopback connection the test owns, so `trace connection --remote 127.0.0.1`
/// has something to answer that this test put there.
///
/// All three sockets are returned and the caller holds them: the listener, the dialling end and
/// the accepted end. Dropping any of them ends the connection the assertions are about.
fn connection() -> (TcpListener, std::net::TcpStream, std::net::TcpStream) {
    let (listener, port) = listener();
    let dialled = std::net::TcpStream::connect(("127.0.0.1", port))
        .expect("a loopback connection to a listener this test just bound");
    let (accepted, _) = listener
        .accept()
        .expect("the listener accepts its own dialler");
    (listener, dialled, accepted)
}

/// The node a graph is rooted at — the object `trace` resolved before it walked anything.
///
/// `graph.v1` names it: "the object the trace started from". The nodes it reaches are the
/// relationships, which is what a trace is for; the subject is the root and nothing else.
fn root_of<'a>(graph: &'a Value, script: &str) -> &'a Value {
    let root = &graph["root"];
    graph["nodes"]
        .as_sequence()
        .unwrap_or_else(|| panic!("graph.v1: `nodes` is a list, got {graph:?}"))
        .iter()
        .find(|node| node["id"] == *root)
        .unwrap_or_else(|| {
            panic!("graph.v1: `{script}` roots the graph at one of its own nodes, got {graph:?}")
        })
}

// --- get process --user / --tree ------------------------------------------------------------

#[test]
fn should_return_only_root_processes_when_user_root_is_requested() {
    // process.yaml: `--user` "Restrict to processes of one user". Unprivileged, root's processes
    // are still enumerable through /proc, so the restricted stream is non-empty — and nothing in
    // it may belong to anyone else.
    let strangers =
        count(r#"get process --user root | where user.name != "root" | count | to json"#);
    assert_eq!(
        strangers, 0,
        "process.yaml: `get process --user root` restricts the stream to root's processes; \
         {strangers} row(s) belong to another user"
    );
    let owned = count("get process --user root | count | to json");
    assert!(
        owned >= 1,
        "pid 1 belongs to root and is visible in /proc, so the restricted stream is not empty"
    );
}

#[test]
fn should_return_only_the_callers_processes_when_user_names_the_caller() {
    let me = current_user();
    let strangers = count(&format!(
        r#"get process --user {me} | where user.name != "{me}" | count | to json"#
    ));
    assert_eq!(
        strangers, 0,
        "process.yaml: `get process --user {me}` restricts the stream to {me}'s processes; \
         {strangers} row(s) belong to another user"
    );
    let owned = count(&format!("get process --user {me} | count | to json"));
    assert!(
        owned >= 1,
        "the shell answering the query runs as {me}, so the restricted stream is not empty"
    );
}

#[test]
fn should_nest_children_under_their_parents_when_tree_is_requested() {
    // process.yaml: `--tree` "Emit the parent/child structure rather than a flat stream." Read
    // as: the stream carries the roots, and every other process is reachable beneath its parent.
    // The test process is a descendant of pid 1 and an ancestor of the shell, so it must appear
    // in the document — and not as a top-level row.
    let flat = count("get process | count | to json");
    let run = ono("get process --tree | to json");
    run.assert_success();
    let roots = rows(&run);
    // Processes come and go between the two runs, so the comparison leaves room: a tree of a
    // Linux process table has a handful of roots (pid 1, kthreadd), never half the table.
    assert!(
        (roots.len() as i64) < flat / 2,
        "process.yaml: `--tree` emits the structure, not the flat stream — {flat} processes \
         cannot all be roots, got {} top-level rows",
        roots.len()
    );

    let me = i64::from(std::process::id());
    let at_top_level = roots.iter().any(|row| row["pid"].as_i64() == Some(me));
    assert!(
        !at_top_level,
        "the test process has a parent, so under `--tree` it is not a top-level row"
    );
    assert!(
        run.stdout().contains(&format!("\"pid\":{me}")),
        "`--tree` restructures the stream but drops nothing: the test process (pid {me}) must \
         still be reachable beneath its parent"
    );
}

// --- find file --name / --depth / --kind / --follow-symlinks ----------------------------------

#[test]
fn should_return_only_matching_names_when_find_file_has_a_name_glob() {
    let dir = tree();
    let run = ono(&format!(
        r#"find file {} --name "*.md" | select name | to json"#,
        dir.path().display()
    ));
    run.assert_success();
    let mut names: Vec<String> = rows(&run).iter().map(|row| text(row, "name")).collect();
    names.sort();
    assert_eq!(
        names,
        ["a.md", "c.md"],
        "file.yaml: `--name` matches entry names against the glob, so only the two `.md` files \
         are found (`b.txt`, `sub`, `link` and the root are not)"
    );
}

#[test]
fn should_stop_descending_when_find_file_has_a_depth() {
    let dir = tree();
    let run = ono(&format!(
        "find file {} --depth 1 | select path | to json",
        dir.path().display()
    ));
    run.assert_success();
    let paths: Vec<String> = rows(&run).iter().map(|row| text(row, "path")).collect();
    assert!(
        paths.iter().any(|path| path.ends_with("/a.md")),
        "a direct entry of the root is within depth 1, got {paths:?}"
    );
    assert!(
        !paths.iter().any(|path| path.ends_with("/sub/c.md")),
        "file.yaml: `--depth 1` is the maximum depth to descend, so `sub/c.md` at depth 2 is \
         not found, got {paths:?}"
    );
}

#[test]
fn should_return_only_directories_when_find_file_has_a_kind() {
    let dir = tree();
    let run = ono(&format!(
        "find file {} --kind dir | select name kind | to json",
        dir.path().display()
    ));
    run.assert_success();
    let found = rows(&run);
    assert!(
        found.iter().any(|row| text(row, "name") == "sub"),
        "`sub` is a directory beneath the root, so `--kind dir` finds it, got {found:?}"
    );
    let others: Vec<String> = found
        .iter()
        .filter(|row| text(row, "kind") != "dir")
        .map(|row| text(row, "name"))
        .collect();
    assert!(
        others.is_empty(),
        "file.yaml: `--kind dir` restricts the walk to one `ono.file/1` kind; found {others:?}"
    );
}

#[test]
fn should_descend_through_a_symlinked_directory_when_follow_symlinks_is_set() {
    let dir = tree();
    let run = ono(&format!(
        "find file {} --follow-symlinks | select path | to json",
        dir.path().display()
    ));
    run.assert_success();
    let paths: Vec<String> = rows(&run).iter().map(|row| text(row, "path")).collect();
    assert!(
        paths.iter().any(|path| path.ends_with("/link/c.md")),
        "file.yaml: `--follow-symlinks` follows symlinks while walking, so `c.md` is also found \
         through `link -> sub`, got {paths:?}"
    );
}

#[test]
fn should_list_a_directory_under_every_symlink_that_reaches_it_when_follow_symlinks_is_set() {
    // ADR-0083 §3 / ADR-0120: following symlinks lists a directory by every name it is reached
    // by. A guard that remembered every directory ever visited let whichever of `aaa`, `sub`
    // and `zzz` readdir yielded first win — and readdir order differs between filesystems.
    let dir = scratch();
    dir.write("sub/c.md", "c\n");
    for link in ["aaa", "zzz"] {
        std::os::unix::fs::symlink(dir.path().join("sub"), dir.path().join(link))
            .expect("a symlink inside the scratch directory");
    }
    let run = ono(&format!(
        "find file {} --follow-symlinks | select path | to json",
        dir.path().display()
    ));
    run.assert_success();
    let paths: Vec<String> = rows(&run).iter().map(|row| text(row, "path")).collect();
    for reached in ["/aaa/c.md", "/sub/c.md", "/zzz/c.md"] {
        assert!(
            paths.iter().any(|path| path.ends_with(reached)),
            "`--follow-symlinks` lists `c.md` under every name that reaches `sub`, but \
             {reached} is missing from {paths:?}"
        );
    }
}

#[test]
fn should_cut_a_symlink_cycle_when_follow_symlinks_is_set() {
    // ADR-0120: a cycle is a directory that is one of its own ancestors on the current walk
    // path. `sub/back -> ..` is listed as the entry it is and never descended into.
    let dir = scratch();
    dir.write("sub/c.md", "c\n");
    std::os::unix::fs::symlink(dir.path(), dir.path().join("sub").join("back"))
        .expect("a symlink inside the scratch directory");
    let run = ono(&format!(
        "find file {} --follow-symlinks | select path | to json",
        dir.path().display()
    ));
    run.assert_success();
    let paths: Vec<String> = rows(&run).iter().map(|row| text(row, "path")).collect();
    assert!(
        paths.iter().any(|path| path.ends_with("/sub/back")),
        "the cycling link is still an entry, got {paths:?}"
    );
    assert!(
        !paths.iter().any(|path| path.contains("/sub/back/")),
        "nothing is listed beneath the link that closes the cycle, got {paths:?}"
    );
}

// --- get filesystem --mounted ----------------------------------------------------------------

#[test]
fn should_return_only_unmounted_filesystems_when_mounted_is_false() {
    // storage.yaml: `--mounted` "Restrict to filesystems that are or are not currently mounted";
    // language.yaml spells a bool as `true`/`false`. filesystem.v1: `target` is "the mount point,
    // or null when the filesystem is not currently mounted". Whatever the machine has, nothing
    // with a mount point may be in the "not mounted" answer.
    let mounted = count("get filesystem --mounted false | where target != null | count | to json");
    assert_eq!(
        mounted, 0,
        "storage.yaml: `--mounted false` restricts the stream to filesystems that are not \
         mounted, yet {mounted} row(s) have a mount point"
    );
}

// --- trace socket --port / trace connection --remote -----------------------------------------

/// The port the root of a `trace socket` graph names, read from the root node's endpoint.
fn traced_local_port(run: &ono_testkit::Run) -> Option<i64> {
    let mut graphs = rows(run);
    assert_eq!(
        graphs.len(),
        1,
        "`trace` yields one `ono.graph/1`, got {:?}",
        run.stdout()
    );
    let graph = graphs.remove(0);
    graph["nodes"]
        .as_sequence()
        .unwrap_or_else(|| panic!("graph.v1: `nodes` is a list, got {graph:?}"))
        .iter()
        .find(|node| node["id"] == graph["root"])
        .and_then(|node| node["value"]["local"]["port"].as_i64())
}

#[test]
fn should_trace_the_socket_on_the_requested_port_when_port_is_given() {
    // Two listeners, two traces: a shell that ignores `--port` and traces whichever socket it
    // finds first can satisfy at most one of them.
    let (_first, first_port) = listener();
    let (_second, second_port) = listener();
    for port in [first_port, second_port] {
        let run = ono(&format!("trace socket --port {port} | to json"));
        run.assert_success();
        assert_eq!(
            traced_local_port(&run),
            Some(i64::from(port)),
            "network.yaml / spec §22.3: `trace socket --port {port}` traces the socket on that \
             port, so the graph's root has `local.port` {port} (ono.endpoint/1), got {:?}",
            run.stdout()
        );
    }
}

#[test]
fn should_trace_nothing_else_when_no_connection_has_the_requested_remote() {
    // network.yaml / spec §22.3: `--remote` names the peer the trace is about, and the subject
    // of a graph is its root — "the object the trace started from" (graph.v1). What the walk
    // reaches from there is relationships, and reaching a process's other sockets is the whole
    // point of a trace, so the assertion is about the root and never about every node.
    //
    // The fixture owns an established loopback connection, which makes both halves say
    // something: the peer that exists is one this test created, and the peer that does not is
    // TEST-NET-1 (RFC 5737), which is never routed. Without the fixture the negative half passed
    // on any host whose socket table happened to be empty of the address, which is a fact about
    // the host and not about the filter (AGENTS.md §11).
    let (_listener, _dialled, _accepted) = connection();

    // The peer that exists: the filter finds it, and the graph is rooted at it.
    let present = "trace connection --remote 127.0.0.1 | to json";
    let found = ono(present);
    found.assert_success();
    for graph in rows(&found) {
        assert_eq!(
            root_of(&graph, present)["value"]["remote"]["address"].as_str(),
            Some("127.0.0.1"),
            "network.yaml / spec §22.3: `--remote 127.0.0.1` traces a connection with that peer, \
             and the fixture is holding one open; the graph is rooted at {:?}",
            graph["root"]
        );
    }

    // The peer that does not: either a structured not-found, or a graph rooted at a connection
    // with that peer. A graph rooted at some other connection is the one answer that is wrong,
    // and it is the answer an ignored `--remote` gives.
    let absent = "trace connection --remote 192.0.2.1 | to json";
    let run = ono(absent);
    if !run.status().is_success() {
        assert!(
            run.stderr().contains("Ono-Sendai-E0102") || run.stderr().contains("Ono-Sendai-E0301"),
            "a trace with nothing to trace fails with a structured not-found (errors.yaml \
             resolve.target_not_found / io.not_found), got {:?}",
            run.stderr()
        );
        return;
    }
    for graph in rows(&run) {
        assert_eq!(
            root_of(&graph, absent)["value"]["remote"]["address"].as_str(),
            Some("192.0.2.1"),
            "network.yaml / spec §22.3: `trace connection --remote 192.0.2.1` traces only the \
             connections with that peer, and this machine holds none; the graph is rooted at \
             {:?}",
            graph["root"]
        );
    }
}

// --- to … --human ------------------------------------------------------------------------------

/// A sparse file of 1.2 MiB — the byte size whose human form spec §13.4 fixes at two decimals
/// (`1.20 GiB` for 1288490188), scaled down so the fixture costs nothing to create.
fn sparse_megabyte_file(dir: &Scratch) -> std::path::PathBuf {
    let path = dir.write("big.bin", "");
    std::fs::File::options()
        .write(true)
        .open(&path)
        .expect("the fixture file was just written")
        .set_len(1_258_292)
        .expect("a sparse file needs no disk space");
    path
}

fn assert_human_megabytes(rendered: &str, what: &str) {
    assert!(
        rendered.starts_with("1.2") && rendered.ends_with("MiB"),
        "spec §13.4 / data.yaml: `--human` emits the display form of a byte size (`1.20 MiB` for \
         1258292 bytes) instead of the canonical integer, but {what} shows {rendered:?}"
    );
}

#[test]
fn should_render_a_byte_size_for_a_human_when_to_json_has_human() {
    let dir = scratch();
    let path = sparse_megabyte_file(&dir);
    let run = ono(&format!(
        "get file {} | select size | to json --human",
        path.display()
    ));
    run.assert_success();
    let mut rows = rows(&run);
    assert_eq!(rows.len(), 1, "one file, one row, got {:?}", run.stdout());
    let size = rows.remove(0)["size"].clone();
    let rendered = size.as_str().map(str::to_owned).unwrap_or_else(|| {
        panic!("data.yaml: with `--human` the byte size is its display text, got {size:?}")
    });
    assert_human_megabytes(&rendered, "`to json --human`");
}

#[test]
fn should_render_a_byte_size_for_a_human_when_to_text_has_human() {
    let dir = scratch();
    let path = sparse_megabyte_file(&dir);
    let run = ono(&format!(
        "get file {} | to text --field size --human",
        path.display()
    ));
    run.assert_success();
    assert_human_megabytes(run.stdout().trim(), "`to text --field size --human`");
}

// --- numeric selectors -------------------------------------------------------------------------

#[test]
fn should_resolve_a_user_by_uid_when_the_selector_is_numeric() {
    // identity.yaml: `get user` has a `uid: int` selector beside `name`, and spec §6.1 resolves
    // a selector to the one object it names — never to nothing.
    let run = ono("get user 0 | select name | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"name":"root"}]"#,
        "identity.yaml: the `uid` selector resolves account 0, which is root on every Linux"
    );
}

#[test]
fn should_resolve_a_group_by_gid_when_the_selector_is_numeric() {
    let run = ono("get group 0 | select name | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"name":"root"}]"#,
        "identity.yaml: the `gid` selector resolves group 0, which is root on every Linux"
    );
}

// --- endpoint fields in predicates -------------------------------------------------------------

#[test]
fn should_resolve_an_endpoint_field_in_a_predicate_when_filtering_sockets_by_local_port() {
    // socket.v1: `local` is an `ono.endpoint/1` with a `port`; spec §41.2 writes
    // `where local.address not in […]`, so a predicate reaches into the endpoint. The test's own
    // listener is the one socket on its port.
    let (_listener, port) = listener();
    let run = ono(&format!(
        "get socket | where local.port == {port} | select local | to json"
    ));
    run.assert_success();
    assert!(
        !run.stderr().contains("Ono-Sendai-E0201"),
        "spec §28.4: `local` is an endpoint record, not null — reading `local.port` is not a \
         type error, got {:?}",
        run.stderr()
    );
    let found = rows(&run);
    assert_eq!(
        found.len(),
        1,
        "exactly one socket — the test's listener — is bound to port {port}, got {found:?}"
    );
    assert_eq!(
        found[0]["local"]["port"].as_i64(),
        Some(i64::from(port)),
        "the row that passed the predicate is the listener, got {found:?}"
    );
}

// --- what running the shell showed, pinned so it cannot regress silently (B-harn-6) -------------

#[test]
fn should_leave_out_the_processes_the_named_user_does_not_own() {
    // The other half of `--user`: the sibling test proves nothing foreign gets *in*, which a
    // provider that answered with an empty stream would also satisfy, and which says nothing
    // about whether the option narrows at all. This one proves there was something to leave out
    // — the shell's own process, which root does not own. Two single runs, and the conclusion
    // needs no arithmetic across them: an unfiltered stream that holds a non-root process and a
    // filtered one that holds none is a filter that narrowed.
    let me = current_user();
    if me == "root" {
        // Running as root there may be no other user's process to leave out, and inventing one
        // would need privileges the suite must not assume.
        ono_testkit::skipped(
            SkipReason::MissingPrivilege,
            "running as root, where there may be no other user's process for the filter to leave \
             out",
        );
        return;
    }
    let strangers = count(r#"get process | where user.name != "root" | count | to json"#);
    assert!(
        strangers >= 1,
        "the shell answering this query runs as {me}, so the unfiltered stream holds at least \
         one process root does not own"
    );
    let survivors =
        count(r#"get process --user root | where user.name != "root" | count | to json"#);
    assert_eq!(
        survivors, 0,
        "process.yaml: `--user root` narrows the stream, so none of the {strangers} process(es) \
         root does not own survives it"
    );
}

#[test]
fn should_trace_a_connection_even_when_another_one_carries_no_identity() {
    // The kernel reports no socket inode for a `TIME_WAIT` connection, and `ono.socket/1`
    // identifies a socket by its inode — so such a row has no identity at all. Whichever
    // connection the provider happens to yield first, `trace` has to answer about one it *can*
    // relate to; refusing the whole graph because the first row of the match is unidentifiable
    // is a failure the user cannot act on (spec §16.5, §22.1).
    //
    // The `TIME_WAIT` row is created here rather than waited for: closing the initiating side of
    // a loopback connection leaves one behind for a minute, which is exactly the state that made
    // this fail on a machine that had been running the suite.
    {
        let (listener, port) = listener();
        let client =
            std::net::TcpStream::connect(("127.0.0.1", port)).expect("a loopback connection opens");
        let accepted = listener.accept().expect("the listener accepts it");
        drop(accepted);
        drop(client);
        drop(listener);
    }

    let (listener, port) = listener();
    let client =
        std::net::TcpStream::connect(("127.0.0.1", port)).expect("a loopback connection opens");
    let accepted = listener.accept().expect("the listener accepts it");

    let run = ono("trace connection --remote 127.0.0.1 | to json");
    run.assert_success();
    assert!(
        !rows(&run).is_empty(),
        "a connection to 127.0.0.1 is open, so the trace has something to answer with, got {:?};          stderr: {}",
        run.stdout(),
        run.stderr()
    );

    drop(accepted);
    drop(client);
    drop(listener);
}

#[test]
fn should_trace_the_connection_that_does_have_the_requested_remote() {
    // The sibling test covers the peer nobody is talking to. This is the answer itself: with a
    // loopback connection open, `trace connection --remote 127.0.0.1` returns a graph, and every
    // socket in it is a socket with that peer (network.yaml, spec §22.3).
    let (listener, port) = listener();
    let client =
        std::net::TcpStream::connect(("127.0.0.1", port)).expect("a loopback connection opens");
    let accepted = listener.accept().expect("the listener accepts it");

    let run = ono("trace connection --remote 127.0.0.1 | to json");
    run.assert_success();

    let graphs = rows(&run);
    assert!(
        !graphs.is_empty(),
        "a connection to 127.0.0.1 is open, so the trace has something to answer with, got {:?}",
        run.stdout()
    );
    let mut with_that_peer = 0_usize;
    for graph in &graphs {
        let nodes = graph["nodes"]
            .as_sequence()
            .unwrap_or_else(|| panic!("graph.v1: `nodes` is a list, got {graph:?}"));
        for node in nodes {
            if node["kind"].as_str() != Some("ono.socket/1") {
                continue;
            }
            // A listening socket the connection reached has no peer of its own; a *connected*
            // socket in this graph must be connected to the peer that was asked for.
            match node["value"]["remote"]["address"].as_str() {
                Some("127.0.0.1") => with_that_peer += 1,
                None => {}
                other => panic!(
                    "spec §22.3: `--remote 127.0.0.1` traces only that peer's connections, and \
                     this graph holds one to {other:?}: {node:?}"
                ),
            }
        }
    }
    assert!(
        with_that_peer >= 1,
        "the trace answers about the connection it was asked for, so the graph holds at least \
         one socket whose peer is 127.0.0.1, got {:?}",
        run.stdout()
    );

    drop(accepted);
    drop(client);
}
