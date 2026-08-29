//! The agentless fallback of spec §21.3: a reduced provider set over plain POSIX commands.
//!
//! Spec §21.3: "If no Ono-Sendai agent exists remotely, the link MAY fall back to SSH and a
//! limited provider set implemented through standard commands/procfs reads. Fallback MUST be
//! visible because semantics and performance may differ."
//!
//! *Visible* is the property these suites hold the implementation to, and it is structural, not
//! a sentence printed somewhere: every target the agent would have served is present in the
//! reduced link, and the ones the reduced set cannot answer are
//! [`Availability::Unavailable`](ono_provider_api::Availability::Unavailable) with the reason.
//! An empty answer where the honest answer is "I cannot see this" is exactly the conflation
//! spec §35 forbids.
//!
//! Every suite runs offline: the far side is a recording fake, or a local child process.

#![allow(
    clippy::expect_used,
    reason = "a shared test helper states a precondition the same way a #[test] body does, where \
              clippy's allow-expect-in-tests does not reach"
)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use ono_core::ErrorCode;
use ono_pipeline::StreamEvent;
use ono_provider_api::{Availability, Provider, ProviderRegistry, Query};
use ono_remote::{AgentlessLink, FarSide, LocalFarSide, SshFarSide};
use ono_value::{ErrorValue, Link, Value};

/// What `ps -e -o pid=,…` writes on a machine with three processes: the fixture the procps
/// adapter's own conformance suite reads (`docs/spec/adapters/fixtures/procps/ps`).
const PS_OUTPUT: &[u8] = b"      1       0 root     Ss    0.7 17264  27284    1  584656 /usr/lib/systemd/systemd --switched-root --system\n      2       0 root     S     0.0     0      0    1  584656 [kthreadd]\n   2120    2098 william  Ss    0.0  6880  10248    2  584576 /usr/bin/dbus-daemon --session\n";

/// What `df --block-size=1 --output=source,fstype,size,used,avail,target` writes.
const DF_OUTPUT: &[u8] = b"Filesystem     Type      1B-blocks         Used    Available Mounted on\n/dev/sda2      ext4    501445033984 262008619008 213908901888 /\n";

/// The targets an agent-mode link to a Linux machine negotiates — what the reduced set is
/// measured against, because the question a user has is "what did I lose?".
const AGENT_TARGETS: &[&str] = &["process", "filesystem", "service", "socket"];

/// A far side that answers from a table instead of running anything, and remembers what it was
/// asked to run.
#[derive(Debug, Default)]
struct Recorded {
    answers: BTreeMap<String, Vec<u8>>,
    asked: Mutex<Vec<Vec<String>>>,
}

impl Recorded {
    fn answering(pairs: &[(&str, &[u8])]) -> Arc<Self> {
        Arc::new(Self {
            answers: pairs
                .iter()
                .map(|(program, output)| ((*program).to_owned(), (*output).to_vec()))
                .collect(),
            asked: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<Vec<String>> {
        self.asked
            .lock()
            .expect("the recorder is not poisoned")
            .clone()
    }
}

impl FarSide for Recorded {
    fn run(&self, argv: &[String], _env: &BTreeMap<String, String>) -> Result<Vec<u8>, ErrorValue> {
        self.asked
            .lock()
            .expect("the recorder is not poisoned")
            .push(argv.to_vec());
        let program = argv.first().map(String::as_str).unwrap_or_default();
        self.answers.get(program).cloned().ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::RemoteUnreachable,
                format!("this fake far side has no `{program}`"),
            )
        })
    }
}

/// A far side that answers `uname` so a link can be opened, and nothing else.
fn reachable(extra: &[(&str, &[u8])]) -> Arc<Recorded> {
    let mut pairs: Vec<(&str, &[u8])> = vec![("uname", b"Linux x86_64\n")];
    pairs.extend_from_slice(extra);
    Recorded::answering(&pairs)
}

fn open(far_side: Arc<Recorded>) -> AgentlessLink {
    AgentlessLink::open("testbox", far_side, AGENT_TARGETS).expect("the far side answered `uname`")
}

async fn drain(stream: ono_pipeline::ValueStream) -> (Vec<Value>, Vec<ErrorValue>) {
    let (mut values, mut failures) = (Vec::new(), Vec::new());
    let mut stream = stream;
    while let Some(event) = stream.recv().await {
        match event {
            StreamEvent::Value(value) => values.push(value),
            StreamEvent::Failure(error) => failures.push(error),
        }
    }
    (values, failures)
}

fn mounted(link: &AgentlessLink) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    link.register_into(&mut registry);
    registry
}

#[tokio::test]
async fn should_answer_processes_from_plain_posix_output_when_no_agent_is_present() {
    let far_side = reachable(&[("ps", PS_OUTPUT)]);
    let link = open(Arc::clone(&far_side));
    let registry = mounted(&link);

    let stream = registry
        .snapshot(&Query::target("process"))
        .expect("the reduced set answers `process`");
    let (values, failures) = drain(stream).await;

    assert!(failures.is_empty(), "nothing failed: {failures:?}");
    let pids: Vec<Option<Value>> = values
        .iter()
        .map(|value| match value {
            Value::Record(record) => record.get("pid").cloned(),
            other => panic!("a process arrives as a record, got {other:?}"),
        })
        .collect();
    assert_eq!(
        pids,
        vec![
            Some(Value::Int(1)),
            Some(Value::Int(2)),
            Some(Value::Int(2120))
        ],
        "spec §21.3: the reduced set really reads the far side, through the v0.3 adapter layer"
    );

    let ran = far_side.asked();
    assert!(
        ran.iter()
            .any(|argv| argv.first().map(String::as_str) == Some("ps")
                && argv.iter().any(|word| word.contains("pid="))),
        "the adapter's own declared invocation is what runs on the far side, got {ran:?}"
    );
}

#[tokio::test]
async fn should_say_which_host_an_agentless_record_was_observed_on() {
    let link = open(reachable(&[("ps", PS_OUTPUT)]));
    let registry = mounted(&link);

    let stream = registry
        .snapshot(&Query::target("process"))
        .expect("the reduced set answers `process`");
    let (values, _) = drain(stream).await;

    for value in &values {
        let Value::Record(record) = value else {
            panic!("a process arrives as a record");
        };
        assert_eq!(
            record.provenance().link(),
            &Link::Remote("testbox".into()),
            "spec §25.2: a record read across a link says which host it came from, agent or not"
        );
    }
}

#[tokio::test]
async fn should_answer_filesystems_from_plain_posix_output_when_no_agent_is_present() {
    let link = open(reachable(&[("df", DF_OUTPUT)]));
    let registry = mounted(&link);

    let stream = registry
        .snapshot(&Query::target("filesystem"))
        .expect("the reduced set answers `filesystem`");
    let (values, failures) = drain(stream).await;

    assert!(failures.is_empty(), "nothing failed: {failures:?}");
    let mounts: Vec<Option<Value>> = values
        .iter()
        .map(|value| match value {
            Value::Record(record) => record.get("target").cloned(),
            other => panic!("a filesystem arrives as a record, got {other:?}"),
        })
        .collect();
    assert_eq!(
        mounts,
        vec![Some(Value::Path(std::sync::Arc::from(
            std::path::Path::new("/")
        )))]
    );
}

#[tokio::test]
async fn should_refuse_a_target_the_reduced_set_cannot_answer_rather_than_answer_nothing() {
    let link = open(reachable(&[]));
    let registry = mounted(&link);

    let refusal = registry
        .snapshot(&Query::target("service"))
        .expect_err("agentless mode has no service manager to ask");

    assert_eq!(
        refusal.code(),
        ErrorCode::ProviderUnavailable,
        "spec §35: `I cannot see this` is a refusal, never an empty list"
    );
    assert!(
        refusal.message().contains("agentless"),
        "the reason names the mode that reduced the answer, got {:?}",
        refusal.message()
    );
}

#[test]
fn should_name_every_target_the_agent_would_have_served() {
    let link = open(reachable(&[]));

    let mut seen: Vec<(String, bool)> = link
        .providers()
        .iter()
        .flat_map(|provider| {
            let available = provider.availability() == Availability::Available;
            provider
                .targets()
                .iter()
                .map(move |target| ((*target).to_owned(), available))
                .collect::<Vec<_>>()
        })
        .collect();
    seen.sort();

    assert_eq!(
        seen,
        vec![
            ("filesystem".to_owned(), true),
            ("process".to_owned(), true),
            ("service".to_owned(), false),
            ("socket".to_owned(), false),
        ],
        "spec §21.3: a reduced link is visibly reduced — every target the agent served is still \
         mounted, and the ones the reduced set cannot answer say so structurally"
    );
}

#[test]
fn should_report_what_the_far_side_said_it_is() {
    let link = open(reachable(&[]));

    assert_eq!(
        link.system(),
        Some("Linux x86_64"),
        "spec §21.2 wants the remote OS and arch out of a link handshake; `uname -s -m` is what \
         a machine without an agent can be asked"
    );
    assert_eq!(link.host(), "testbox");
}

#[test]
fn should_refuse_to_open_an_agentless_link_to_a_far_side_that_does_not_answer() {
    let far_side = Recorded::answering(&[]);

    let refusal = AgentlessLink::open("nowhere", far_side, AGENT_TARGETS)
        .expect_err("a far side that cannot run `uname` is not a far side");

    assert_eq!(refusal.code(), ErrorCode::RemoteUnreachable);
}

#[test]
fn should_run_the_reduced_set_commands_through_ssh_on_the_far_side() {
    let far_side = SshFarSide::new(ono_remote::SshTarget::new("prod-db"));

    let spelled = far_side.spelled(
        &[
            "ps".to_owned(),
            "-e".to_owned(),
            "-o".to_owned(),
            "pid=,args=".to_owned(),
        ],
        &BTreeMap::from([("LC_ALL".to_owned(), "C".to_owned())]),
    );

    assert_eq!(
        spelled,
        vec![
            "ssh".to_owned(),
            "-o".to_owned(),
            "BatchMode=yes".to_owned(),
            "-T".to_owned(),
            "--".to_owned(),
            "prod-db".to_owned(),
            "LC_ALL='C' 'ps' '-e' '-o' 'pid=,args='".to_owned(),
        ],
        "spec §21.3's fallback is `ssh <host> <command>`; a refusal is never a prompt \
         (ADR-0015 standing rule 4), and every word is quoted so a host's login shell cannot \
         re-read one"
    );
}

#[test]
fn should_run_the_reduced_set_commands_as_a_child_when_the_far_side_is_this_machine() {
    let far_side = LocalFarSide;

    let output = far_side
        .run(
            &["uname".to_owned(), "-s".to_owned()],
            &BTreeMap::from([("LC_ALL".to_owned(), "C".to_owned())]),
        )
        .expect("`uname -s` runs on the machine the suite runs on");

    assert!(
        !output.is_empty(),
        "the local far side is the same code path over a child process, which is what makes the \
         fallback provable without a network"
    );
}
