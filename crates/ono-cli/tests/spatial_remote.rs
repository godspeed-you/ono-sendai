//! Remote systems as space (v0.4 §19): a linked host is a reachable system root, not an SSH
//! subprocess, and every crossing of that boundary is visible.
//!
//! Narrative: `docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` — §19 (link map,
//! `jump` across links, federated map, cross-host relationships), §6.5 (`jump` records source
//! and destination), §6.6 (`home` returns to the root `SYSTEM` place *for the current host*),
//! §20.1 (the `NavigationStep` schema, with its `scope_crossing`), §20.3 (dead destinations),
//! §3.2 (`SpatialScope`: "Crossing a scope boundary MUST be observable in the navigation trail
//! and prompt/HUD"), §11.4/§11.5 (relation explainability and confidence), §21.1/§21.3 (the
//! prompt carries link/host and marks the remote boundary without relying on colour), §35.2
//! (the six permission states stay distinct), §35.4 (remote traversal honours link capabilities;
//! `jump` never silently opens a connection), §43.7 (the remote test list this file follows
//! item by item), §53 ("Remote roots are explicit spaces; boundary crossing always visible").
//!
//! The fixture is the one that already works: `link host testbox --transport local` spawns this
//! very binary as `ono --agent` over a pipe pair (ADR-0037), so every test here is offline,
//! unprivileged and deterministic. `crates/ono-cli/tests/remote.rs` and `remote_commands.rs`
//! prove the v0.2/v0.3 link family; nothing here repeats it — this file is about the *place*
//! a link becomes.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use ono_process::{Command, Executor, PtySession, WindowSize};
use ono_testkit::Shell;
use ono_testkit::ono;
use serde_yaml_ng::Value;

mod support;
use support::{balanced_end, field, list_at, search};

/// The link every test in this file navigates to. `local` spawns this binary as `ono --agent`.
const LINK: &str = "link host testbox --transport local";

/// The §11.5 confidence vocabulary. A relation that carries anything else is not explainable.
const CONFIDENCE: [&str; 5] = ["exact", "strong", "inferred", "user_declared", "unknown"];

/// Runs with colour switched off, so an assertion about a visible boundary marker is about the
/// text and not about an escape sequence (§21.3: "MUST be visually recognizable even in minimal
/// colorless terminals", "Color MAY reinforce these states but MUST NOT be the sole indicator").
fn ono_colorless(script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .run()
}

/// Whether `text` opens like JSON rather than like a rendered table cell.
///
/// `serde_yaml_ng` parses JSON, and it also parses YAML flow mappings such as the `{name:
/// testbox}` an `ActionResult` table prints in its TARGET column. JSON always quotes its keys,
/// so requiring the opening token to be followed by a quote (or by a closing bracket, or by the
/// start of a JSON scalar inside an array) keeps rendered text out of the document scan.
fn opens_like_json(text: &str) -> bool {
    let mut characters = text.chars();
    let opening = characters.next();
    let next = characters.find(|character| !character.is_whitespace());
    match (opening, next) {
        (Some('{'), Some('"' | '}')) => true,
        (Some('['), Some(character)) => {
            matches!(character, '"' | '{' | '[' | ']' | '-' | 't' | 'f' | 'n')
                || character.is_ascii_digit()
        }
        _ => false,
    }
}

/// Every JSON document a script wrote to stdout, in order.
///
/// A v0.4 script mixes documents with plain lines: `link host` prints a summary line of its own
/// before the spatial command runs, and a script may ask for two views in one run. The scanner
/// takes the documents and ignores everything between them, so a test can compare the *first*
/// `look --json` with the *second* without depending on how the preamble is rendered.
fn documents(run: &ono_testkit::Run) -> Vec<Value> {
    let characters: Vec<char> = run.stdout().chars().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        if matches!(characters[index], '{' | '[')
            && let Some(end) = balanced_end(&characters, index)
            && let text = characters[index..=end].iter().collect::<String>()
            && opens_like_json(&text)
            && let Ok(value) = serde_yaml_ng::from_str::<Value>(&text)
        {
            found.push(value);
            index = end + 1;
            continue;
        }
        index += 1;
    }
    found
}

/// The single JSON document the script wrote, or a failure that shows what it wrote instead.
fn document(run: &ono_testkit::Run, what: &str) -> Value {
    let mut found = documents(run);
    assert_eq!(
        found.len(),
        1,
        "{what} — expected exactly one JSON document on stdout, got {:?}; stderr: {:?}",
        run.stdout(),
        run.stderr()
    );
    found.remove(0)
}

/// The `n`-th JSON document the script wrote.
fn nth_document(run: &ono_testkit::Run, index: usize, what: &str) -> Value {
    let found = documents(run);
    found.get(index).cloned().unwrap_or_else(|| {
        panic!(
            "{what} — expected at least {} JSON documents on stdout, got {:?}; stderr: {:?}",
            index + 1,
            run.stdout(),
            run.stderr()
        )
    })
}

fn text_at(document: &Value, path: &str, what: &str) -> String {
    field(document, path)
        .as_str()
        .unwrap_or_else(|| {
            panic!("{what} — `{path}` must be a string, got {document:?}");
        })
        .to_owned()
}

/// Reads from the terminal until `needle` appears in everything seen so far, or the budget runs
/// out. Everything read is appended to `seen`, so a failing assertion can print the transcript.
fn wait_for(shell: &mut PtySession, seen: &mut String, needle: &str, budget: Duration) -> bool {
    let mut buffer = [0u8; 8192];
    let deadline = std::time::Instant::now() + budget;
    while std::time::Instant::now() < deadline {
        if let Ok(Some(count)) = shell.read_timeout(&mut buffer, Duration::from_millis(150)) {
            seen.push_str(&String::from_utf8_lossy(&buffer[..count]));
        }
        if seen.contains(needle) {
            return true;
        }
    }
    false
}

fn rendered(value: &Value) -> String {
    serde_yaml_ng::to_string(value).expect("a value serialises")
}

// --- §19.1 the link map -----------------------------------------------------------------------

#[test]
fn should_list_a_linked_host_among_the_places_when_looking_at_the_local_root() {
    // §19.1: at local `SYSTEM`, `look` exposes the available links with their state. §29.1: the
    // structured form works without a TTY.
    let run = ono(&format!("{LINK}; look --json"));
    let view = document(&run, "§29.1: `look --json` must work non-interactively");

    let serialised = rendered(&view);
    assert!(
        serialised.contains("testbox"),
        "§19.1: the local root's view names the links this session holds, got {serialised}"
    );

    let links = list_at(
        &view,
        "links",
        "§19.1: the local root exposes a `links` group",
    );
    let testbox = links
        .iter()
        .find(|link| {
            search(link, "name")
                .and_then(|n| n.as_str().map(str::to_owned))
                .as_deref()
                == Some("testbox")
        })
        .unwrap_or_else(|| panic!("§19.1: `testbox` is one of the links, got {links:?}"));
    assert_eq!(
        text_at(testbox, "state", "§19.1: a link entry carries its state"),
        "connected",
        "§19.1/§35.2: a link that just negotiated is `connected`, and the state is one of the \
         six distinct states of §35.2 — never omitted, got {testbox:?}"
    );
}

// --- §19.2 `jump` across links ----------------------------------------------------------------

#[test]
fn should_give_a_linked_host_a_root_place_distinct_from_the_local_root() {
    // §19.2: `jump prod/web01` "MUST produce a new `SystemPlace` for the remote host". §3.1: the
    // `SpatialId` is what identifies a place; two roots on two hosts are two conceptual objects,
    // so the ids differ even though both are "the SYSTEM root".
    let run = ono(&format!("look --json; {LINK}; jump testbox; look --json"));
    let local = nth_document(&run, 0, "§29.1: the local root view");
    let remote = nth_document(&run, 1, "§19.2: the remote root view after `jump`");

    let local_id = text_at(
        &local,
        "place.id",
        "§3.1: a place carries an opaque `SpatialId`",
    );
    let remote_id = text_at(
        &remote,
        "place.id",
        "§3.1: a place carries an opaque `SpatialId`",
    );
    assert_ne!(
        local_id, remote_id,
        "§19.2: `jump` to a link produces a *new* SystemPlace; the remote root and the local \
         root are different places, not the same id seen twice"
    );

    let scope = rendered(&field(&remote, "place.scope"));
    assert!(
        scope.contains("testbox"),
        "§3.2: the remote root belongs to a RemoteHostScope naming the host, got {scope}"
    );
    assert!(
        !rendered(&field(&local, "place.scope")).contains("testbox"),
        "§3.2: the local root is not in the remote's scope, got {:?}",
        field(&local, "place.scope")
    );
}

#[test]
fn should_announce_the_boundary_in_plain_text_when_jumping_to_a_linked_host() {
    // §53: "Remote roots are explicit spaces; boundary crossing always visible." §21.3: the
    // marker must survive a colourless terminal, so it is text and not an escape sequence.
    let run = ono_colorless(&format!("{LINK}; jump testbox"));
    run.assert_success();
    let seen = format!("{}{}", run.stdout(), run.stderr());
    assert!(
        seen.contains("testbox"),
        "§19.2/§53: `jump` says which host it moved to, got {seen:?}"
    );
    assert!(
        seen.contains("remote") || seen.contains("link"),
        "§53/§21.3: the crossing of a host boundary is stated, not merely implied by a changed \
         name; the answer must say the new place is remote, got {seen:?}"
    );
}

#[test]
fn should_mark_the_remote_host_in_the_prompt_after_a_jump() {
    // §21.1: the prompt's semantic components include `link/host`. §21.3: "Privilege, remote and
    // namespace changes MUST be visually recognizable even in minimal colorless terminals."
    // This is interactive by nature (§43.4), so it is the one PTY test in this file.
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string());
    let mut shell: PtySession = executor
        .run_pty(&command, WindowSize::new(30, 100))
        .expect("a pseudo-terminal");

    let mut seen = String::new();

    assert!(
        wait_for(&mut shell, &mut seen, ">", Duration::from_secs(10)),
        "a prompt"
    );
    shell
        .write_all(format!("{LINK}\n").as_bytes())
        .expect("the terminal accepts the link");
    assert!(
        wait_for(&mut shell, &mut seen, "testbox", Duration::from_secs(10)),
        "the link is made; saw:\n{seen}"
    );
    shell
        .write_all(b"jump testbox\n")
        .expect("the terminal accepts the jump");
    // Drain until the jump has been answered and the shell has drawn its next prompt. The
    // needle never appears, so the budget is what ends the wait — the transcript, not a race,
    // is what the assertion below reads.
    let before = seen.len();
    let _ = wait_for(&mut shell, &mut seen, "\u{0}", Duration::from_secs(5));
    // A bare newline redraws the prompt with nothing echoed after it, so the last prompt line
    // is the prompt itself and not the command the terminal echoed onto it.
    shell.write_all(b"\n").expect("a bare line");
    let _ = wait_for(&mut shell, &mut seen, "\u{0}", Duration::from_secs(3));

    let after_jump = seen[before..].to_owned();
    let prompt = after_jump
        .lines()
        .rev()
        .find(|line| line.contains('>'))
        .unwrap_or_default()
        .to_owned();
    assert!(
        prompt.contains("testbox"),
        "§21.1/§21.3: the prompt carries link/host, so a user returning from a long command \
         knows where the next relative spatial command will operate; the prompt line was \
         {prompt:?}, the whole transcript after the jump was:\n{after_jump}"
    );

    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}

// --- §20.1 the cross-host trail ---------------------------------------------------------------

#[test]
fn should_record_the_host_and_the_scope_crossing_of_every_step_in_the_trail() {
    // §6.5: `jump` "MUST visibly record the source and destination in the trail". §20.1 gives
    // the `NavigationStep` schema, including `scope_crossing`. §3.2: "Crossing a scope boundary
    // MUST be observable in the navigation trail". Together: a cross-host path is unambiguous
    // because each step says which host it happened on.
    let run = ono(&format!(
        "{LINK}; enter compute; jump testbox; enter compute; trail --json"
    ));
    let trail = document(&run, "§29.1: `trail --json` must work non-interactively");
    let steps = trail
        .as_sequence()
        .cloned()
        .unwrap_or_else(|| list_at(&trail, "steps", "§20.1: the trail is a list of steps"));
    assert!(
        steps.len() >= 3,
        "§20.1: three movements were made, so three steps were recorded, got {steps:?}"
    );

    for step in &steps {
        for required in ["from", "to", "movement", "timestamp"] {
            assert!(
                search(step, required).is_some_and(|value| !value.is_null()),
                "§20.1: every NavigationStep carries `{required}`, got {step:?}"
            );
        }
        let hosts = rendered(step);
        assert!(
            hosts.contains("testbox") || hosts.contains("local"),
            "§20.1/§19: a step names the host it happened on, so a cross-host path can be read \
             back unambiguously, got {step:?}"
        );
    }

    let jump = steps
        .iter()
        .find(|step| {
            search(step, "movement").and_then(|m| m.as_str().map(str::to_owned)).as_deref()
                == Some("jump")
        })
        .unwrap_or_else(|| panic!("§20.1: `movement` is one of enter|follow|jump|back|up|home and the jump is recorded, got {steps:?}"));
    let crossing = search(jump, "scope_crossing").unwrap_or(Value::Null);
    assert!(
        !crossing.is_null(),
        "§20.1/§3.2: the step that crossed into the remote host carries its `scope_crossing`; \
         a null crossing on a cross-host jump makes the trail lie, got {jump:?}"
    );
    assert!(
        rendered(&crossing).contains("testbox"),
        "§3.2: the boundary that was crossed names the scope it entered, got {crossing:?}"
    );

    // The two `enter compute` steps happened on different hosts and must not read alike.
    let enters: Vec<&Value> = steps
        .iter()
        .filter(|step| {
            search(step, "movement")
                .and_then(|m| m.as_str().map(str::to_owned))
                .as_deref()
                == Some("enter")
        })
        .collect();
    assert_eq!(
        enters.len(),
        2,
        "§20.1: both `enter compute` movements are recorded, got {steps:?}"
    );
    assert_ne!(
        text_at(enters[0], "to", "§20.1: a step's destination"),
        text_at(enters[1], "to", "§20.1: a step's destination"),
        "§19/§43.7: local COMPUTE and testbox's COMPUTE are two places; a trail that gives them \
         the same destination id has merged two hosts"
    );
}

#[test]
fn should_return_home_to_the_local_root_from_a_remote_place() {
    // §6.6: "`home` returns to the root `SYSTEM` place for the current host." Reading chosen:
    // after a `jump` into a link the current host *is* the remote one, so `home` lands on the
    // remote root — and the step that returns across the boundary is the one that must carry a
    // `scope_crossing` (§3.2). `back` is what walks the history back over the link (§6.6).
    //
    // Three `back`s, because the walk is three movements deep: `home` is itself a movement §20.1
    // records and §2.4 makes reversible, so the history is jump, enter, home (ADR-0184).
    let run = ono(&format!(
        "{LINK}; jump testbox; enter compute; home; look --json; back; back; back; look --json"
    ));
    let after_home = nth_document(&run, 0, "§6.6: the view after `home`");
    let after_back = nth_document(&run, 1, "§6.6: the view after walking the trail back");

    assert!(
        rendered(&field(&after_home, "place.scope")).contains("testbox"),
        "§6.6: `home` returns to the root SYSTEM place *for the current host*, which after a \
         jump into a link is the remote one, got {after_home:?}"
    );
    assert!(
        !rendered(&field(&after_back, "place.scope")).contains("testbox"),
        "§6.6/§3.2: `back` follows navigation history, so walking back across the jump leaves \
         the remote scope and the crossing is visible, got {after_back:?}"
    );
}

// --- §43.7 no accidental local/remote identity merge ------------------------------------------

#[test]
fn should_keep_a_remote_process_place_distinct_from_the_local_one_with_the_same_pid() {
    // §43.7 asks explicitly for "no accidental local/remote identity merge". pid 1 exists on
    // both sides of this fixture — the `local` transport's agent runs on this same machine, so
    // the two observations are of literally the same kernel process and every naive identity
    // scheme merges them. §3.2 makes the scope part of what an object *is*, so the two places
    // must keep different SpatialIds; §3.1 keeps the display name out of identity.
    let run = ono(&format!(
        "enter process/1; look --json; home; {LINK}; jump testbox; enter process/1; look --json"
    ));
    let local = nth_document(&run, 0, "§6.3: the local process place");
    let remote = nth_document(&run, 1, "§6.3: the remote process place");

    let local_id = text_at(
        &local,
        "place.id",
        "§3.1: a place carries an opaque `SpatialId`",
    );
    let remote_id = text_at(
        &remote,
        "place.id",
        "§3.1: a place carries an opaque `SpatialId`",
    );
    assert_ne!(
        local_id, remote_id,
        "§43.7: pid 1 on testbox and pid 1 locally are two spatial objects in two scopes; one \
         id for both is exactly the accidental local/remote identity merge the spec forbids"
    );
    assert!(
        rendered(&field(&remote, "place.scope")).contains("testbox"),
        "§3.2: the remote process belongs to testbox's scope, got {remote:?}"
    );
    assert!(
        !rendered(&field(&local, "place.scope")).contains("testbox"),
        "§3.2: the local process does not, got {local:?}"
    );
}

// --- §43.7/§35.2 stale and disconnected link state --------------------------------------------

#[test]
fn should_report_a_place_behind_a_detached_link_as_stale_rather_than_empty() {
    // §35.2: `available`, `empty`, `unknown`, `permission_denied`, `unsupported` and `stale`
    // "MUST remain distinct". §53: "Unknown/denied data? Distinct from empty." §20.3: a dead
    // destination is resolved or announced, never silently skipped. So: standing on a remote
    // place whose link has just been detached, `look` states staleness — either as the place's
    // state or as the structured `spatial.stale` of §40 — and never answers with an empty
    // neighbourhood, which would read as "the host has nothing".
    let run = ono(&format!(
        "{LINK}; jump testbox; detach link testbox; look --json"
    ));
    let stated = format!("{}{}", run.stdout(), run.stderr());
    assert!(
        stated.contains("stale"),
        "§35.2/§43.7: after the link is gone the place is `stale`, stated in the view or as the \
         `spatial.stale` error of §40; got stdout {:?} and stderr {:?}",
        run.stdout(),
        run.stderr()
    );

    if let Some(view) = documents(&run).first() {
        let nearby = rendered(&field(view, "nearby"));
        assert!(
            !nearby.contains("[]") || stated.contains("stale"),
            "§35.2: `files 0` is not an acceptable answer for `files permission denied for 14 \
             process FDs`, and an empty neighbourhood is not an acceptable answer for a link \
             that is gone, got {view:?}"
        );
    }
}

#[test]
fn should_keep_a_detached_link_visible_with_its_state_in_the_link_map() {
    // §19.1's own example keeps a link that is not connected in the map: `home/nas01
    // disconnected  last seen 3h ago`. A link the user made and then detached therefore stays
    // in the local root's view with a state that says so — dropping it silently would be the
    // "empty" answer §35.2 and §53 forbid.
    let run = ono(&format!("{LINK}; detach link testbox; look --json"));
    let view = document(&run, "§29.1: `look --json` at the local root");
    let links = list_at(
        &view,
        "links",
        "§19.1: the local root exposes a `links` group",
    );
    let testbox = links
        .iter()
        .find(|link| {
            search(link, "name")
                .and_then(|n| n.as_str().map(str::to_owned))
                .as_deref()
                == Some("testbox")
        })
        .unwrap_or_else(|| {
            panic!("§19.1: a detached link stays in the link map with its state, got {links:?}")
        });
    let state = text_at(testbox, "state", "§19.1: a link entry carries its state");
    assert!(
        ["disconnected", "stale"].contains(&state.as_str()),
        "§19.1/§35.2: a detached link reads `disconnected` (or `stale`), never `connected` and \
         never absent, got {state:?} in {testbox:?}"
    );
}

// --- §19.4 cross-host relationship confidence -------------------------------------------------

#[test]
fn should_carry_provenance_and_confidence_on_every_relation_that_comes_from_the_far_side() {
    // §19.4: cross-host edges rest on explicit remote evidence or strong multi-sided
    // correlation, and "One-sided observations MAY be displayed but MUST carry the correct
    // confidence". §11.4: every displayed relationship supports inspection and the result
    // includes provider, provenance, confidence and observed_at. §11.5 fixes the vocabulary.
    //
    // A genuinely two-sided cross-host correlation needs the richer fixture of §43.3 (two hosts
    // with a real connection between them), which an unprivileged offline container cannot
    // build; what is asserted here is the honesty requirement that holds for every edge the far
    // side reports: it says who observed it, from where, and how sure it is.
    let run = ono(&format!(
        "{LINK}; jump testbox; enter process/1; near --all | to json"
    ));
    let neighbours = document(&run, "§29.4: `near` is an ordinary object stream")
        .as_sequence()
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "§29.4: `near` streams `SpatialNeighbor`s, got {:?}",
                run.stdout()
            )
        });
    assert!(
        !neighbours.is_empty(),
        "§12: pid 1 has exits — a parent scope, children, a cgroup — on the far side too, got \
         {:?}",
        run.stdout()
    );

    for neighbour in &neighbours {
        let confidence = text_at(
            neighbour,
            "confidence",
            "§11.5: a relation carries a confidence",
        );
        assert!(
            CONFIDENCE.contains(&confidence.as_str()),
            "§11.5: confidence is one of {CONFIDENCE:?}, got {confidence:?} in {neighbour:?}"
        );
        let provenance = rendered(&field(neighbour, "provenance"));
        assert!(
            provenance.contains("testbox"),
            "§19.4/§11.4: an edge observed on the far side names the host whose evidence it \
             rests on — otherwise a remote observation is indistinguishable from a local one, \
             got {neighbour:?}"
        );
        assert!(
            search(neighbour, "provider").is_some_and(|value| !value.is_null()),
            "§11.4: `inspect relation` must be able to answer `provider`, so the edge carries \
             one, got {neighbour:?}"
        );
    }
}

// --- §35.4 remote boundaries ------------------------------------------------------------------

#[test]
fn should_refuse_to_jump_to_a_hostname_that_is_not_a_known_link() {
    // §35.4: "`jump` MUST NOT silently establish arbitrary new network connections merely
    // because a hostname resembles a known place." `.invalid` is reserved by RFC 2606 and never
    // resolves, so a shell that tried anyway would stall in the resolver.
    //
    // What proves nothing was dialled is the *error*, asserted below: a shell that had tried
    // would answer with a resolve or connect failure, not with `spatial.not_found`. The budget
    // is only the guard that turns a hang into a failure, so it is generous rather than tight —
    // it was ten seconds, and this refusal costs eight of CPU in a debug build on a 500-process
    // host, which made the guard a race with the machine rather than with a resolver (S11c).
    let run = Shell::new()
        .args([
            "-c",
            "try { jump prod/web01.invalid } catch e { $e | to json }",
        ])
        .timeout(Duration::from_secs(60))
        .try_run()
        .unwrap_or_else(|error| {
            panic!(
                "§35.4: an unknown host is refused, not dialled — the run must not block on a \
                 resolver or a connect: {error}"
            )
        });
    let caught = document(&run, "§40: the refusal is a structured error value");
    let errors = caught
        .as_sequence()
        .cloned()
        .unwrap_or_else(|| vec![caught.clone()]);
    let error = errors
        .first()
        .unwrap_or_else(|| panic!("§40: one error was caught, got {caught:?}"));
    let name = text_at(error, "name", "§40: an error carries its dotted name");
    assert!(
        name.starts_with("spatial."),
        "§40: every spatial refusal has a name from the spatial taxonomy — \
         `spatial.remote_unavailable` or `spatial.not_found` for a host nothing links to — got \
         {name:?} in {error:?}"
    );
    assert!(
        text_at(error, "code", "§40: an error carries its stable code").starts_with("Ono-Sendai-E"),
        "spec §43: the code is the stable, user-visible identity, got {error:?}"
    );
}

// --- §19.3 the federated map ------------------------------------------------------------------

#[test]
fn should_not_expand_a_remote_graph_into_the_default_root_map() {
    // §19.3: "The default root map SHOULD NOT automatically expand all remote graphs." A link
    // that is held is not an invitation to walk the other machine.
    let run = ono(&format!("{LINK}; map --json"));
    let map = document(
        &run,
        "§22: `map --json` returns a renderer-independent SpatialMap",
    );
    let nodes = list_at(&map, "nodes", "§22: a SpatialMap carries `nodes`");
    let remote: Vec<&Value> = nodes
        .iter()
        .filter(|node| rendered(node).contains("testbox"))
        .collect();
    assert!(
        remote.len() <= 1,
        "§19.3: the default root map may show the link itself, but must not expand testbox's \
         graph into it; got {} nodes mentioning the host: {remote:?}",
        remote.len()
    );
}

#[test]
fn should_show_the_linked_hosts_when_the_federated_map_is_asked_for() {
    // §19.3: `map links` is the explicit request, and it shows the local host beside the linked
    // ones. §22: the answer is a SpatialMap, so it is renderer-independent.
    let run = ono(&format!("{LINK}; map links --json"));
    let map = document(
        &run,
        "§22: `map --json` returns a renderer-independent SpatialMap",
    );
    let nodes = list_at(&map, "nodes", "§22: a SpatialMap carries `nodes`");
    assert!(
        nodes.iter().any(|node| rendered(node).contains("testbox")),
        "§19.3: the federated map shows the linked host, got {nodes:?}"
    );
    assert!(
        nodes.len() >= 2,
        "§19.3: the federated map shows the local root beside the linked host, got {nodes:?}"
    );

    let edges = list_at(&map, "edges", "§22: a SpatialMap carries `edges`");
    let link_edge = edges
        .iter()
        .find(|edge| rendered(edge).contains("testbox"))
        .unwrap_or_else(|| {
            panic!("§19.3: the local host and testbox are joined by an edge, got {edges:?}")
        });
    let confidence = text_at(
        link_edge,
        "confidence",
        "§22: a MapEdge carries `confidence`",
    );
    assert!(
        CONFIDENCE.contains(&confidence.as_str()),
        "§11.5/§19.4: the edge's confidence comes from the fixed vocabulary {CONFIDENCE:?}, so a \
         one-sided observation can never be dressed up as exact, got {confidence:?}"
    );
}
