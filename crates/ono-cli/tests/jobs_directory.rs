//! A job's relative paths keep the directory the job was started in (issue #302).
//!
//! A process has one working directory, and the foreground moves it. A job is part of the same
//! process, so a relative path the job resolved through the kernel meant whatever directory the
//! foreground stood in at the moment the job reached it: `remove file *.o &` followed by `cd ~/src`
//! could act on `~/src`. A job's filesystem meaning is fixed by the directory it was started in.
//!
//! Every test here holds the job at a gate — a FIFO it reads by its absolute path — until the
//! foreground has moved, and the script itself opens the gate after the move, so the order is
//! guaranteed by the script and not by timing. No test asserts a duration; the shell's watchdog
//! bounds every wait.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::path::{Path, PathBuf};
use std::time::Duration;

use ono_testkit::{Scratch, scratch};

use support::run_bounded;

const BUDGET: Duration = Duration::from_secs(60);

/// Two directories — where the job starts and where the foreground goes — and a gate the job
/// waits at until the script opens it.
struct Rig {
    scratch: Scratch,
    root: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let scratch = scratch();
        let root = scratch.path().to_path_buf();
        std::fs::create_dir_all(root.join("start")).expect("the start directory");
        std::fs::create_dir_all(root.join("moved")).expect("the moved-to directory");
        let rig = Self { scratch, root };
        rig.fifo("gate");
        rig
    }

    /// Makes a FIFO at `relative`, which a reader blocks on until a writer has come and gone.
    fn fifo(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .expect("mkfifo runs");
        assert!(made.success(), "the FIFO {} exists", path.display());
        path
    }

    fn start(&self) -> PathBuf {
        self.root.join("start")
    }

    fn moved(&self) -> PathBuf {
        self.root.join("moved")
    }

    fn gate(&self) -> PathBuf {
        self.root.join("gate")
    }

    /// A program that waits until the gate is opened.
    fn wait_at_gate(&self) -> String {
        format!("sh -c 'read line < {}'", self.gate().display())
    }

    /// A program that opens the gate: it writes one line and closes it.
    fn open_gate(&self) -> String {
        format!("sh -c 'echo go > {}'", self.gate().display())
    }

    fn put(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the directory");
        std::fs::write(path, contents).expect("the fixture is written");
    }

    fn exists(&self, relative: &str) -> bool {
        self.root.join(relative).exists()
    }
}

/// A program that waits until one of `paths` exists; the watchdog bounds it.
fn wait_for_either(paths: &[&Path]) -> String {
    let tests = paths
        .iter()
        .map(|path| format!("[ ! -e {} ]", path.display()))
        .collect::<Vec<_>>()
        .join(" && ");
    format!("sh -c 'while {tests}; do sleep 0.02; done'")
}

#[test]
fn should_write_where_a_native_job_started_when_the_foreground_moved_before_it_wrote() {
    // The line is native stages only. `write file` collects its content until the gate closes,
    // and only then names `out.txt` to the filesystem — after the foreground has moved.
    let rig = Rig::new();
    let gate2 = rig.fifo("gate2");
    let absolute = rig.root.join("absolute.txt");

    let open2 = format!("sh -c 'echo go > {}'", gate2.display());
    let settled = wait_for_either(&[&rig.start().join("out.txt"), &rig.moved().join("out.txt")]);
    let written = wait_for_either(&[&absolute]);
    let run = run_bounded(
        &rig.scratch,
        &format!(
            "cd {start}\n\
             read file {gate} | write file out.txt &\n\
             read file {gate2} | write file {absolute} &\n\
             cd {moved}\n\
             {open}\n\
             {open2}\n\
             {settled}\n\
             {written}\n\
             echo settled",
            start = rig.start().display(),
            gate = rig.gate().display(),
            gate2 = gate2.display(),
            absolute = absolute.display(),
            moved = rig.moved().display(),
            open = rig.open_gate(),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        rig.exists("start/out.txt"),
        "the job wrote its relative `out.txt` in the directory it was started in. {}",
        run.report()
    );
    assert!(
        !rig.exists("moved/out.txt"),
        "nothing appeared in the directory the foreground moved to. {}",
        run.report()
    );
    assert!(
        absolute.exists(),
        "an absolute path means what it says, in a job as anywhere. {}",
        run.report()
    );
}

#[test]
fn should_read_a_relative_path_where_a_job_started_when_the_foreground_moved_before_it_read() {
    // A read, not a mutation: the rule is about what a relative path means, not about which
    // command uses it. The job's function reads `marker.txt` once the gate opens; only the start
    // directory has one. The name also arrives as an evaluated argument, so a path written as an
    // expression is held to the same rule as a word.
    let rig = Rig::new();
    rig.put("start/marker.txt", "start\n");

    let run = run_bounded(
        &rig.scratch,
        &format!(
            "cd {start}\n\
             fn probe(name) {{ {wait}; get file $name | select name | to json; get file marker.txt | select name | to json }}\n\
             probe marker.txt &\n\
             cd {moved}\n\
             {open}\n\
             fg %1\n\
             echo \"status-$?\"",
            start = rig.start().display(),
            moved = rig.moved().display(),
            wait = rig.wait_at_gate(),
            open = rig.open_gate(),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        !run.stderr.contains("io.not_found"),
        "the job's relative path was not looked up where the foreground went. {}",
        run.report()
    );
    assert!(
        run.stdout.matches("[{\"name\":\"marker.txt\"}]").count() == 2,
        "both reads — the word and the evaluated argument — found the start directory's file. {}",
        run.report()
    );
    assert!(
        run.stdout.contains("status-0"),
        "the job succeeded and `fg` reports it. {}",
        run.report()
    );
}

#[test]
fn should_remove_the_jobs_globbed_files_and_not_the_namesakes_where_the_foreground_went() {
    // The issue's own shape: `remove file *.o` in a job. The glob expands in the job's directory
    // to relative names, and the directory the foreground moves to has files of the same names.
    let rig = Rig::new();
    rig.put("start/one.o", "");
    rig.put("start/two.o", "");
    rig.put("start/keep.txt", "");
    rig.put("moved/one.o", "");
    rig.put("moved/two.o", "");

    let run = run_bounded(
        &rig.scratch,
        &format!(
            "cd {start}\n\
             fn clean() {{ {wait}; remove file *.o --confirm | to json }}\n\
             clean &\n\
             cd {moved}\n\
             {open}\n\
             fg %1\n\
             echo \"status-$?\"",
            start = rig.start().display(),
            moved = rig.moved().display(),
            wait = rig.wait_at_gate(),
            open = rig.open_gate(),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        !rig.exists("start/one.o") && !rig.exists("start/two.o") && rig.exists("start/keep.txt"),
        "the job removed what its glob named, in its own directory. {}",
        run.report()
    );
    assert!(
        rig.exists("moved/one.o") && rig.exists("moved/two.o"),
        "the namesakes where the foreground stands are untouched. {}",
        run.report()
    );
}

#[test]
fn should_list_the_jobs_directory_when_a_job_lists_without_naming_one() {
    // An omitted directory means the working directory, and a job's working directory is the one it
    // was started in.
    let rig = Rig::new();
    rig.put("start/start-only.txt", "");
    rig.put("moved/moved-only.txt", "");

    let run = run_bounded(
        &rig.scratch,
        &format!(
            "cd {start}\n\
             fn walk() {{ {wait}; get dir | select name | to json }}\n\
             walk &\n\
             cd {moved}\n\
             {open}\n\
             fg %1",
            start = rig.start().display(),
            moved = rig.moved().display(),
            wait = rig.wait_at_gate(),
            open = rig.open_gate(),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        run.stdout.contains("start-only.txt") && !run.stdout.contains("moved-only.txt"),
        "the job listed the directory it was started in. {}",
        run.report()
    );
}

#[test]
fn should_open_a_jobs_native_redirection_in_the_jobs_directory() {
    let rig = Rig::new();
    rig.put("start/marker.txt", "start\n");

    let run = run_bounded(
        &rig.scratch,
        &format!(
            "cd {start}\n\
             fn keep() {{ {wait}; get file marker.txt | select name | to json > kept.json }}\n\
             keep &\n\
             cd {moved}\n\
             {open}\n\
             fg %1\n\
             echo \"status-$?\"",
            start = rig.start().display(),
            moved = rig.moved().display(),
            wait = rig.wait_at_gate(),
            open = rig.open_gate(),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        rig.exists("start/kept.json") && !rig.exists("moved/kept.json"),
        "the job's redirection opened its file where the job was started. {}",
        run.report()
    );
}

#[test]
fn should_keep_the_jobs_directory_when_the_foreground_leaves_the_directory_it_entered() {
    // `enter dir` moves the foreground, and `leave` moves it back. A job started inside the
    // entered directory keeps it when the foreground leaves.
    let rig = Rig::new();

    let run = run_bounded(
        &rig.scratch,
        &format!(
            "cd {moved}\n\
             enter dir {start}\n\
             read file {gate} | write file out.txt &\n\
             leave\n\
             {open}\n\
             {wait}\n\
             echo settled",
            start = rig.start().display(),
            moved = rig.moved().display(),
            gate = rig.gate().display(),
            open = rig.open_gate(),
            wait = wait_for_either(&[&rig.start().join("out.txt"), &rig.moved().join("out.txt")]),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        rig.exists("start/out.txt") && !rig.exists("moved/out.txt"),
        "the job wrote in the directory it was started in, not where `leave` took the shell. {}",
        run.report()
    );
}

#[test]
fn should_run_a_jobs_programs_in_the_jobs_directory_and_leave_the_foregrounds_paths_relative() {
    // Programs were always given the job's directory; that stays so. And the foreground's own
    // relative paths are untouched by any of this: they still resolve where it stands and are
    // reported as written.
    let rig = Rig::new();
    rig.put("moved/here.txt", "");

    let run = run_bounded(
        &rig.scratch,
        &format!(
            "cd {start}\n\
             fn where() {{ {wait}; sh -c 'pwd > where.txt' }}\n\
             where &\n\
             cd {moved}\n\
             {open}\n\
             fg %1\n\
             get file here.txt | select path | to json",
            start = rig.start().display(),
            moved = rig.moved().display(),
            wait = rig.wait_at_gate(),
            open = rig.open_gate(),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    let recorded = std::fs::read_to_string(rig.start().join("where.txt")).unwrap_or_default();
    assert_eq!(
        Path::new(recorded.trim()).canonicalize().ok(),
        rig.start().canonicalize().ok(),
        "the job's program ran in the job's directory. {}",
        run.report()
    );
    assert!(
        run.stdout.contains("[{\"path\":\"here.txt\"}]"),
        "the foreground resolves and reports its relative path as before. {}",
        run.report()
    );
}

#[test]
fn should_plan_a_jobs_relative_target_in_the_jobs_directory() {
    // `plan` resolves its operation's target to the absolute path it freezes (change spec §4.3).
    // From a job, that resolution happens where the job was started.
    let rig = Rig::new();
    rig.put("start/victim.txt", "start\n");
    rig.put("moved/victim.txt", "moved\n");
    let planned = rig.root.join("plan.json");

    let run = run_bounded(
        &rig.scratch,
        &format!(
            "cd {start}\n\
             fn propose() {{ {wait}; plan remove file victim.txt | to json > {planned} }}\n\
             propose &\n\
             cd {moved}\n\
             {open}\n\
             fg %1",
            start = rig.start().display(),
            moved = rig.moved().display(),
            wait = rig.wait_at_gate(),
            open = rig.open_gate(),
            planned = planned.display(),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    let plan = std::fs::read_to_string(&planned).unwrap_or_default();
    let start = rig.start().join("victim.txt").display().to_string();
    let moved = rig.moved().join("victim.txt").display().to_string();
    assert!(
        plan.contains(&start) && !plan.contains(&moved),
        "the plan froze the job's own `victim.txt`. plan: {plan}\n{}",
        run.report()
    );
}

// --- every way a relative path reaches a job (review of #302) ------------------------------------

#[test]
fn should_remove_the_jobs_file_when_records_captured_before_launch_are_piped_into_a_mutation() {
    // Review H1: records named relatively before the job started carry identities like `a.o`.
    // Piped into a mutation inside the job they mean the directory the job was started in.
    let rig = Rig::new();
    rig.put("start/a.o", "");
    rig.put("moved/a.o", "");

    let run = run_bounded(
        &rig.scratch,
        &format!(
            "cd {start}\n\
             let fs = (get file a.o)\n\
             fn clean() {{ {wait}; $fs | remove file --confirm | to json }}\n\
             clean &\n\
             cd {moved}\n\
             {open}\n\
             fg %1",
            start = rig.start().display(),
            moved = rig.moved().display(),
            wait = rig.wait_at_gate(),
            open = rig.open_gate(),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        !rig.exists("start/a.o") && rig.exists("moved/a.o"),
        "the piped record's relative identity meant the job's directory. {}",
        run.report()
    );
}

#[test]
fn should_run_an_opaque_plan_action_applied_in_a_job_in_the_jobs_directory() {
    // Review H2: an opaque action's program is a child the job starts, so it runs where the job
    // was started.
    let rig = Rig::new();
    rig.put("start/a.o", "");
    rig.put("moved/a.o", "");

    let run = run_bounded(
        &rig.scratch,
        &format!(
            "set config change.allow_opaque_actions true\n\
             cd {start}\n\
             fn wipe() {{ {wait}; plan --opaque rm a.o | apply --accept-risk --accept-irreversible --confirm | to json }}\n\
             wipe &\n\
             cd {moved}\n\
             {open}\n\
             fg %1",
            start = rig.start().display(),
            moved = rig.moved().display(),
            wait = rig.wait_at_gate(),
            open = rig.open_gate(),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        !rig.exists("start/a.o") && rig.exists("moved/a.o"),
        "the opaque program ran in the job's directory. {}",
        run.report()
    );
}

#[test]
fn should_read_a_plugin_package_named_relatively_in_a_job_from_the_jobs_directory() {
    // Review H3: `path:./pkg` is a relative path like any other.
    let rig = Rig::new();
    support::lay_out_echo_package(&rig.start(), "dev.ono.example.anchored", "");

    let run = run_bounded(
        &rig.scratch,
        &format!(
            "cd {start}\n\
             fn check() {{ {wait}; verify plugin path:./dev.ono.example.anchored | to json }}\n\
             check &\n\
             cd {moved}\n\
             {open}\n\
             fg %1",
            start = rig.start().display(),
            moved = rig.moved().display(),
            wait = rig.wait_at_gate(),
            open = rig.open_gate(),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    assert!(
        !run.stderr.contains("holds no `manifest.yaml`")
            && run.stdout.contains("dev.ono.example.anchored"),
        "the job read the package in its own directory. {}",
        run.report()
    );
}

#[test]
fn should_start_a_handler_named_relatively_in_a_job_from_and_in_the_jobs_directory() {
    // Review H4: a program a native command starts on a job's behalf runs in the job's directory,
    // and a path-shaped program name means the job's directory.
    let rig = Rig::new();
    rig.put("start/a.txt", "");
    rig.put("moved/a.txt", "");
    for side in ["start", "moved"] {
        let tool = rig.root.join(side).join("tool");
        support::executable(
            &tool,
            &format!(
                "#!/bin/sh\necho \"{side} $(pwd)\" > {}\n",
                rig.root.join("ran").display()
            ),
        );
    }

    let run = run_bounded(
        &rig.scratch,
        &format!(
            "cd {start}\n\
             fn show() {{ {wait}; open file a.txt --with ./tool | to json }}\n\
             show &\n\
             cd {moved}\n\
             {open}\n\
             fg %1",
            start = rig.start().display(),
            moved = rig.moved().display(),
            wait = rig.wait_at_gate(),
            open = rig.open_gate(),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    let ran = std::fs::read_to_string(rig.root.join("ran")).unwrap_or_default();
    let (which, cwd) = ran.trim().split_once(' ').unwrap_or_default();
    assert_eq!(
        which,
        "start",
        "the job's own `./tool` ran. {}",
        run.report()
    );
    assert_eq!(
        Path::new(cwd).canonicalize().ok(),
        rig.start().canonicalize().ok(),
        "it ran in the job's directory. {}",
        run.report()
    );
}

#[test]
#[cfg(feature = "remote")]
fn should_send_a_jobs_relative_path_to_a_link_as_written() {
    // Review H5: inside a link frame the remote answers, and a local directory means nothing
    // there; the path travels as it was written.
    let rig = Rig::new();

    let run = run_bounded(
        &rig.scratch,
        &format!(
            "cd {start}\n\
             fn far() {{ {wait}; link host far --transport local; enter link far; get file relname-only | to json }}\n\
             far &\n\
             cd {moved}\n\
             {open}\n\
             fg %1",
            start = rig.start().display(),
            moved = rig.moved().display(),
            wait = rig.wait_at_gate(),
            open = rig.open_gate(),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    let anchored = rig.start().join("relname-only").display().to_string();
    assert!(
        run.stderr.contains("relname-only") && !run.stderr.contains(&anchored),
        "the remote was asked for the path as written. {}",
        run.report()
    );
}

#[test]
fn should_keep_a_trailing_slash_when_a_job_anchors_a_path() {
    // Review L1: `link/` names what the link points to, `link` the link. Anchoring keeps the text
    // it joins, so the job acts on the object the same words name in the foreground.
    let rig = Rig::new();
    rig.put("start/target/inside.txt", "");
    std::os::unix::fs::symlink(rig.start().join("target"), rig.start().join("link"))
        .expect("the link");

    let run = run_bounded(
        &rig.scratch,
        &format!(
            "cd {start}\n\
             fn kind() {{ {wait}; get file link/ | select kind | to json }}\n\
             kind &\n\
             cd {moved}\n\
             {open}\n\
             fg %1\n\
             cd {start}\n\
             get file link/ | select kind | to json",
            start = rig.start().display(),
            moved = rig.moved().display(),
            wait = rig.wait_at_gate(),
            open = rig.open_gate(),
        ),
        BUDGET,
    );

    assert!(run.finished, "{}", run.report());
    let kinds: Vec<&str> = run
        .stdout
        .lines()
        .filter(|line| line.starts_with("[{\"kind\""))
        .collect();
    assert_eq!(kinds.len(), 2, "both described it. {}", run.report());
    assert_eq!(
        kinds[0],
        kinds[1],
        "the job described `link/` as the foreground does. {}",
        run.report()
    );
}
