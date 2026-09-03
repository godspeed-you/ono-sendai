//! What a native plugin runs inside, and what happens when it cannot be made to (v0.4.1 spec
//! §0.5.3, §2.3, §16.1–§16.3, §59.7, Appendix D; issues #59 and #60).
//!
//! `crates/ono-kuang-supervisor/src/sandbox.rs` says of the controls it installs between `fork`
//! and `exec`: *"so the artifact never executes an instruction outside it"*. Appendix D says the
//! same in a table — for `no_new_privs`, resource limits and session separation the Failure
//! column reads `spawn fails` — and §2.3 makes it a release-wide invariant: *"If Ono claims that
//! a safety control is applied before an operation, failure to apply that control MUST prevent
//! the operation from starting."*
//!
//! At the time this suite was written none of that was true. Every confinement syscall in the
//! pre-exec closure discarded its return value and the closure ended in an unconditional
//! `Ok(())`, so a control the kernel refused to install was a control nobody heard about, and
//! the artifact exec’d anyway — inside whatever confinement happened to survive. Issues #59
//! and #60 made each return value fatal to the spawn, and the proof below runs un-ignored from
//! that increment on (ADR-0443, ADR-0444).
//!
//! # How the failure is arranged
//!
//! §59.7 asks for "an injectable platform layer/test hook" that makes a mandatory control fail.
//! This suite needs no such layer, because one mandatory control can be made to fail from
//! outside the process with nothing but the standard library, deterministically and without
//! privileges: `setsid` returns `EPERM` when the calling process is already a process-group
//! leader, and `Command::process_group(0)` makes the child exactly that before the pre-exec
//! closure runs. Session separation is mandatory — §16.4 lists it as `mandatory for the native
//! supervised tier`, Appendix D as `required`, failure `spawn fails` — so a child that reaches
//! `exec` after that call failed is the defect of §0.5.3, observed rather than simulated.
//!
//! The test below is the failure proof required by §57 phase H0 before the fixes in issues #59
//! and #60 may land. It was committed `#[ignore]`d because it failed at HEAD; the increment that
//! made pre-exec failures fatal removed the attribute rather than the test, and its assertion is
//! the one it was written with.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::PathBuf;

use ono_kuang_protocol::CpuBudget;
use ono_kuang_supervisor::{native_process, working_directory};

/// The session id in a `/proc/<pid>/stat` line.
///
/// The command name is parenthesised and may itself contain spaces, so everything after the last
/// `)` is positional: `state`, `ppid`, `pgrp`, `session`.
fn session_of(stat: &str) -> Option<&str> {
    stat.rsplit_once(')')?.1.split_whitespace().nth(3)
}

/// The session this test process runs in, which a child that separated itself must not share.
fn our_session() -> String {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("this process has a stat line");
    session_of(&stat)
        .expect("a stat line carries the session id")
        .to_owned()
}

#[tokio::test]
async fn should_not_exec_the_plugin_when_a_mandatory_confinement_control_fails() {
    let root = std::env::temp_dir().join(format!("ono-confinement-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a private directory for the fixture plugin");
    let marker = root.join("the-plugin-ran");

    // Stands in for the plugin's runtime artifact: the first thing it does on startup is record
    // that it started, which is the marker §59.7 requires to remain absent.
    let mut command = tokio::process::Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(format!("cat /proc/self/stat > {}", marker.display()))
        // Makes the child its own process-group leader before the pre-exec closure runs, so the
        // mandatory `setsid` in that closure is refused with EPERM.
        .process_group(0);
    let sandbox = native_process(
        512 * 1024 * 1024,
        CpuBudget::Interactive,
        working_directory(Some(&root), &PathBuf::from("/bin/sh")),
    );

    let spawned = spawn(
        &mut command,
        &sandbox,
        &NativePlatform::shared(),
        "the-plugin",
    );
    if let Ok((mut child, _report)) = spawned {
        let _ = child.wait().await;
    }
    let ran = std::fs::read_to_string(&marker).ok();
    let ran_in_session = ran.as_deref().and_then(session_of).map(str::to_owned);
    let _ = std::fs::remove_dir_all(&root);

    assert_eq!(
        ran_in_session,
        None,
        "v0.4.1 §2.3 and §16.3: a mandatory control that could not be installed MUST prevent the \
         plugin from starting, and Appendix D spells session separation out — `required`, \
         failure `spawn fails`. `setsid` was refused here because the child was already a \
         process-group leader, and the artifact ran regardless — in the supervisor's own session \
         {}, which is the confinement it was supposed to have left. A confinement report calling \
         this spawn confined would be claiming it of nothing (§16.5).",
        our_session()
    );
}

// ------------------------------------------------------------------------------------------
// The control table (issue #58, v0.4.1 §16.1, §16.4, §52.1, §52.3, Appendix D).
// ------------------------------------------------------------------------------------------

/// The registry `docs/spec/hardening/kuang_confinement_controls.yaml`, as the gate reads it.
///
/// §16.4 asks for *one* central table, and §52.2 for one source of truth behind the runtime, the
/// generated documentation and the tests. So these tests read the table itself rather than a
/// second copy of it typed into the suite: a row that disagrees with the runtime is the drift
/// §52.3 exists to catch, and a table nothing reads is not a source of truth.
fn control_table() -> serde_yaml_ng::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec/hardening/kuang_confinement_controls.yaml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} must be readable: {error}", path.display()));
    serde_yaml_ng::from_str(&text).expect("the control table is valid YAML")
}

/// The `native-confined` rows as `(control id, requirement, failure)`.
fn native_confined_rows() -> Vec<(String, String, String)> {
    let table = control_table();
    let tiers = table["tiers"]
        .as_sequence()
        .expect("the table declares tiers");
    let tier = tiers
        .iter()
        .find(|tier| tier["id"].as_str() == Some("native-confined"))
        .expect("the tier v0.4.1 ships is declared");
    tier["controls"]
        .as_sequence()
        .expect("the tier declares its controls")
        .iter()
        .map(|row| {
            let field = |name: &str| {
                row[name]
                    .as_str()
                    .unwrap_or_else(|| panic!("every control row carries `{name}`"))
                    .to_owned()
            };
            (field("control"), field("requirement"), field("failure"))
        })
        .collect()
}

#[test]
fn should_classify_every_control_the_confinement_table_declares() {
    use ono_kuang_protocol::{Control, ExecutionTier};

    let table = control_table();
    let declared: Vec<String> = table["controls"]
        .as_sequence()
        .expect("the table declares controls")
        .iter()
        .map(|control| {
            control["id"]
                .as_str()
                .expect("every control carries an id")
                .to_owned()
        })
        .collect();

    for id in &declared {
        assert!(
            Control::from_id(id).is_some(),
            "v0.4.1 §16.4 asks for one central table and §52.3 makes an unknown control id a gate \
             failure. `{id}` is declared in \
             docs/spec/hardening/kuang_confinement_controls.yaml and the supervisor has never \
             heard of it, so whatever the table says about it governs nothing."
        );
    }
    for control in Control::ALL {
        assert!(
            declared.iter().any(|id| id == control.id()),
            "the supervisor knows the control `{}` and the central table declares it nowhere, so \
             its requirement is a constant in the code rather than a row an operator can read \
             (§16.4, §52.2).",
            control.id()
        );
    }
    for (id, _, _) in native_confined_rows() {
        let control = Control::from_id(&id).expect("checked above");
        let _requirement = ExecutionTier::NativeConfined.requirement(control);
    }
}

#[test]
fn should_treat_a_control_the_table_calls_mandatory_as_mandatory() {
    use ono_kuang_protocol::{Control, ExecutionTier, Requirement};

    for (id, requirement, failure) in native_confined_rows() {
        let control = Control::from_id(&id)
            .unwrap_or_else(|| panic!("`{id}` is a control the supervisor knows"));
        let expected = match requirement.as_str() {
            "mandatory" => Requirement::Mandatory,
            "best_effort" => Requirement::BestEffort,
            "not_provided" => Requirement::NotProvided,
            other => panic!("`{other}` is not one of §16.4's requirement words"),
        };
        assert_eq!(
            ExecutionTier::NativeConfined.requirement(control),
            expected,
            "v0.4.1 §16.4: the central table calls `{id}` `{requirement}` in the \
             `native-confined` tier, and the supervisor treats it as something else. A control \
             the table calls mandatory is one §2.3 says the spawn must not survive."
        );
        assert_eq!(
            ExecutionTier::NativeConfined.failure(control).as_str(),
            failure,
            "Appendix D's Failure column for `{id}` reads `{failure}`, and the supervisor would \
             do something else with it."
        );
    }
}

// ------------------------------------------------------------------------------------------
// Every security-relevant syscall return value is checked (issue #59, v0.4.1 §16.2, §0.5.3,
// §65.4, §59.7's "injectable platform layer").
// ------------------------------------------------------------------------------------------

use std::sync::Arc;

use ono_kuang_protocol::{Control, ExecutionTier, Requirement};
use ono_kuang_supervisor::{
    ConfinementEntry, ConfinementPlan, ConfinementPlatform, ControlResult, NativePlatform, spawn,
};

/// A platform that installs everything the host does, except one control it refuses.
///
/// §59.7 asks for "an injectable platform layer/test hook that makes `PR_SET_NO_NEW_PRIVS`
/// fail". This is that layer: the supervisor takes its platform from the caller, production
/// passes [`NativePlatform`], and a test passes this. Nothing about the fault lives in the
/// supervisor, and the fake refuses exactly one control so the assertion has one cause.
struct Refuses {
    control: Control,
    errno: i32,
    inner: NativePlatform,
}

impl ConfinementPlatform for Refuses {
    fn install(&self, control: Control, plan: &ConfinementPlan) -> std::io::Result<()> {
        if control == self.control {
            // `from_raw_os_error` allocates nothing, which matters: this runs in the forked
            // child, between fork and exec, where a malloc could deadlock on a lock another
            // thread held at the moment of the fork.
            return Err(std::io::Error::from_raw_os_error(self.errno));
        }
        self.inner.install(control, plan)
    }
}

fn refusing(control: Control) -> Arc<dyn ConfinementPlatform> {
    Arc::new(Refuses {
        control,
        errno: libc::EPERM,
        inner: NativePlatform,
    })
}

/// A fixture whose artifact records that it started, in a directory of its own.
struct Fixture {
    root: PathBuf,
    marker: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("ono-confinement-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private directory for the fixture plugin");
        let marker = root.join("the-plugin-ran");
        Self { root, marker }
    }

    /// A command that writes the startup marker §59.7 requires to remain absent.
    fn command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(format!("echo started > {}", self.marker.display()));
        command
    }

    fn sandbox(&self) -> ono_kuang_supervisor::Sandbox {
        native_process(
            512 * 1024 * 1024,
            CpuBudget::Interactive,
            working_directory(Some(&self.root), &PathBuf::from("/bin/sh")),
        )
    }

    fn plugin_ran(&self) -> bool {
        self.marker.exists()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn should_report_the_failing_control_when_a_confinement_syscall_returns_an_error() {
    // §16.2: "Every syscall used to establish a mandatory security or resource control MUST have
    // its return value checked." The observable form of "checked" is that the caller can say
    // which one failed; a return value that is checked and then discarded is not checked.
    let fixture = Fixture::new("named");
    let mut command = fixture.command();
    let outcome = spawn(
        &mut command,
        &fixture.sandbox(),
        &refusing(Control::NoNewPrivs),
        "example",
    );

    let error = match outcome {
        Ok(_) => panic!(
            "v0.4.1 §2.3 and §65.4: `PR_SET_NO_NEW_PRIVS` was refused and the spawn succeeded \
             anyway. Calling a confinement syscall, discarding its result and executing the \
             plugin is the named failure mode of this release."
        ),
        Err(error) => error,
    };
    assert!(
        error.message().contains(Control::NoNewPrivs.id())
            || error
                .metadata()
                .get("control")
                .and_then(serde_json::Value::as_str)
                == Some(Control::NoNewPrivs.id()),
        "§16.3: the caller must receive a structured error identifying which control could not \
         be installed, and this one says only: {error:?}"
    );
    assert!(
        !fixture.plugin_ran(),
        "§59.7: a marker the plugin would create on startup MUST remain absent"
    );
}

#[tokio::test]
async fn should_check_the_result_of_every_control_the_table_marks_mandatory() {
    // One case per mandatory control, which is what issue #59's exit test asks for. The list is
    // read from `Control::ALL` rather than typed here, so a control added later cannot escape
    // the check by nobody remembering to add a case for it.
    for control in ExecutionTier::NativeConfined.mandatory_controls() {
        if !ono_kuang_supervisor::is_installed_by_the_platform(control) {
            // Installed by the parent or held for the instance's whole life, not by a syscall in
            // the child. The suite covers those separately; this test is about §16.2's syscalls.
            continue;
        }
        let fixture = Fixture::new(&format!("mandatory-{control}"));
        let mut command = fixture.command();
        let outcome = spawn(
            &mut command,
            &fixture.sandbox(),
            &refusing(control),
            "example",
        );
        assert!(
            outcome.is_err(),
            "v0.4.1 §2.3: `{control}` is `{}` in the `native-confined` tier, so a kernel that \
             refuses it MUST prevent the plugin from starting. It started.",
            Requirement::Mandatory
        );
        assert!(
            !fixture.plugin_ran(),
            "`{control}` was refused and the plugin ran anyway (§59.7)"
        );
    }
}

#[tokio::test]
async fn should_start_the_plugin_when_a_best_effort_control_is_refused() {
    // The other half of §16.4: a best-effort failure "MUST still be observable in diagnostics but
    // does not prevent spawn". Refusing the spawn over a scheduling preference would be a denial
    // of service in the name of a nice level.
    let best_effort: Vec<Control> = Control::ALL
        .iter()
        .copied()
        .filter(|&control| {
            ExecutionTier::NativeConfined.requirement(control) == Requirement::BestEffort
                && ono_kuang_supervisor::is_installed_by_the_platform(control)
        })
        .collect();
    assert!(
        !best_effort.is_empty(),
        "§16.4 declares at least `scheduling_priority` best-effort; if the table no longer does, \
         this test is asserting nothing"
    );
    for control in best_effort {
        let fixture = Fixture::new(&format!("best-effort-{control}"));
        let mut command = fixture.command();
        let (mut child, report) = spawn(
            &mut command,
            &fixture.sandbox(),
            &refusing(control),
            "example",
        )
        .unwrap_or_else(|error| {
            panic!("§16.4: a best-effort `{control}` failure must not prevent spawn: {error:?}")
        });
        let _ = child.wait().await;
        assert!(
            report.failed().any(|entry| entry.control() == control),
            "§16.4: a best-effort failure must still be observable in diagnostics, and this \
             report does not mention `{control}`"
        );
    }
}

// ------------------------------------------------------------------------------------------
// A pre-exec failure prevents the exec (issue #60, v0.4.1 §16.3, §18.1, §59.7, §59.8, §67.3).
// ------------------------------------------------------------------------------------------

use ono_kuang_protocol::{KuangErrorCode, Manifest};
use ono_kuang_supervisor::{LoadConfig, Supervisor, host_platform};

/// A package whose runtime artifact is a shell script that records that it started.
///
/// The load never completes — a shell script does not speak the KUANG/11 protocol — which is
/// exactly what makes the assertion sharp: the question is not whether the load failed, it is
/// *where*. A load that reaches the handshake has already exec'd the artifact, and §59.7 says it
/// must not have.
struct Package {
    root: PathBuf,
    artifact: PathBuf,
    marker: PathBuf,
}

impl Package {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("ono-load-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private directory for the fixture package");
        let marker = root.join("the-plugin-ran");
        let artifact = root.join("echo");
        std::fs::write(
            &artifact,
            format!("#!/bin/sh\necho started > {}\nexit 0\n", marker.display()),
        )
        .expect("the fixture artifact");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&artifact, std::fs::Permissions::from_mode(0o755))
                .expect("an executable fixture artifact");
        }
        Self {
            root,
            artifact,
            marker,
        }
    }

    fn config(&self, platform: Arc<dyn ConfinementPlatform>) -> LoadConfig {
        let manifest = Manifest::parse(MANIFEST).expect("the fixture manifest is valid");
        let mut config = LoadConfig::new(&self.artifact, manifest);
        config.private_dir = Some(self.root.clone());
        config.platform = host_platform();
        config.confinement = platform;
        config
    }

    fn plugin_ran(&self) -> bool {
        self.marker.exists()
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const MANIFEST: &str = r#"
format: kuang-package/1
package:
  id: dev.example.echo
  name: echo
  version: 0.1.0
  description: Emits what it is asked to emit.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64, linux-arm64, linux-x86_64]
runtime:
  kind: native-process
  entry: runtime/echo
  memory_max: 64MiB
  cpu_budget: interactive
  startup: lazy
roles: [provider]
network:
  outbound: none
"#;

#[tokio::test]
async fn should_never_exec_the_plugin_when_a_mandatory_control_cannot_be_installed() {
    // §59.7, verbatim: "Using an injectable platform layer/test hook that makes
    // `PR_SET_NO_NEW_PRIVS` fail, a native plugin spawn MUST fail before plugin code executes."
    let package = Package::new("no-new-privs");
    let error = Supervisor::load(package.config(refusing(Control::NoNewPrivs)))
        .await
        .expect_err("a spawn whose mandatory control was refused does not produce a loaded plugin");

    assert_eq!(
        error.code(),
        KuangErrorCode::PluginNoNewPrivsFailed,
        "§16.3 names the error family, and §67.3 shows the shape: the operator learns which \
         control failed, not that something did. Got: {error:?}"
    );
    assert!(
        !package.plugin_ran(),
        "§59.7: a marker the plugin would create on startup MUST remain absent"
    );
}

#[tokio::test]
async fn should_leave_the_plugins_startup_marker_absent_after_a_failed_confinement_setup() {
    // §59.8: "When mandatory `setrlimit` installation fails, plugin execution MUST not proceed."
    // The control case runs first, because a marker that is absent when the fixture cannot write
    // it at all would prove nothing.
    let control = Package::new("rlimit-control");
    let _ = Supervisor::load(control.config(NativePlatform::shared())).await;
    assert!(
        control.plugin_ran(),
        "the fixture artifact writes its marker when it is allowed to run; without that, the \
         assertion below is vacuous"
    );

    let refused = Package::new("rlimit-refused");
    let error = Supervisor::load(refused.config(refusing(Control::RlimitData)))
        .await
        .expect_err("a refused mandatory resource limit does not produce a loaded plugin");
    assert_eq!(error.code(), KuangErrorCode::PluginResourceLimitFailed);
    assert!(
        !refused.plugin_ran(),
        "§59.8 and §2.3: the plugin executed although the ceiling it was to run under was refused"
    );
}

#[tokio::test]
async fn should_name_the_control_that_could_not_be_installed_in_the_structured_error() {
    // §54.1: a refusal names the boundary that decided it, in an ordinary structured error and
    // without `RUST_LOG=debug`. §67.3 is the rendered shape:
    //
    //     error plugin.confinement_failed:
    //       example was not started because no_new_privs could not be installed
    //       required control: no_new_privs
    //       execution tier: native-confined
    let package = Package::new("named-control");
    let error = Supervisor::load(package.config(refusing(Control::SessionSeparation)))
        .await
        .expect_err("a refused mandatory control does not produce a loaded plugin");

    assert_eq!(error.code(), KuangErrorCode::PluginConfinementFailed);
    assert_eq!(
        error
            .metadata()
            .get("control")
            .and_then(serde_json::Value::as_str),
        Some(Control::SessionSeparation.id()),
        "a script branches on the metadata, not on the sentence: {error:?}"
    );
    assert_eq!(
        error
            .metadata()
            .get("execution_tier")
            .and_then(serde_json::Value::as_str),
        Some(ExecutionTier::NativeConfined.id()),
        "§67.3 shows the tier beside the control, because which controls were promised depends \
         on it: {error:?}"
    );
    let help = error.help().unwrap_or_default();
    assert!(
        help.contains("required control: session_separation"),
        "§67.3's second line, so the operator does not have to read the metadata: {help}"
    );
    assert!(
        error.message().contains("dev.example.echo"),
        "the error names the package that did not start: {error:?}"
    );
    assert!(!package.plugin_ran());
}

// ------------------------------------------------------------------------------------------
// A confinement report per spawn (issue #61, v0.4.1 §16.5, §2.6).
// ------------------------------------------------------------------------------------------

#[tokio::test]
async fn should_report_the_state_of_every_control_after_a_successful_spawn() {
    // §16.5's five columns, for every control the tier claims. A report with a gap in it would
    // let "we do not know" read as "it is fine", which §2.6 forbids in as many words.
    let fixture = Fixture::new("report");
    let mut command = fixture.command();
    let (mut child, report) = spawn(
        &mut command,
        &fixture.sandbox(),
        &NativePlatform::shared(),
        "example",
    )
    .expect("this host installs every control the native-confined tier claims");
    let _ = child.wait().await;

    let claimed: Vec<Control> = ExecutionTier::NativeConfined.claimed_controls().collect();
    assert_eq!(
        report
            .entries()
            .iter()
            .map(ConfinementEntry::control)
            .collect::<Vec<_>>(),
        claimed,
        "§16.5: the report has one row per control the tier claimed, not per control it managed"
    );
    for entry in report.entries() {
        assert_eq!(
            entry.result(),
            ControlResult::Applied,
            "`{}` was claimed by the `native-confined` tier and the report says `{}`. §16.5's \
             invariant is that a successful spawn implies every required control applied, and a \
             report is where that stops being an aspiration.",
            entry.control(),
            entry.result()
        );
        assert!(entry.attempted());
        assert_eq!(
            entry.required(),
            ExecutionTier::NativeConfined
                .requirement(entry.control())
                .is_mandatory(),
            "the `required` column comes from the central table, not from a second opinion"
        );
        assert_eq!(
            entry.platform_detail(),
            None,
            "§16.5: no secrets, and nothing to explain"
        );
    }
    assert!(report.is_confined());
    assert_eq!(report.tier(), ExecutionTier::NativeConfined);
    // Appendix D's last four rows are a statement about the tier, not an outcome of a spawn: a
    // report that listed `filesystem_isolation` at all would invite exactly the inference the
    // appendix closes by forbidding.
    assert!(report.entry(Control::FilesystemIsolation).is_none());
    assert!(report.entry(Control::NetworkIsolation).is_none());
}

#[tokio::test]
async fn should_mark_a_best_effort_control_that_was_not_available_as_skipped_rather_than_applied() {
    // §2.6: "If Ono cannot determine whether a plugin control was installed … it MUST report an
    // explicit unknown/refusal state rather than claim success." A platform that does not
    // implement a best-effort control has installed nothing, and the row must say so.
    struct Unavailable;
    impl ConfinementPlatform for Unavailable {
        fn install(&self, control: Control, plan: &ConfinementPlan) -> std::io::Result<()> {
            if control == Control::SchedulingPriority {
                return Err(std::io::Error::from(std::io::ErrorKind::Unsupported));
            }
            NativePlatform.install(control, plan)
        }
    }

    let fixture = Fixture::new("unavailable");
    let mut command = fixture.command();
    let sandbox = native_process(
        512 * 1024 * 1024,
        // A class with a non-zero nice level, so the control is actually attempted.
        CpuBudget::Background,
        working_directory(Some(&fixture.root), &PathBuf::from("/bin/sh")),
    );
    let (mut child, report) = spawn(
        &mut command,
        &sandbox,
        &(Arc::new(Unavailable) as Arc<dyn ConfinementPlatform>),
        "example",
    )
    .expect("§16.4: a best-effort control does not prevent spawn");
    let _ = child.wait().await;

    let entry = report
        .entry(Control::SchedulingPriority)
        .expect("the tier claims it, so the report has a row for it");
    assert_eq!(
        entry.result(),
        ControlResult::Skipped,
        "a control this platform does not implement was not applied and was not refused: it was \
         skipped, and §2.6 forbids reporting the difference as success"
    );
    assert!(!entry.required());
    assert!(
        report.is_confined(),
        "§16.4: a best-effort gap is not a reason to refuse the spawn"
    );
}

#[tokio::test]
async fn should_never_hand_back_a_plugin_whose_report_is_not_confined() {
    // Issue #61's exit test: any report with a `required` control not `applied` correlates with
    // no running plugin. Driven over every mandatory control the platform installs, at the load
    // boundary a caller actually uses.
    for control in ExecutionTier::NativeConfined.mandatory_controls() {
        if !ono_kuang_supervisor::is_installed_by_the_platform(control) {
            continue;
        }
        let package = Package::new(&format!("unconfined-{control}"));
        let loaded = Supervisor::load(package.config(refusing(control))).await;
        assert!(
            loaded.is_err(),
            "`{control}` could not be installed and the supervisor handed back a plugin anyway \
             (§16.5, §2.3)"
        );
        assert!(
            !package.plugin_ran(),
            "`{control}`: the artifact ran (§59.7)"
        );
    }
}

// ------------------------------------------------------------------------------------------
// The execution tier is a name (issue #64, v0.4.1 §17.2, §17.3).
// ------------------------------------------------------------------------------------------

#[tokio::test]
async fn should_report_a_named_execution_tier_rather_than_a_sandboxed_boolean() {
    // §17.2: "The v0.4.1 code SHOULD avoid boolean names such as `sandboxed: true` that cannot
    // represent these distinctions." A boolean can say that *something* is in force; it cannot
    // say whether the filesystem is behind a kernel policy or merely behind a broker, which is
    // the difference §15.1 exists to keep. So the answer is a name, and the name resolves to a
    // table of controls.
    let fixture = Fixture::new("tier");
    let mut command = fixture.command();
    let (mut child, report) = spawn(
        &mut command,
        &fixture.sandbox(),
        &NativePlatform::shared(),
        "example",
    )
    .expect("the native-confined tier installs on this host");
    let _ = child.wait().await;

    assert_eq!(report.tier().id(), "native-confined");
    assert!(
        report.tier().is_available(),
        "§17.3: a tier this build cannot install must not be offered, and this one it can"
    );
    assert!(!ExecutionTier::NativeIsolated.is_available());
    assert!(!ExecutionTier::Wasm.is_available());

    // The name is not a synonym for "sandboxed": it stands for a list of controls, and it says
    // in as many words which boundary it does not have.
    let boundary = report.tier().boundary();
    assert!(
        boundary.contains("not a complete filesystem or network sandbox"),
        "§15.2's security meaning has to survive wherever the tier is described: {boundary}"
    );
    assert!(
        report
            .entries()
            .iter()
            .any(|entry| entry.control() == Control::NoNewPrivs && entry.required()),
        "the tier name resolves to the controls it stands for, which is what a boolean could not do"
    );
}
