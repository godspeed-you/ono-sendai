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
# A build handed its Dockerfile on standard input (`-`) reads it, which keeps the writer's pipe
# whole.
if [ "$1" = build ] && [ "${!#}" = - ]; then
  cat > /dev/null
fi
# What the background build of the filesystem stage holds open, for the test that the image
# lock is not among it.
case " $* " in
  *" --target filesystems-base "*)
    for fd in /proc/$$/fd/*; do readlink "$fd"; done >> "$STUB_LOG.base-fds" 2>/dev/null ;;
esac
# A build prints what `STUB_BUILD_OUTPUT` holds, the way BuildKit's plain progress would.
if [ "$1" = build ] || [ "$2" = build ]; then
  printf '%s' "${STUB_BUILD_OUTPUT:-}"
fi
# A build of the main image holds when `STUB_HOLD_BUILD` names a path, the way `run` does below.
if [ "$1" = build ] && [ -n "${STUB_HOLD_BUILD:-}" ] && [[ " $* " == *" --tag "* ]]; then
  : > "$STUB_HOLD_BUILD.started"
  for _ in $(seq 600); do
    [ -e "$STUB_HOLD_BUILD.release" ] && break
    sleep 0.1
  done
fi
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
    for script in ["acceptance.sh", "image-lock.sh", "package-check.sh"] {
        std::fs::copy(
            repo().join("scripts").join(script),
            root.join("scripts").join(script),
        )
        .expect("the harness is copied");
    }
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
    // What `scripts/package-check.sh` reads: the version of `ono-cli`, and the packages of it.
    write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/ono-cli\"]\nresolver = \"2\"\n",
    );
    write(
        "crates/ono-cli/Cargo.toml",
        "[package]\nname = \"ono-cli\"\nversion = \"1.2.3\"\nedition = \"2021\"\n",
    );
    write("crates/ono-cli/src/main.rs", "fn main() {}\n");
    write(
        "Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"ono-cli\"\nversion = \"1.2.3\"\n",
    );
    let (deb_arch, rpm_arch) = match std::env::consts::ARCH {
        "aarch64" => ("arm64", "aarch64"),
        _ => ("amd64", "x86_64"),
    };
    write(
        &format!("dist/ono_1.2.3_{deb_arch}.deb"),
        "a stand-in package",
    );
    write(
        &format!("dist/ono-1.2.3-1.{rpm_arch}.rpm"),
        "a stand-in package",
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
        self.script("acceptance.sh", arguments, log)
    }

    /// `scripts/<script> <arguments>` in this checkout, logging runtime commands to `log`.
    fn script(&self, script: &str, arguments: &[&str], log: &Path) -> Command {
        let mut command = Command::new("bash");
        command
            .arg(self.root.join("scripts").join(script))
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
            // The stand-in runtime's `run` reads its standard input to the end, as a container
            // given one does; inherited from the test, it may never end.
            .stdin(Stdio::null())
            .env_remove("ONO_ACCEPTANCE_IMAGE")
            .env_remove("ONO_ACCEPTANCE_LAYER_CACHE")
            .env_remove("STUB_HOLD")
            .env_remove("STUB_HOLD_BUILD")
            .env_remove("STUB_BUILD_OUTPUT");
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

    // The second run is another user's — `sudo` in the same checkout — with a runtime
    // directory of its own. The image tag is the daemon's, so the lock has to be one both agree
    // on, not one in either user's runtime directory.
    let other_log = checkout.root.with_file_name("other.log");
    let elsewhere = checkout.root.with_file_name("another-users-runtime");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let other = checkout
        .command(&["000-runs"], &other_log)
        .env("XDG_RUNTIME_DIR", &elsewhere)
        .env("TMPDIR", &elsewhere)
        .output()
        .unwrap();
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

// --- #139: the filesystem stage's packages come from a layer cache in CI -----------------------

#[test]
fn should_read_and_write_the_filesystem_stage_through_the_layer_cache_only_when_asked() {
    // Issue #139: on every CI run the acceptance image job downloaded the filesystem stage's
    // packages from the Ubuntu archive again, three minutes or more of the job. buildx's GitHub
    // Actions cache backend keeps that layer between runs. What this pins is the contract the
    // workflow relies on: the cache is used only when asked for, a run that may not write only
    // reads, a failed export never fails the build, and an image built by buildx's container
    // driver is loaded where `docker save` and the cases can find it.
    let checkout = checkout("ono-sendai");
    let build_lines = |layer_cache: Option<&str>| -> Vec<String> {
        let log = checkout
            .root
            .with_file_name(format!("{}.log", layer_cache.unwrap_or("none")));
        let mut command = checkout.command(&["--build-only"], &log);
        if let Some(mode) = layer_cache {
            command.env("ONO_ACCEPTANCE_LAYER_CACHE", mode);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "--build-only with ONO_ACCEPTANCE_LAYER_CACHE={layer_cache:?} failed:\n{}",
            text(&output)
        );
        logged(&log)
            .into_iter()
            .filter(|line| line.contains("build "))
            .collect()
    };
    let base = |lines: &[String]| -> String {
        lines
            .iter()
            .find(|line| line.contains("--target filesystems-base"))
            .cloned()
            .unwrap_or_else(|| panic!("no build of the filesystem stage's packages: {lines:?}"))
    };

    // Unasked, nothing changes: a workstation's runtime may not have buildx at all.
    let plain = build_lines(None);
    assert!(
        plain.iter().all(|line| !line.contains("--cache")
            && !line.contains("--load")
            && line.starts_with("build ")),
        "a run nobody asked to use a layer cache used one: {plain:#?}"
    );

    // Asked to read and write.
    let written = build_lines(Some("gha"));
    let stage = base(&written);
    assert!(
        stage.contains("--cache-from type=gha,scope=ono-acceptance-filesystems-base")
            && stage.contains("--cache-to type=gha,")
            && stage.contains("scope=ono-acceptance-filesystems-base")
            && stage.contains("ignore-error=true"),
        "the filesystem stage is not read from and written to its own cache scope, or a failed \
         export would fail the build: {stage}"
    );
    for line in written.iter().filter(|line| line.contains("--tag ")) {
        assert!(
            line.starts_with("buildx build") && line.contains("--load"),
            "an image built through buildx is not loaded into the image store, so neither \
             `docker save` nor a case can find it: {line}"
        );
    }

    // Asked only to read, as a branch that does not own the cache budget is.
    let read = build_lines(Some("gha-read"));
    let stage = base(&read);
    assert!(
        stage.contains("--cache-from type=gha,scope=ono-acceptance-filesystems-base")
            && !stage.contains("--cache-to"),
        "a read-only layer cache wrote to the cache: {stage}"
    );

    // And CI asks for it: every branch reads, `main` and `implementation` write, the builder is
    // one that can export a cache, and it is set up before the cargo cache mounts are handed to
    // it, so the mounts land in the builder that builds.
    let workflow = support::read(".github/workflows/ci.yml");
    let job = support::workflow_job(&workflow, "acceptance-image");
    assert!(
        job.contains("ONO_ACCEPTANCE_LAYER_CACHE:")
            && job.contains("'gha'")
            && job.contains("'gha-read'"),
        "the acceptance image job does not use the layer cache (issue #139):\n{job}"
    );
    let builder = job
        .find("docker/setup-buildx-action@")
        .expect("the job sets up a builder that can export a layer cache");
    let mounts = job
        .find("buildkit-cache-dance@")
        .expect("the job still hands its cargo caches to the build");
    assert!(
        builder < mounts,
        "the cargo cache mounts are handed to a builder the build does not use:\n{job}"
    );
    assert!(
        job.contains("crazy-max/ghaction-github-runtime@"),
        "the build step has no Actions cache token, so the gha backend cannot reach the cache"
    );

    // And a mode it does not know is refused rather than ignored.
    let log = checkout.root.with_file_name("unknown.log");
    let output = checkout
        .command(&["--build-only"], &log)
        .env("ONO_ACCEPTANCE_LAYER_CACHE", "registry")
        .output()
        .unwrap();
    assert!(
        !output.status.success() && text(&output).contains("ONO_ACCEPTANCE_LAYER_CACHE"),
        "an unknown layer-cache mode was accepted:\n{}",
        text(&output)
    );
}

// --- the package validation image, per checkout (ADR-0901 applied to package-check.sh) -------

#[test]
fn should_give_package_validation_an_image_of_its_own_and_keep_one_another_run_uses() {
    // `scripts/package-check.sh` built and force-removed the one tag `ono-package-check:fedora`,
    // which is #185's defect in a second script: a validation in another worktree lost its image
    // in the middle of the rpm checks.
    let first = checkout("ono-sendai");
    let second = checkout("ono-sendai");
    let validate = |checkout: &Checkout, log: &Path| {
        let output = checkout
            .script("package-check.sh", &[], log)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "package validation against the stand-in runtime failed:\n{}",
            text(&output)
        );
    };
    let first_log = first.root.with_file_name("first.log");
    let second_log = second.root.with_file_name("second.log");
    validate(&first, &first_log);
    validate(&second, &second_log);
    let first_built = built_images(&logged(&first_log));
    let second_built = built_images(&logged(&second_log));
    assert_eq!(
        first_built.len(),
        1,
        "one image is prepared: {first_built:?}"
    );
    assert!(
        first_built[0].starts_with("ono-package-check:"),
        "{first_built:?}"
    );
    assert_ne!(
        first_built, second_built,
        "two worktrees prepare the same package-validation image, so one run removes it from \
         under the other"
    );
    assert_eq!(
        removed_images(&logged(&first_log)),
        first_built,
        "a run removes exactly the image it prepared"
    );

    // Two runs of one checkout share it; the one that finishes first leaves it.
    let hold = first.root.with_file_name("hold");
    let held_log = first.root.with_file_name("held.log");
    let mut held = first
        .script("package-check.sh", &[], &held_log)
        .env("STUB_HOLD", &hold)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("bash must be runnable in the gate");
    wait_for(&hold.with_extension("started"), &mut held);
    let other_log = first.root.with_file_name("other.log");
    let other = first
        .script("package-check.sh", &[], &other_log)
        .output()
        .unwrap();
    let _ = std::fs::write(hold.with_extension("release"), "");
    let held = held.wait_with_output().expect("the held run finishes");
    assert!(other.status.success(), "{}", text(&other));
    assert!(
        removed_images(&logged(&other_log)).is_empty(),
        "package validation removed its image while another validation was using it:\n{}",
        text(&other)
    );
    assert!(held.status.success(), "{}", text(&held));
    assert_eq!(
        removed_images(&logged(&held_log)),
        built_images(&logged(&held_log)),
        "the last run to finish removes the image"
    );
}

/// The paths one of `scripts/gate.sh`'s packaging lists (`NAME=( … )`) names.
fn gate_list(gate: &str, name: &str) -> Vec<String> {
    let start = gate
        .find(&format!("{name}=("))
        .unwrap_or_else(|| panic!("scripts/gate.sh declares {name}"));
    let body = &gate[start + name.len() + 2..];
    let end = body.find(')').expect("the list closes");
    body[..end].split_whitespace().map(str::to_owned).collect()
}

#[test]
fn should_select_the_packaging_suite_when_a_file_the_packages_ship_is_added_or_removed() {
    // The gate runs `xtask/tests/packaging.rs` only when one of its inputs moved (ADR-0563), so a
    // file the packages carry that neither list names can vanish without the suite that asserts
    // its path inside the package ever running.
    let root = repo();
    let gate = std::fs::read_to_string(root.join("scripts/gate.sh")).expect("the gate");
    let mut selected = gate_list(&gate, "PACKAGING_INPUTS");
    selected.extend(gate_list(&gate, "PACKAGING_ASSETS"));

    let manifest: toml::Table = toml::from_str(
        &std::fs::read_to_string(root.join("crates/ono-cli/Cargo.toml"))
            .expect("the shell's manifest"),
    )
    .expect("the manifest parses");
    let metadata = &manifest["package"]["metadata"];
    let mut sources: Vec<String> = Vec::new();
    for asset in metadata["deb"]["assets"].as_array().expect("deb assets") {
        sources.push(asset[0].as_str().expect("a deb source").to_owned());
    }
    for asset in metadata["generate-rpm"]["assets"]
        .as_array()
        .expect("rpm assets")
    {
        sources.push(asset["source"].as_str().expect("an rpm source").to_owned());
    }

    let mut unselected = Vec::new();
    for source in sources {
        // The binaries are built, not checked in; a change to what they are made of is a change
        // to the workspace, which every other rule of the gate already covers.
        if source.starts_with("target/") {
            continue;
        }
        let path = source.trim_start_matches("../../");
        let path = match path.find('*') {
            Some(glob) => path[..glob].trim_end_matches('/'),
            None => path,
        };
        let covered = selected.iter().any(|entry| {
            path == entry || path.starts_with(&format!("{}/", entry.trim_end_matches('/')))
        });
        if !covered {
            unselected.push(path.to_owned());
        }
    }
    unselected.dedup();
    assert!(
        unselected.is_empty(),
        "shipped by the packages but selecting no packaging run in scripts/gate.sh: {unselected:?}"
    );
}

// --- #185 follow-up: what the lock does not cover yet ----------------------------------------

#[test]
fn should_keep_an_image_built_for_later_runs_until_it_is_released() {
    // `--build-only` builds the images for `--no-build` runs that come after it. A full run in
    // the same checkout finishing in between used to remove them, and the `--no-build` runs then
    // found no image.
    let checkout = checkout("ono-sendai");
    let built_log = checkout.root.with_file_name("built.log");
    let built = checkout.run(&["--build-only"], &built_log);
    assert!(built.status.success(), "{}", text(&built));

    let full_log = checkout.root.with_file_name("full.log");
    let full = checkout.run(&["000-runs"], &full_log);
    assert!(full.status.success(), "{}", text(&full));
    assert!(
        removed_images(&logged(&full_log)).is_empty(),
        "a full run removed the images a `--build-only` run built for later `--no-build` runs:\n{}",
        text(&full)
    );
    assert!(
        text(&full).contains("--build-only"),
        "the run says why it kept the image:\n{}",
        text(&full)
    );
}

#[test]
fn should_hand_the_background_build_no_copy_of_the_image_lock() {
    // The filesystem stage builds in the background while the main image compiles. A copy of the
    // lock in that child outlives the run when the run dies, and holds the image against removal.
    let checkout = checkout("ono-sendai");
    let log = checkout.root.with_file_name("fds.log");
    let output = checkout.run(&["--build-only"], &log);
    assert!(output.status.success(), "{}", text(&output));
    let fds = std::fs::read_to_string(log.with_extension("log.base-fds"))
        .expect("the background build ran");
    assert!(
        !fds.contains(".lock"),
        "the background build holds the image lock open:\n{fds}"
    );
}

#[test]
fn should_leave_no_build_log_behind_when_a_run_is_stopped() {
    let checkout = checkout("ono-sendai");
    let tmp = checkout.root.with_file_name("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let hold = checkout.root.with_file_name("hold-build");
    let log = checkout.root.with_file_name("stopped.log");
    let mut run = checkout
        .command(&["--build-only"], &log)
        .env("STUB_HOLD_BUILD", &hold)
        .env("TMPDIR", &tmp)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for(&hold.with_extension("started"), &mut run);
    let stopped = Command::new("kill")
        .args(["-TERM", &run.id().to_string()])
        .status()
        .unwrap();
    assert!(stopped.success());
    // The stand-in build outlives the stopped script and holds its output open until released.
    let _ = std::fs::write(hold.with_extension("release"), "");
    let _ = run.wait_with_output();
    let left: Vec<_> = std::fs::read_dir(&tmp)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert!(
        left.is_empty(),
        "a stopped run left its temporary build log behind: {left:?}"
    );
}

#[test]
fn should_keep_the_image_lock_in_the_repository_every_run_of_the_checkout_shares() {
    // A checkout that is the top of a repository keeps its lock in the common git directory,
    // which its worktrees, its owner and root all reach; one that is not keeps it in itself. Never
    // in a per-user runtime directory, which a `sudo` run does not share.
    let plain = checkout("ono-sendai");
    let log = plain.root.with_file_name("plain.log");
    assert!(plain.run(&["000-runs"], &log).status.success());
    assert!(
        plain.root.join(".ono-acceptance.lock").is_file(),
        "a checkout outside any repository keeps its lock in itself"
    );
    assert!(
        std::fs::read_dir(&plain.runtime_dir)
            .unwrap()
            .next()
            .is_none(),
        "the lock is in the per-user runtime directory"
    );

    let repository = checkout("ono-sendai");
    let git = |arguments: &[&str]| {
        let status = Command::new("git")
            .args(arguments)
            .current_dir(&repository.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git must be runnable in the gate");
        assert!(
            status.status.success(),
            "git {arguments:?}: {}",
            text(&status)
        );
    };
    git(&["init", "--quiet"]);
    let log = repository.root.with_file_name("repository.log");
    let output = repository.run(&["000-runs"], &log);
    assert!(output.status.success(), "{}", text(&output));
    assert!(
        repository.root.join(".git/ono-acceptance.lock").is_file(),
        "the top of a repository keeps its lock in the common git directory"
    );
    assert!(
        !repository.root.join(".ono-acceptance.lock").exists(),
        "and leaves nothing in the working tree"
    );
}

// --- package validation's record and manifest check ---------------------------------------------

/// The deb and rpm file names of the stand-in packages in a checkout's `dist/`.
fn stand_in_packages() -> (String, String) {
    let (deb_arch, rpm_arch) = match std::env::consts::ARCH {
        "aarch64" => ("arm64", "aarch64"),
        _ => ("amd64", "x86_64"),
    };
    (
        format!("ono_1.2.3_{deb_arch}.deb"),
        format!("ono-1.2.3-1.{rpm_arch}.rpm"),
    )
}

#[test]
fn should_record_what_each_release_directory_validated_on_its_own() {
    // The record of what was installed was one file per checkout, so two validations of two
    // directories — a release-check and a plain package-check — overwrote each other's.
    let checkout = checkout("ono-sendai");
    let (deb, rpm) = stand_in_packages();
    let other = checkout.root.join("other-dist");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join(&deb), "another build's package").unwrap();
    std::fs::write(other.join(&rpm), "another build's package").unwrap();

    let first = checkout
        .script(
            "package-check.sh",
            &[],
            &checkout.root.with_file_name("a.log"),
        )
        .output()
        .unwrap();
    assert!(first.status.success(), "{}", text(&first));
    let second = checkout
        .script(
            "package-check.sh",
            &["--dist", other.to_str().unwrap()],
            &checkout.root.with_file_name("b.log"),
        )
        .output()
        .unwrap();
    assert!(second.status.success(), "{}", text(&second));

    let records: Vec<String> = std::fs::read_dir(checkout.root.join("target"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(".sha256"))
        .map(|path| std::fs::read_to_string(path).unwrap())
        .collect();
    assert_eq!(
        records.len(),
        2,
        "two release directories validated, and not two records of what was installed: \
         {records:?}"
    );
    assert!(
        std::fs::read_to_string(checkout.root.join("target/package-check.sha256"))
            .is_ok_and(|record| record.contains(&deb)),
        "the default directory's record stays where the release workflow collects it"
    );
}

#[test]
fn should_require_the_manifest_to_record_every_package_that_was_validated() {
    // The comparison passed when any one validated digest appeared in SHA256SUMS, so a manifest
    // that described the .deb and not the .rpm was accepted.
    let checkout = checkout("ono-sendai");
    let (deb, _) = stand_in_packages();
    let dist = checkout.root.join("dist");
    let digest = |name: &str| {
        let output = Command::new("sha256sum")
            .arg(dist.join(name))
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next()
            .unwrap()
            .to_owned()
    };
    std::fs::write(
        dist.join("SHA256SUMS"),
        format!("{}  {deb}\n", digest(&deb)),
    )
    .unwrap();
    let output = checkout
        .script(
            "package-check.sh",
            &[],
            &checkout.root.with_file_name("m.log"),
        )
        .output()
        .unwrap();
    assert!(
        !output.status.success() && text(&output).contains("SHA256SUMS"),
        "a manifest that leaves a validated package out was accepted:\n{}",
        text(&output)
    );
}

// --- #139 follow-up: the cached layer ages out, and a run says whether it was cached ---------

#[test]
fn should_rebuild_the_filesystem_stage_weekly_and_say_whether_it_came_from_the_cache() {
    // The layer cache kept the unpinned `apt-get install` of the filesystem stage from its first
    // write onwards, re-read on every push and never evicted, so CI stopped seeing what the Ubuntu
    // archive serves. An ISO week in the stage's build arguments is a new cache key every week.
    let checkout = checkout("ono-sendai");
    let log = checkout.root.with_file_name("week.log");
    let output = checkout
        .command(&["--build-only"], &log)
        .env("ONO_ACCEPTANCE_LAYER_CACHE", "gha-read")
        .env(
            "STUB_BUILD_OUTPUT",
            "#7 [filesystems-base 2/2] RUN apt-get update && apt-get install\n#7 CACHED\n",
        )
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
    let base = logged(&log)
        .into_iter()
        .find(|line| line.contains("--target filesystems-base"))
        .expect("the filesystem stage was built");
    let week = base
        .split_whitespace()
        .find_map(|word| word.strip_prefix("ONO_ARCHIVE_WEEK="))
        .unwrap_or_else(|| panic!("the stage is built without an archive week: {base}"));
    assert!(
        week.len() == 8 && week.as_bytes()[4] == b'-' && week.as_bytes()[5] == b'W',
        "the archive week is an ISO week, `YYYY-Www`: {week}"
    );
    let fs = logged(&log)
        .into_iter()
        .find(|line| line.contains("--target runtime-filesystems"))
        .expect("the filesystem image was built");
    assert!(
        fs.contains(&format!("ONO_ARCHIVE_WEEK={week}")),
        "the filesystem image is built on a different week's stage than the one just built: {fs}"
    );
    assert!(
        text(&output).contains("from the layer cache"),
        "a cache hit on the filesystem stage is not reported:\n{}",
        text(&output)
    );

    let dockerfile = support::read("docker/Dockerfile");
    let stage = dockerfile
        .split("AS filesystems-base")
        .nth(1)
        .and_then(|rest| rest.split("apt-get install").next())
        .expect("the filesystem stage installs packages");
    assert!(
        stage.contains("ARG ONO_ARCHIVE_WEEK"),
        "the filesystem stage does not declare the archive week before it installs, so the week \
         does not reach the cache key:\n{stage}"
    );
}

#[test]
fn should_name_the_ci_acceptance_image_explicitly_in_the_job_that_builds_it_and_the_jobs_that_run_it()
 {
    // The image job builds and packs the images, and each group job loads them and runs
    // `--no-build`. With the tag derived from the checkout path, the two only agree because
    // GitHub's runners happen to check out at the same path.
    let workflow = support::read(".github/workflows/ci.yml");
    let tag_of = |job: &str| {
        let text = support::workflow_job(&workflow, job);
        text.lines()
            .find_map(|line| line.trim().strip_prefix("ONO_ACCEPTANCE_IMAGE:"))
            .map(|value| value.trim().trim_matches('"').to_owned())
            .unwrap_or_else(|| {
                panic!("the `{job}` job does not name its acceptance image:\n{text}")
            })
    };
    let built = tag_of("acceptance-image");
    assert_eq!(
        built,
        tag_of("acceptance"),
        "the group jobs run an image other than the one the image job built"
    );
    assert!(
        built.starts_with("ono-sendai:acceptance"),
        "`{built}` is not packed by the `ono-sendai:acceptance*` filter"
    );
}

// --- the budget where the packages are built (issue #125, ADR-0866) ----------------------------

/// A packaging tool that answers its version and writes the file it is told to, nothing else.
fn stand_in_packager(bin: &Path, name: &str, version: &str) {
    let path = bin.join(name);
    std::fs::write(
        &path,
        format!(
            "#!/usr/bin/env bash\nshift\nif [ \"$1\" = --version ]; then echo '{name} {version}'; \
             exit 0; fi\nwhile [ $# -gt 0 ]; do if [ \"$1\" = --output ]; then : > \"$2\"; fi; \
             shift; done\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A checkout that can run `scripts/package.sh --no-build`: the script, the budget script, a
/// registry budgeting `ono` at 1000 and `kuang-compile` at 500 bytes on x86_64 only, stand-in
/// packagers, and stand-in binaries of the given sizes where the build leaves them.
fn packaging_checkout(ono: u64, compiler: u64) -> Checkout {
    let checkout = checkout("ono-sendai");
    for script in ["package.sh", "binary-size.sh"] {
        std::fs::copy(
            repo().join("scripts").join(script),
            checkout.root.join("scripts").join(script),
        )
        .expect("the script is copied");
    }
    let limits = checkout.root.join("docs/contracts/hardening/limits.yaml");
    std::fs::write(
        &limits,
        "version: 1\nlimits: []\nbuild_budgets:\n  - key: build.ono\n    binary: ono\n    \
         triple: x86_64-unknown-linux-gnu\n    budget: 1000\n  - key: build.compiler\n    \
         binary: kuang-compile\n    triple: x86_64-unknown-linux-gnu\n    budget: 500\n",
    )
    .unwrap();
    stand_in_packager(&checkout.bin, "cargo-deb", "3.7.0");
    stand_in_packager(&checkout.bin, "cargo-generate-rpm", "0.21.0");
    for triple in ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"] {
        let release = checkout.root.join("target").join(triple).join("release");
        std::fs::create_dir_all(&release).unwrap();
        for (name, bytes) in [("ono", ono), ("kuang-compile", compiler)] {
            let path = release.join(name);
            std::fs::File::create(&path)
                .and_then(|file| file.set_len(bytes))
                .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    checkout
}

fn package(checkout: &Checkout, triple: &str) -> Output {
    let dist = checkout.root.join("scratch-dist");
    checkout
        .script(
            "package.sh",
            &[
                "--no-build",
                "--target",
                triple,
                "--dist",
                dist.to_str().unwrap(),
            ],
            &checkout.root.with_file_name("package.log"),
        )
        .env("SOURCE_DATE_EPOCH", "1700000000")
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("bash must be runnable in the gate")
}

fn packaged(checkout: &Checkout) -> bool {
    std::fs::read_dir(checkout.root.join("scratch-dist"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "deb"))
}

#[test]
fn should_package_every_shipped_binary_within_its_budget_and_say_how_large_it_is() {
    let checkout = packaging_checkout(1000, 500);
    let output = package(&checkout, "x86_64-unknown-linux-gnu");
    assert!(output.status.success(), "{}", text(&output));
    let said = text(&output);
    for (binary, bytes) in [("/ono ", "1000 bytes"), ("/kuang-compile ", "500 bytes")] {
        assert!(
            said.lines().any(|line| line.starts_with("binary-size:")
                && line.contains(binary)
                && line.contains(bytes)),
            "the release job's log states the size of every shipped binary: {said}"
        );
    }
    assert!(packaged(&checkout));
}

#[test]
fn should_refuse_to_package_a_binary_over_the_budget_of_its_triple() {
    // The release workflow runs scripts/package.sh on the runner that builds the bytes that
    // ship; a budget checked anywhere else checks some other build.
    for (ono, compiler) in [(1001, 500), (1000, 501)] {
        let checkout = packaging_checkout(ono, compiler);
        let output = package(&checkout, "x86_64-unknown-linux-gnu");
        assert!(
            !output.status.success() && text(&output).contains("over its budget"),
            "ono {ono}, kuang-compile {compiler}: {}",
            text(&output)
        );
        assert!(!packaged(&checkout), "nothing over budget is packaged");
    }
}

#[test]
fn should_refuse_to_package_for_a_triple_no_budget_covers() {
    // The registry budgets x86_64 only; an aarch64 package would ship unmeasured.
    let checkout = packaging_checkout(10, 10);
    let output = package(&checkout, "aarch64-unknown-linux-gnu");
    assert!(
        !output.status.success() && text(&output).contains("aarch64-unknown-linux-gnu"),
        "{}",
        text(&output)
    );
    assert!(!packaged(&checkout));
}

#[test]
fn should_say_so_when_it_packages_a_binary_it_does_not_measure() {
    // scripts/rebuild-check.sh --binary packages a binary it was handed — the gate hands in a
    // stand-in — to compare two packaging runs; its size is not a size of anything that ships.
    // Skipping the check is allowed there only out loud (ADR-0866).
    let checkout = packaging_checkout(5000, 5000);
    let dist = checkout.root.join("scratch-dist");
    let output = checkout
        .script(
            "package.sh",
            &[
                "--no-build",
                "--target",
                "x86_64-unknown-linux-gnu",
                "--dist",
                dist.to_str().unwrap(),
                "--size-unmeasured",
                "a stand-in handed to the test",
            ],
            &checkout.root.with_file_name("package.log"),
        )
        .env("SOURCE_DATE_EPOCH", "1700000000")
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("bash must be runnable in the gate");
    assert!(output.status.success(), "{}", text(&output));
    assert!(
        text(&output).contains("binary-size: not measured — a stand-in handed to the test"),
        "{}",
        text(&output)
    );
}
