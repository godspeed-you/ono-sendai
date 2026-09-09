//! The recorder widens the time a user can see, never the scope (v0.5 §10.5, §22.8, §30.2).

mod common;

use common::{instant, options, settings};
use ono_recorder::{PROHIBITIONS, Recorder, privilege, service};

#[test]
fn should_run_with_the_users_own_privileges_when_the_recorder_starts() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    recorder
        .start(&settings(), instant("2026-08-31T12:00:00Z"))
        .expect("a start");

    let report = recorder.privilege();

    assert!(
        !report.is_elevated(),
        "§10.5: the recorder MUST run with the user's privileges — real {:?}, effective {}",
        report.real_uid,
        report.effective_uid
    );
    assert!(
        !report.is_setuid(),
        "§10.5: it MUST NOT become setuid — saved {:?}",
        report.saved_uid
    );
}

#[test]
fn should_own_its_store_when_the_recorder_creates_one() {
    use std::os::unix::fs::MetadataExt as _;

    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    recorder
        .start(&settings(), instant("2026-08-31T12:00:00Z"))
        .expect("a start");

    let report = recorder.privilege();
    let metadata =
        std::fs::metadata(directory.path().join("ledger.sqlite3")).expect("the store exists");

    assert_eq!(
        report.store_owner,
        Some(metadata.uid()),
        "§30.2: the ledger belongs to the user it is about"
    );
    assert!(
        report.store_is_own(),
        "the recorder reads and writes as the user, not as somebody else"
    );
}

#[test]
fn should_name_the_four_prohibitions_when_the_privilege_policy_is_inspected() {
    assert_eq!(
        PROHIBITIONS,
        [
            "setuid",
            "automatic_sudo",
            "privileged_daemon_for_visibility",
            "read_beyond_user",
        ],
        "§10.5's four prohibitions are the contract `docs/contracts/temporal/recorder.yaml` states"
    );
}

#[test]
fn should_spawn_nothing_but_the_users_own_service_manager_when_the_source_is_scanned() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();

    for file in rust_files(&source) {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for (number, line) in text.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            let invokes = code.contains("Command")
                || code.contains("spawn")
                || code.contains("exec")
                || code.contains(".arg");
            if invokes {
                for reach in ["sudo", "pkexec", "doas", "polkit", "setuid", "seteuid"] {
                    if code.contains(reach) {
                        offenders.push(format!("{}:{}: {reach}", file.display(), number + 1));
                    }
                }
            }
            if let Some(rest) = code.split_once("Command::new(")
                && !rest.1.starts_with("\"systemctl\"")
            {
                offenders.push(format!(
                    "{}:{}: spawns something other than the user's own service manager",
                    file.display(),
                    number + 1
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "§10.5: it MUST NOT automatically request sudo or run a privileged daemon: {offenders:?}"
    );
}

#[test]
fn should_manage_only_the_users_own_service_when_the_unit_is_driven() {
    let runner = service::RecordingRunner::default();
    let control = service::ServiceControl::new(&runner);

    control.start().expect("a start");
    control.stop().expect("a stop");
    control.is_active().expect("a query");

    for call in runner.calls() {
        assert!(
            call.contains(&"--user".to_owned()) && !call.contains(&"--system".to_owned()),
            "§10.5: no privileged system daemon is started merely to see more: {call:?}"
        );
    }
}

#[test]
fn should_read_the_kernels_answer_rather_than_the_environment_when_privilege_is_inspected() {
    let report = privilege::inspect(std::path::Path::new("/proc"), None);

    assert_eq!(
        report.effective_uid,
        ono_process::effective_uid(),
        "the answer comes from the kernel, not from `$USER`"
    );
}

fn rust_files(directory: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files
}
