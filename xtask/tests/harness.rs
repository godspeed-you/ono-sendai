//! The harness scripts, driven from outside: `scripts/acceptance.sh` is run against a stand-in
//! container runtime that records what it was asked to do, so how the harness names, builds and
//! removes its images is observed without building one.
//!
//! The stand-in fakes the outside world (the container daemon), not the harness: every line of
//! `scripts/acceptance.sh` that runs here is the line a real run executes (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use ono_testkit::{Scratch, scratch};

mod support;
use support::repo;

/// A container runtime that does nothing but write down every command it was given.
///
/// `run` holds when `STUB_HOLD` names a path: it announces itself at `<path>.started` and waits,
/// bounded, for `<path>.release`, which is how a test keeps one acceptance run in the middle of a
/// case while another one finishes.
const STUB_RUNTIME: &str = r#"#!/usr/bin/env bash
printf '%s\n' "$*" >> "$STUB_LOG"
if [ "$1" = run ]; then
  cat > /dev/null
  if [ -n "${STUB_HOLD:-}" ]; then
    : > "$STUB_HOLD.started"
    for _ in $(seq 600); do
      [ -e "$STUB_HOLD.release" ] && break
      sleep 0.1
    done
  fi
fi
exit 0
"#;

/// A checkout holding what `scripts/acceptance.sh` reads, at `<parent>/<name>`, with a stand-in
/// runtime on its own `PATH` directory.
struct Checkout {
    _scratch: Scratch,
    root: PathBuf,
    bin: PathBuf,
    runtime_dir: PathBuf,
}

fn checkout(name: &str) -> Checkout {
    let scratch = scratch();
    let root = scratch.path().join(name);
    let write = |relative: &str, text: &str| {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    };
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::copy(
        repo().join("scripts/acceptance.sh"),
        root.join("scripts/acceptance.sh"),
    )
    .expect("the harness is copied");
    write("docker/acceptance/groups", "core 000 099\n");
    write(
        "docker/acceptance/cases/000-runs.case",
        "case: the stand-in runs a case\nrun: true\n",
    );
    write(
        "docker/acceptance/cases/001-filesystems.case",
        "case: the stand-in runs a filesystem case\nimage: filesystems\nrun: true\n",
    );
    write(
        "docs/contracts/hardening/expected_test_skips.yaml",
        "acceptance:\n",
    );
    let bin = scratch.path().join("bin");
    let docker = bin.join("docker");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(&docker, STUB_RUNTIME).unwrap();
    std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755)).unwrap();
    let runtime_dir = scratch.path().join("runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    Checkout {
        _scratch: scratch,
        root,
        bin,
        runtime_dir,
    }
}

impl Checkout {
    /// `scripts/acceptance.sh <arguments>` in this checkout, logging runtime commands to `log`.
    fn command(&self, arguments: &[&str], log: &Path) -> Command {
        let mut command = Command::new("bash");
        command
            .arg(self.root.join("scripts/acceptance.sh"))
            .args(arguments)
            .current_dir(&self.root)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.bin.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("STUB_LOG", log)
            .env("XDG_RUNTIME_DIR", &self.runtime_dir)
            .env_remove("ONO_ACCEPTANCE_IMAGE")
            .env_remove("ONO_ACCEPTANCE_LAYER_CACHE")
            .env_remove("STUB_HOLD");
        command
    }

    fn run(&self, arguments: &[&str], log: &Path) -> Output {
        self.command(arguments, log)
            .output()
            .expect("bash must be runnable in the gate")
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn logged(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The image a run built with `--tag`, and the image it removed, as the runtime saw them.
fn built_images(log: &[String]) -> Vec<String> {
    log.iter()
        .filter(|line| line.starts_with("build ") || line.starts_with("buildx build "))
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            words.find(|word| *word == "--tag")?;
            words.next().map(str::to_owned)
        })
        .collect()
}

fn removed_images(log: &[String]) -> Vec<String> {
    log.iter()
        .filter_map(|line| line.strip_prefix("image rm --force "))
        .map(str::to_owned)
        .collect()
}

fn wait_for(path: &Path, child: &mut Child) {
    let started = Instant::now();
    while !path.exists() {
        if let Ok(Some(status)) = child.try_wait() {
            panic!("the held run ended ({status}) before it reached its case");
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the held run never reached its case"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

// --- #185: one image per checkout, and none removed from under another run -------------------

#[test]
fn should_build_and_remove_an_image_of_its_own_in_each_worktree() {
    // Issue #185: two worktrees shared `ono-sendai:acceptance`, so the first run to finish removed
    // the image the other was still running cases in (sixteen spurious `Unable to find image`
    // failures), and a run could grade the other worktree's binary and pass. Both checkouts here
    // carry the same directory name, as two clones of one repository do.
    let first = checkout("ono-sendai");
    let second = checkout("ono-sendai");
    let first_log = first.root.with_file_name("first.log");
    let second_log = second.root.with_file_name("second.log");

    for (checkout, log) in [(&first, &first_log), (&second, &second_log)] {
        let output = checkout.run(&[], log);
        assert!(
            output.status.success(),
            "the run against the stand-in runtime failed:\n{}",
            text(&output)
        );
    }
    let first_built = built_images(&logged(&first_log));
    let second_built = built_images(&logged(&second_log));
    assert_eq!(
        first_built.len(),
        2,
        "a run with a filesystem case builds the image and its filesystem image: {first_built:?}"
    );
    for image in &first_built {
        assert!(
            !second_built.contains(image),
            "two worktrees build the same image `{image}`, so one run can remove or replace the \
             image the other is running its cases in (issue #185): {first_built:?} against \
             {second_built:?}"
        );
        assert!(
            image.starts_with("ono-sendai:acceptance"),
            "`{image}` does not start with `ono-sendai:acceptance`, the prefix CI packs the \
             images by"
        );
        let tag = image.trim_start_matches("ono-sendai:");
        assert!(
            tag.len() <= 128
                && tag
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')),
            "`{image}` is not a tag a container runtime accepts"
        );
    }
    assert_eq!(
        removed_images(&logged(&first_log)),
        first_built,
        "a run removes exactly the images it built, and no other"
    );

    // The same checkout names the same image every time, so `--no-build` finds the one an
    // earlier `--build-only` left.
    let again = first.root.with_file_name("again.log");
    assert!(first.run(&[], &again).status.success());
    assert_eq!(
        built_images(&logged(&again)),
        first_built,
        "one checkout named two different images on two runs"
    );

    // And a name given explicitly is the name used.
    let named = first.root.with_file_name("named.log");
    let output = first
        .command(&[], &named)
        .env("ONO_ACCEPTANCE_IMAGE", "ono-sendai:acceptance-h11")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
    assert_eq!(
        built_images(&logged(&named)),
        vec![
            "ono-sendai:acceptance-h11".to_owned(),
            "ono-sendai:acceptance-h11-filesystems".to_owned()
        ],
        "ONO_ACCEPTANCE_IMAGE no longer names the image"
    );
}

#[test]
fn should_leave_an_image_in_place_while_another_run_is_still_using_it() {
    // Issue #185's other half: whatever the tag, a run that finishes must not remove an image a
    // concurrent run is still running cases in. Two runs of one checkout share an image by
    // construction, so this is where the rule has to hold on its own.
    let checkout = checkout("ono-sendai");
    let hold = checkout.root.with_file_name("hold");
    let held_log = checkout.root.with_file_name("held.log");
    let mut held = checkout
        .command(&["000-runs"], &held_log)
        .env("STUB_HOLD", &hold)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("bash must be runnable in the gate");
    wait_for(&hold.with_extension("started"), &mut held);

    let other_log = checkout.root.with_file_name("other.log");
    let other = checkout.run(&["000-runs"], &other_log);
    let _ = std::fs::write(hold.with_extension("release"), "");
    let held = held.wait_with_output().expect("the held run finishes");

    assert!(
        other.status.success(),
        "the second run failed:\n{}",
        text(&other)
    );
    assert!(
        removed_images(&logged(&other_log)).is_empty(),
        "a run removed the image while another run was in the middle of a case in it, which is \
         how that run's remaining cases fail with `Unable to find image` (issue #185):\n{}",
        text(&other)
    );
    assert!(
        text(&other).contains("another run"),
        "a run that keeps the image says why, so the image left behind is not a mystery:\n{}",
        text(&other)
    );

    assert!(held.status.success(), "{}", text(&held));
    let built = built_images(&logged(&held_log));
    assert_eq!(
        removed_images(&logged(&held_log)),
        built,
        "the last run to finish removes the image, so nothing is left behind"
    );
}

// --- #196: the sub-branch convention can be carried out as written ----------------------------

/// Whether a GitHub branch filter matches `branch`: `**` crosses `/`, `*` does not.
fn filter_matches(pattern: &str, branch: &str) -> bool {
    if let Some(rest) = pattern.strip_prefix("**") {
        return (0..=branch.len())
            .filter(|at| branch.is_char_boundary(*at))
            .any(|at| filter_matches(rest, &branch[at..]));
    }
    if let Some(rest) = pattern.strip_prefix('*') {
        return (0..=branch.len())
            .filter(|at| branch.is_char_boundary(*at))
            .take_while(|at| !branch[..*at].contains('/'))
            .any(|at| filter_matches(rest, &branch[at..]));
    }
    match (pattern.chars().next(), branch.chars().next()) {
        (Some(p), Some(b)) if p == b => {
            filter_matches(&pattern[p.len_utf8()..], &branch[b.len_utf8()..])
        }
        (None, None) => true,
        _ => false,
    }
}

#[test]
fn should_document_a_sub_branch_form_git_can_create_and_ci_runs_on() {
    // Issue #196: AGENTS.md §12.1 named `implementation/<crate>`, and git refuses to create it
    // beside a branch called `implementation` — a ref cannot be a file and a directory at once.
    let agents = support::read("AGENTS.md");
    let rule = agents
        .split("\n- ")
        .find(|item| item.starts_with("Sub-branches are allowed"))
        .expect("AGENTS.md §12.1 still says which sub-branches parallel agents may use");
    let form = rule
        .split('`')
        .skip(1)
        .step_by(2)
        .find(|token| token.starts_with("implementation") && token.contains('<'))
        .unwrap_or_else(|| panic!("the rule names no `implementation…<slug>` form:\n{rule}"));
    let open = form.find('<').unwrap();
    let close = form[open..].find('>').map(|at| open + at).unwrap();
    let branch = format!(
        "{}h7-spatial-performance{}",
        &form[..open],
        &form[close + 1..]
    );

    let git = scratch();
    let run = |arguments: &[&str]| {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(git.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git must be runnable in the gate");
        (output.status.success(), text(&output))
    };
    for arguments in [
        &["init", "--quiet", "--initial-branch", "main"][..],
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "--quiet",
            "--allow-empty",
            "--message",
            "baseline",
        ],
        &["branch", "implementation"],
    ] {
        let (ok, said) = run(arguments);
        assert!(ok, "git {arguments:?} failed: {said}");
    }
    let (created, said) = run(&["branch", &branch, "implementation"]);
    assert!(
        created,
        "AGENTS.md §12.1 names the sub-branch form `{form}`, and git refuses to create \
         `{branch}` beside `implementation` (issue #196): {said}"
    );

    // And a push of it gets CI, as a push of `implementation` does.
    let workflow: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&support::read(".github/workflows/ci.yml")).unwrap();
    let on = workflow
        .get("on")
        .or_else(|| workflow.get(serde_yaml_ng::Value::Bool(true)))
        .expect("ci.yml has triggers");
    let branches: Vec<&str> = on
        .get("push")
        .and_then(|push| push.get("branches"))
        .and_then(serde_yaml_ng::Value::as_sequence)
        .map(|list| {
            list.iter()
                .filter_map(serde_yaml_ng::Value::as_str)
                .collect()
        })
        .unwrap_or_default();
    assert!(
        branches
            .iter()
            .any(|pattern| filter_matches(pattern, &branch)),
        "a push to the documented sub-branch `{branch}` starts no CI run; ci.yml pushes on \
         {branches:?}"
    );
}
