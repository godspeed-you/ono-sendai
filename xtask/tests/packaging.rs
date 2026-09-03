//! The installable shape of the `ono` binary (docs/ACCEPTANCE.md §4.5, ADR-0121, ADR-0122):
//! `cargo deb` and `cargo generate-rpm` read `crates/ono-cli/Cargo.toml`, and what they build
//! puts `ono` at `/usr/bin/ono`, registers it as a login shell on install, unregisters it on
//! removal, and ships the licence, the README and the generated command reference.
//!
//! These tests package a stand-in binary so the gate stays cheap and never depends on a release
//! build lying around. The real binary is installed in fresh containers by
//! `scripts/package-check.sh`, which is the referee for "the package works".

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use ono_testkit::SkipReason;

use ono_testkit::scratch;

mod support;
use support::{repo, workflow_job};

/// A private target directory holding a stand-in `release/ono` — this test executable, which
/// is a genuine ELF binary so dependency scanners see what they see on the real thing.
fn staged_target_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ono-packaging-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("release")).expect("a scratch target directory");
    std::fs::copy(
        std::env::current_exe().expect("the test executable has a path"),
        dir.join("release/ono"),
    )
    .expect("the stand-in binary is staged");
    dir
}

fn run(program: &str, args: &[&str], target_dir: &Path) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(repo())
        .env("CARGO_TARGET_DIR", target_dir)
        .output()
        .unwrap_or_else(|error| panic!("`{program}` must be runnable in the gate: {error}"));
    assert!(
        output.status.success(),
        "`{program} {}` failed:\n{}\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn debian_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => panic!("no Debian architecture name for {other}"),
    }
}

#[test]
fn should_build_a_deb_that_installs_ono_as_a_registered_login_shell() {
    let target_dir = staged_target_dir("deb");
    let deb = target_dir.join("ono.deb");
    let deb_path = deb.to_str().expect("a UTF-8 scratch path");
    run(
        "cargo",
        &[
            "deb",
            "--package",
            "ono-cli",
            "--no-build",
            "--no-strip",
            "--output",
            deb_path,
        ],
        &target_dir,
    );

    let info = run("dpkg-deb", &["--info", deb_path], &target_dir);
    for field in [
        "Package: ono".to_owned(),
        format!("Version: {}", env!("CARGO_PKG_VERSION")),
        "Section: shells".to_owned(),
        format!("Architecture: {}", debian_arch()),
    ] {
        assert!(
            info.contains(&field),
            "the control file carries `{field}`:\n{info}"
        );
    }
    let depends = info
        .lines()
        .find_map(|line| line.trim().strip_prefix("Depends:"))
        .expect("the package declares its dependencies");
    assert!(
        depends.contains("debianutils"),
        "add-shell/remove-shell come from debianutils, so the package depends on it: {depends}"
    );
    assert!(
        depends.contains("libc6"),
        "the shared-library dependencies are computed, not guessed: {depends}"
    );

    let contents = run("dpkg-deb", &["--contents", deb_path], &target_dir);
    for path in [
        "./usr/bin/ono",
        "./usr/share/doc/ono/copyright",
        "./usr/share/doc/ono/README.md",
        "./usr/share/doc/ono/reference/commands.md",
        "./usr/share/doc/ono/reference/adapters/README.md",
    ] {
        assert!(
            contents.lines().any(|line| line.ends_with(path)),
            "the package ships {path}:\n{contents}"
        );
    }
    let binary = contents
        .lines()
        .find(|line| line.ends_with("./usr/bin/ono"))
        .expect("the binary is listed");
    assert!(
        binary.starts_with("-rwxr-xr-x"),
        "/usr/bin/ono is installed executable for everyone: {binary}"
    );

    let control = target_dir.join("control");
    run(
        "dpkg-deb",
        &["--control", deb_path, control.to_str().unwrap()],
        &target_dir,
    );
    let postinst = std::fs::read_to_string(control.join("postinst")).expect("a postinst script");
    assert!(
        postinst.contains("add-shell /usr/bin/ono"),
        "installing registers /usr/bin/ono in /etc/shells through add-shell:\n{postinst}"
    );
    let postrm = std::fs::read_to_string(control.join("postrm")).expect("a postrm script");
    assert!(
        postrm.contains("remove-shell /usr/bin/ono"),
        "removing unregisters /usr/bin/ono through remove-shell:\n{postrm}"
    );
    assert!(
        !control.join("conffiles").exists(),
        "the package owns no configuration files"
    );
    let _ = std::fs::remove_dir_all(&target_dir);
}

#[test]
fn should_build_an_rpm_that_installs_ono_as_a_registered_login_shell() {
    let target_dir = staged_target_dir("rpm");
    let rpm_path = target_dir.join("ono.rpm");
    run(
        "cargo",
        &[
            "generate-rpm",
            "--package",
            "crates/ono-cli",
            "--target-dir",
            target_dir.to_str().unwrap(),
            "--output",
            rpm_path.to_str().unwrap(),
        ],
        &target_dir,
    );
    let bytes = std::fs::read(&rpm_path).expect("the rpm was written");
    let header = rpm::Header::parse(&bytes);

    assert_eq!(header.string(rpm::NAME), "ono");
    assert_eq!(header.string(rpm::VERSION), env!("CARGO_PKG_VERSION"));
    assert_eq!(header.string(rpm::RELEASE), "1");
    assert_eq!(header.string(rpm::ARCH), std::env::consts::ARCH);
    assert_eq!(header.string(rpm::LICENSE), "MIT");
    assert!(
        !header.string(rpm::SUMMARY).is_empty(),
        "the package carries a summary"
    );

    let files = header.files();
    for path in [
        "/usr/bin/ono",
        "/usr/share/licenses/ono/LICENSE",
        "/usr/share/doc/ono/README.md",
        "/usr/share/doc/ono/reference/commands.md",
        "/usr/share/doc/ono/reference/adapters/README.md",
    ] {
        assert!(
            files.contains(&path.to_owned()),
            "the package ships {path}: {files:?}"
        );
    }
    let requires = header.strings(rpm::REQUIRENAME);
    assert!(
        requires.iter().any(|name| name.starts_with("libc.so.6")),
        "the shared-library dependencies are computed, not guessed: {requires:?}"
    );

    let post_install = header.string(rpm::POSTIN);
    assert!(
        post_install.contains("/etc/shells") && post_install.contains("/usr/bin/ono"),
        "installing registers /usr/bin/ono in /etc/shells:\n{post_install}"
    );
    let post_uninstall = header.string(rpm::POSTUN);
    assert!(
        post_uninstall.contains("/etc/shells") && post_uninstall.contains("/usr/bin/ono"),
        "removing unregisters /usr/bin/ono from /etc/shells:\n{post_uninstall}"
    );
    let _ = std::fs::remove_dir_all(&target_dir);
}

/// Just enough of the RPM file format (lead, signature header, header) to read the tags a
/// package check needs — the gate machine has no `rpm`; the containers of
/// `scripts/package-check.sh` do.
mod rpm {
    use super::BTreeMap;

    pub const NAME: i32 = 1000;
    pub const VERSION: i32 = 1001;
    pub const RELEASE: i32 = 1002;
    pub const SUMMARY: i32 = 1004;
    pub const LICENSE: i32 = 1014;
    pub const ARCH: i32 = 1022;
    pub const POSTIN: i32 = 1024;
    pub const POSTUN: i32 = 1026;
    pub const REQUIRENAME: i32 = 1049;
    pub const FILEMODES: i32 = 1030;
    pub const FILEMTIMES: i32 = 1034;
    pub const FILEUSERNAME: i32 = 1039;
    pub const FILEGROUPNAME: i32 = 1040;
    const DIRINDEXES: i32 = 1116;
    const BASENAMES: i32 = 1117;
    const DIRNAMES: i32 = 1118;

    const LEAD_MAGIC: [u8; 4] = [0xed, 0xab, 0xee, 0xdb];
    const HEADER_MAGIC: [u8; 4] = [0x8e, 0xad, 0xe8, 0x01];
    const LEAD_LEN: usize = 96;

    #[derive(Debug)]
    pub enum Value {
        Int16(Vec<u32>),
        Int32(Vec<u32>),
        Strings(Vec<String>),
    }

    pub struct Header {
        tags: BTreeMap<i32, Value>,
    }

    fn be_u32(bytes: &[u8], at: usize) -> u32 {
        u32::from_be_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    }

    /// Parses one header structure starting at `at`; returns its tags and the offset after it.
    fn parse_section(bytes: &[u8], at: usize) -> (BTreeMap<i32, Value>, usize) {
        assert_eq!(
            &bytes[at..at + 4],
            &HEADER_MAGIC,
            "a header structure at {at}"
        );
        let count = be_u32(bytes, at + 8) as usize;
        let data_len = be_u32(bytes, at + 12) as usize;
        let index = at + 16;
        let data = index + count * 16;
        let mut tags = BTreeMap::new();
        for entry in 0..count {
            let at = index + entry * 16;
            let tag = be_u32(bytes, at) as i32;
            let kind = be_u32(bytes, at + 4);
            let offset = data + be_u32(bytes, at + 8) as usize;
            let n = be_u32(bytes, at + 12) as usize;
            let value = match kind {
                3 => Value::Int16(
                    (0..n)
                        .map(|i| {
                            u32::from(u16::from_be_bytes(
                                bytes[offset + i * 2..offset + i * 2 + 2]
                                    .try_into()
                                    .expect("two bytes"),
                            ))
                        })
                        .collect(),
                ),
                4 => Value::Int32((0..n).map(|i| be_u32(bytes, offset + i * 4)).collect()),
                6 | 8 | 9 => {
                    let mut strings = Vec::with_capacity(n);
                    let mut cursor = offset;
                    for _ in 0..n {
                        let end = bytes[cursor..]
                            .iter()
                            .position(|byte| *byte == 0)
                            .expect("a terminated string");
                        strings.push(
                            String::from_utf8_lossy(&bytes[cursor..cursor + end]).into_owned(),
                        );
                        cursor += end + 1;
                    }
                    Value::Strings(strings)
                }
                _ => continue,
            };
            tags.insert(tag, value);
        }
        (tags, data + data_len)
    }

    impl Header {
        pub fn parse(bytes: &[u8]) -> Self {
            assert_eq!(&bytes[..4], &LEAD_MAGIC, "an RPM lead");
            let (_signature, after_signature) = parse_section(bytes, LEAD_LEN);
            let header_at = after_signature.div_ceil(8) * 8;
            let (tags, _) = parse_section(bytes, header_at);
            Self { tags }
        }

        pub fn strings(&self, tag: i32) -> Vec<String> {
            match self.tags.get(&tag) {
                Some(Value::Strings(strings)) => strings.clone(),
                other => panic!("tag {tag} is a string tag, found {other:?}"),
            }
        }

        /// The numeric values of an int16 or int32 tag.
        pub fn numbers(&self, tag: i32) -> Vec<u32> {
            match self.tags.get(&tag) {
                Some(Value::Int16(values) | Value::Int32(values)) => values.clone(),
                other => panic!("tag {tag} is a numeric tag, found {other:?}"),
            }
        }

        pub fn string(&self, tag: i32) -> String {
            self.strings(tag).into_iter().next().unwrap_or_default()
        }

        pub fn files(&self) -> Vec<String> {
            let dirs = self.strings(DIRNAMES);
            let indexes = match self.tags.get(&DIRINDEXES) {
                Some(Value::Int32(indexes)) => indexes.clone(),
                other => panic!("dir indexes are int32, found {other:?}"),
            };
            self.strings(BASENAMES)
                .into_iter()
                .zip(indexes)
                .map(|(base, dir)| format!("{}{base}", dirs[dir as usize]))
                .collect()
        }
    }
}

// --- the acceptance image must be built from the sources it copies ------------------------------

#[test]
fn should_stamp_the_workspace_before_building_when_the_image_caches_its_target_directory() {
    // The referee's own referee. `docker/Dockerfile` keeps `target/` in a BuildKit cache mount,
    // and cargo decides what to rebuild from mtimes: `COPY` preserves the host's, so a source
    // file edited *before* the cached artifacts were written looks older than them and the crate
    // is declared fresh. The image then ships the previous binary while carrying the new source,
    // and every acceptance case grades yesterday's code — silently, and while passing. This was
    // observed, not imagined: an image built from a tree containing `prompt.vcs` shipped a binary
    // that had never heard of it.
    //
    // Stamping the workspace with the build's own clock is what closes it. The rule is asserted
    // here rather than trusted, because nothing else can notice its absence.
    let dockerfile =
        std::fs::read_to_string(repo().join("docker/Dockerfile")).expect("the image recipe");
    let Some(build) = dockerfile
        .split("RUN ")
        .find(|step| step.contains("cargo build --release"))
    else {
        panic!("docker/Dockerfile no longer builds the binary with `cargo build --release`");
    };

    if !build.contains("type=cache,target=/build/target") {
        // No cache mount, no staleness: a fresh target directory rebuilds everything anyway.
        ono_testkit::skipped(
            SkipReason::FixtureNotApplicable,
            "the Dockerfile mounts no build cache, so there is no stale artifact to guard against",
        );
        return;
    }
    assert!(
        build.contains("touch"),
        "the build step keeps `target/` in a cache mount and does not stamp the sources it \
         copied, so cargo can declare a changed crate fresh and the image can ship the previous \
         binary. Restore the `find … -exec touch {{}} +` before `cargo build`, or drop the cache \
         mount. Step:\n{build}"
    );
    let stamp = build.find("touch").expect("the stamp");
    let compile = build.find("cargo build").expect("the build");
    assert!(
        stamp < compile,
        "the sources are stamped after the build, which stamps nothing that mattered"
    );
}

/// Spec §44.4: a release build fails when lockfile resolution would change.
///
/// `--locked` is one word in `scripts/package.sh` and in `docker/Dockerfile`, and the whole
/// reproducibility contract of §46 rests on it: without it cargo silently re-resolves, and the
/// same commit built a month apart is built against different code. This arranges the failure
/// from outside — a workspace whose lockfile no longer describes its manifest — and proves the
/// build refuses rather than repairs, and that `--locked` is what refuses it.
///
/// What it pins is cargo's half of the contract, so it was green the day it was written. The
/// repository's half — that every release build actually passes the flag — is
/// `xtask/tests/supply_chain.rs::should_build_the_release_with_a_locked_dependency_graph`, and
/// that one was not (ADR-0450).
#[test]
fn should_refuse_a_release_build_whose_lockfile_would_change() {
    let workspace = scratch();
    workspace.write(
        "Cargo.toml",
        "[workspace]\n\n[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n\
         edition = \"2021\"\n\n[dependencies]\nlate = { path = \"late\" }\n",
    );
    workspace.write("src/lib.rs", "");
    workspace.write(
        "late/Cargo.toml",
        "[package]\nname = \"late\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    workspace.write("late/src/lib.rs", "");
    // A lockfile from before `late` was depended on: resolution would have to change to build.
    workspace.write(
        "Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    );

    let build = |locked: bool| {
        let mut command = Command::new("cargo");
        command
            .arg("build")
            .arg("--offline")
            .current_dir(workspace.path())
            .env("CARGO_TARGET_DIR", workspace.path().join("target"));
        if locked {
            command.arg("--locked");
        }
        let output = command
            .output()
            .unwrap_or_else(|error| panic!("cargo must be runnable in the gate: {error}"));
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    let (built, complaint) = build(true);
    assert!(
        !built,
        "a build with a stale lockfile succeeded under --locked, so nothing stops a release \
         from re-resolving its dependency graph (spec §44.4):\n{complaint}"
    );
    assert!(
        complaint.contains("lock file") && complaint.contains("--locked"),
        "the refusal does not say the lockfile is what stopped it:\n{complaint}"
    );

    let (built, complaint) = build(false);
    assert!(
        built,
        "the same workspace does not build without --locked either, so the fixture proves \
         nothing about the flag:\n{complaint}"
    );
}

// --- the determinism inputs (spec §46.2–§46.4, ADR-0526) ----------------------------------------

/// Runs `scripts/package.sh --print-determinism` under `root` and returns what it decided.
///
/// The flag exists so the four inputs can be read without building anything: they are fixed
/// before the first tool runs, which is the only place a test can observe them from outside.
fn determinism_inputs(root: &Path, environment: &[(&str, &str)]) -> (bool, String) {
    let mut command = Command::new("bash");
    command
        .arg(root.join("scripts/package.sh"))
        .arg("--print-determinism")
        .current_dir(root);
    command.env_remove("SOURCE_DATE_EPOCH");
    for (name, value) in environment {
        command.env(name, value);
    }
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("bash must be runnable in the gate: {error}"));
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

#[test]
fn should_set_every_determinism_input_before_a_release_build() {
    // A developer workstation in Berlin, with a German locale and a private umask. None of it
    // may reach the artifact (spec §46.3, §46.4).
    let (built, report) = determinism_inputs(
        &repo(),
        &[
            ("LC_ALL", "de_DE.UTF-8"),
            ("LANG", "de_DE.UTF-8"),
            ("LANGUAGE", "de:en"),
            ("TZ", "Europe/Berlin"),
        ],
    );
    assert!(
        built,
        "`scripts/package.sh --print-determinism` refused a perfectly ordinary environment:\n\
         {report}"
    );

    for setting in ["LC_ALL=C.UTF-8", "LANG=C.UTF-8", "TZ=UTC", "umask=0022"] {
        assert!(
            report.contains(setting),
            "the release build does not fix `{setting}`, so the locale, timezone or file mode of \
             whoever ran it can reach the package (spec §46.3, §46.4):\n{report}"
        );
    }

    // §46.4: the packages carry the release's ownership, never the workstation's UID/GID.
    assert!(
        report.contains("owner=0:0"),
        "the release build does not state the ownership its packages carry, so it inherits the \
         developer's UID/GID (spec §46.4):\n{report}"
    );

    // §46.2: derived from the commit, so the same commit always yields the same timestamp. A
    // wall clock would move between two builds of one commit, which is precisely what §46.5
    // then fails on.
    let committed = Command::new("git")
        .args(["log", "-1", "--format=%ct"])
        .current_dir(repo())
        .output()
        .expect("git must be runnable in the gate");
    let epoch = String::from_utf8_lossy(&committed.stdout).trim().to_owned();
    assert!(
        !epoch.is_empty(),
        "the repository has no commit to date from"
    );
    assert!(
        report.contains(&format!("SOURCE_DATE_EPOCH={epoch}")),
        "SOURCE_DATE_EPOCH is not the release commit's own timestamp, so a wall-clock build time \
         can reach an artifact field (spec §46.2). Expected {epoch}:\n{report}"
    );
}

#[test]
fn should_refuse_a_release_build_that_leaves_a_determinism_input_unset() {
    // A tree the packaging script can reach but git cannot describe: there is no commit to date
    // the artifacts from, and §46.2 leaves exactly one alternative — the wall clock — which is
    // the thing it forbids. So the build refuses.
    let elsewhere = scratch();
    std::fs::create_dir_all(elsewhere.path().join("scripts")).expect("a scratch scripts/ dir");
    std::fs::copy(
        repo().join("scripts/package.sh"),
        elsewhere.path().join("scripts/package.sh"),
    )
    .expect("the packaging script is copied");

    let (built, report) = determinism_inputs(
        elsewhere.path(),
        &[(
            "GIT_DIR",
            elsewhere
                .path()
                .join("there-is-no-repository-here")
                .to_str()
                .expect("a UTF-8 scratch path"),
        )],
    );
    assert!(
        !built,
        "the release build proceeded with no derivable SOURCE_DATE_EPOCH, so its artifacts would \
         carry whatever the clock said (spec §46.2):\n{report}"
    );
    assert!(
        report.contains("SOURCE_DATE_EPOCH"),
        "the refusal does not name the determinism input that was missing, so nobody can fix \
         it:\n{report}"
    );

    // And a value that is present but not a timestamp is refused the same way: `unset` means
    // "the pipeline cannot derive a deterministic one", not "the variable is empty".
    let (built, report) = determinism_inputs(&repo(), &[("SOURCE_DATE_EPOCH", "yesterday")]);
    assert!(
        !built && report.contains("SOURCE_DATE_EPOCH"),
        "a SOURCE_DATE_EPOCH that is not a timestamp was accepted:\n{report}"
    );
}

#[test]
fn should_normalize_file_ownership_and_mode_in_every_produced_package() {
    // One fixed epoch, and both packages built from it: every member of both archives must carry
    // it, owned by root, with the mode the manifest declares — not the mode of the file on the
    // machine that packaged it (spec §46.4).
    const EPOCH: &str = "1700000000";

    let target_dir = staged_target_dir("determinism");
    let deb = target_dir.join("ono.deb");
    let deb_path = deb.to_str().expect("a UTF-8 scratch path");
    let mut command = Command::new("cargo");
    command
        .args([
            "deb",
            "--package",
            "ono-cli",
            "--no-build",
            "--no-strip",
            "--output",
            deb_path,
        ])
        .current_dir(repo())
        .env("CARGO_TARGET_DIR", &target_dir)
        .env("SOURCE_DATE_EPOCH", EPOCH)
        .env("LC_ALL", "C.UTF-8")
        .env("TZ", "UTC");
    let output = command.output().expect("cargo-deb must be runnable");
    assert!(
        output.status.success(),
        "cargo deb failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // `dpkg-deb --contents` renders mtimes in the *reader's* timezone, so the reader is given
    // one too. The archive itself carries the epoch; what varies here is only the rendering.
    let listing = Command::new("dpkg-deb")
        .args(["--contents", deb_path])
        .env("TZ", "UTC")
        .env("LC_ALL", "C.UTF-8")
        .output()
        .expect("dpkg-deb must be runnable in the gate");
    let listing = String::from_utf8_lossy(&listing.stdout).into_owned();
    assert!(!listing.trim().is_empty(), "the .deb lists no members");
    for line in listing.lines() {
        assert!(
            line.contains(" 0/0 ") || line.contains(" root/root "),
            "a .deb member is not owned by uid 0/gid 0, so the packager's UID/GID reached the \
             archive (spec §46.4): {line}"
        );
        assert!(
            line.contains("2023-11-14 22:13"),
            "a .deb member does not carry SOURCE_DATE_EPOCH, so its mtime is the moment it was \
             packaged (spec §46.2, §46.4): {line}"
        );
    }
    let binary = listing
        .lines()
        .find(|line| line.ends_with("./usr/bin/ono"))
        .expect("the binary is listed");
    assert!(
        binary.starts_with("-rwxr-xr-x"),
        "/usr/bin/ono does not carry the declared 755: {binary}"
    );

    let rpm_path = target_dir.join("ono.rpm");
    let output = Command::new("cargo")
        .args([
            "generate-rpm",
            "--package",
            "crates/ono-cli",
            "--target-dir",
            target_dir.to_str().unwrap(),
            "--output",
            rpm_path.to_str().unwrap(),
        ])
        .current_dir(repo())
        .env("CARGO_TARGET_DIR", &target_dir)
        .env("SOURCE_DATE_EPOCH", EPOCH)
        .env("LC_ALL", "C.UTF-8")
        .env("TZ", "UTC")
        .output()
        .expect("cargo-generate-rpm must be runnable");
    assert!(
        output.status.success(),
        "cargo generate-rpm failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = std::fs::read(&rpm_path).expect("the rpm was written");
    let header = rpm::Header::parse(&bytes);

    let owners = header.strings(rpm::FILEUSERNAME);
    let groups = header.strings(rpm::FILEGROUPNAME);
    assert!(
        !owners.is_empty()
            && owners.iter().all(|owner| owner == "root")
            && groups.iter().all(|group| group == "root"),
        "an rpm member is not owned by root:root, so the packager's identity reached the package \
         (spec §46.4): {owners:?} {groups:?}"
    );
    let times = header.numbers(rpm::FILEMTIMES);
    let epoch: u32 = EPOCH.parse().expect("a numeric epoch");
    assert!(
        !times.is_empty() && times.iter().all(|time| *time == epoch),
        "an rpm member does not carry SOURCE_DATE_EPOCH, so its mtime is the moment it was \
         packaged (spec §46.2, §46.4): {times:?}"
    );
    let modes = header.numbers(rpm::FILEMODES);
    assert!(
        modes.iter().all(|mode| mode & 0o7000 == 0),
        "an rpm member carries a setuid, setgid or sticky bit: {modes:?}"
    );
    let files = header.files();
    let binary = files
        .iter()
        .position(|path| path == "/usr/bin/ono")
        .expect("the binary is packaged");
    assert_eq!(
        modes[binary] & 0o7777,
        0o755,
        "/usr/bin/ono does not carry the declared 755"
    );

    let _ = std::fs::remove_dir_all(&target_dir);
}

// --- two clean builds of one commit (spec §46.1, §46.5, §46.6, ADR-0527) ------------------------

/// The SHA-256 of some bytes, spelled the way `sha256sum` spells it.
fn sha256_of(bytes: &[u8]) -> String {
    xtask::reproducibility::digest(bytes)
}

/// Runs `scripts/rebuild-check.sh`, which builds every artifact twice and compares the bytes.
fn rebuild_check(arguments: &[&str]) -> (bool, String) {
    let output = Command::new("bash")
        .arg(repo().join("scripts/rebuild-check.sh"))
        .args(arguments)
        .current_dir(repo())
        .output()
        .unwrap_or_else(|error| panic!("bash must be runnable in the gate: {error}"));
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

#[test]
fn should_produce_identical_hashes_for_two_clean_builds_of_one_commit() {
    // §46.5 in the form a gate can run: one commit, one binary, two packaging runs in two clean
    // environments that disagree about locale, timezone, umask, temporary directory and build
    // directory — and the same bytes out of both.
    let work = scratch();
    let staged = staged_target_dir("rebuild");
    let (identical, report) = rebuild_check(&[
        "--binary",
        staged
            .join("release/ono")
            .to_str()
            .expect("a UTF-8 scratch path"),
        "--work",
        work.path().to_str().expect("a UTF-8 scratch path"),
    ]);
    assert!(
        identical,
        "two clean builds of one commit did not produce identical artifacts (spec §46.1, \
         §46.5):\n{report}"
    );
    assert!(
        report.contains(".deb") && report.contains(".rpm"),
        "the comparison covered neither package, so it proved nothing about the release \
         targets of §46.1:\n{report}"
    );

    // And what it compared is what a release publishes: both directories hold the same set of
    // artifacts, and every pair hashes the same.
    let left = work.path().join("a/dist");
    let right = work.path().join("b/dist");
    let digests = |directory: &Path| -> BTreeMap<String, String> {
        let mut found = BTreeMap::new();
        for entry in std::fs::read_dir(directory)
            .unwrap_or_else(|error| panic!("{} is readable: {error}", directory.display()))
        {
            let entry = entry.expect("a directory entry");
            let bytes = std::fs::read(entry.path()).expect("an artifact is readable");
            found.insert(
                entry.file_name().to_string_lossy().into_owned(),
                sha256_of(&bytes),
            );
        }
        found
    };
    let left = digests(&left);
    assert!(
        left.keys().any(|name| name.ends_with(".deb")) && left.keys().any(|n| n.ends_with(".rpm")),
        "the first build produced no packages: {left:?}"
    );
    assert_eq!(
        left,
        digests(&right),
        "the two builds disagree about at least one artifact"
    );
    let _ = std::fs::remove_dir_all(&staged);
}

#[test]
fn should_name_the_differing_archive_member_when_a_seeded_difference_is_introduced() {
    // §46.5: "A mismatch MUST fail the release check and produce a diagnostic identifying which
    // files/archive members differ where tooling permits." A comparison that only says "these
    // two files differ" leaves the maintainer where they started, so the difference is seeded
    // deliberately and the diagnostic has to name the member it landed in.
    let work = scratch();
    let staged = staged_target_dir("seeded");
    let (identical, report) = rebuild_check(&[
        "--binary",
        staged
            .join("release/ono")
            .to_str()
            .expect("a UTF-8 scratch path"),
        "--work",
        work.path().to_str().expect("a UTF-8 scratch path"),
    ]);
    assert!(
        identical,
        "the unseeded comparison must be clean:\n{report}"
    );

    let deb = std::fs::read_dir(work.path().join("b/dist"))
        .expect("the second build's artifacts")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|extension| extension == "deb"))
        .expect("a .deb to seed a difference into");
    // The payload member, changed in place so the archive stays a well-formed `ar`: this is the
    // shape a genuine non-reproducibility takes, not a truncated file.
    let mut bytes = std::fs::read(&deb).expect("the package is readable");
    let payload = seeded_payload_offset(&bytes);
    bytes[payload] ^= 0xff;
    std::fs::write(&deb, &bytes).expect("the seeded package is written");

    let (identical, report) = rebuild_check(&[
        "--compare",
        work.path().join("a/dist").to_str().unwrap(),
        work.path().join("b/dist").to_str().unwrap(),
    ]);
    assert!(
        !identical,
        "a seeded difference was not detected, so the comparison proves nothing:\n{report}"
    );
    assert!(
        report.contains("data.tar"),
        "the diagnostic does not name the archive member that differs, so it leaves the \
         maintainer with two hashes and no lead (spec §46.5):\n{report}"
    );
    assert!(
        report.contains(
            deb.file_name()
                .expect("a file name")
                .to_str()
                .expect("a UTF-8 name")
        ),
        "the diagnostic does not name the artifact that differs:\n{report}"
    );
    let _ = std::fs::remove_dir_all(&staged);
}

/// The offset of the first byte of a `.deb`'s `data.tar.*` member.
fn seeded_payload_offset(bytes: &[u8]) -> usize {
    let mut cursor = 8; // the `!<arch>\n` magic
    while cursor + 60 <= bytes.len() {
        let header = &bytes[cursor..cursor + 60];
        let name = String::from_utf8_lossy(&header[0..16]).trim().to_owned();
        let size: usize = String::from_utf8_lossy(&header[48..58])
            .trim()
            .parse()
            .expect("an ar member length");
        let body = cursor + 60;
        if name.starts_with("data.tar") {
            return body + size / 2;
        }
        cursor = body + size + size % 2;
    }
    panic!("the .deb carries no data.tar member");
}

#[test]
fn should_require_reproducibility_of_every_supported_architecture_separately() {
    // §46.6: "Reproducibility is not proven merely because x86_64 is stable if aarch64 artifacts
    // differ between clean builds." The comparison therefore runs inside the per-architecture
    // packaging job, where each runner is native, rather than once at the end.
    let workflow = support::read(".github/workflows/release.yml");
    assert!(
        workflow.contains("rebuild-check.sh"),
        "the release workflow never compares two builds, so §46.5 is asserted nowhere a release \
         passes through:\n{workflow}"
    );

    // The comparison runs per architecture: `rebuild` builds the same commit on a second
    // runner — the freshest clean environment this repository can reach — and `reproducibility`
    // compares the two, once for each supported architecture.
    for job in ["rebuild:", "reproducibility:"] {
        assert!(
            workflow.contains(&format!("\n  {job}")),
            "the release workflow has no `{job}` job, so nothing builds this commit a second \
             time (spec §46.5):\n{workflow}"
        );
    }
    let comparison = workflow_job(&workflow, "reproducibility");
    for architecture in ["x86_64", "aarch64"] {
        assert!(
            comparison.contains(architecture),
            "the comparison does not cover {architecture}, so that architecture can differ \
             between two clean builds without the release noticing (spec §46.6):\n{comparison}"
        );
    }
    assert!(
        comparison.contains("--compare"),
        "the comparison job does not compare two builds:\n{comparison}"
    );

    // And publication waits for it: a release that ships before the comparison finishes has
    // learned nothing from it.
    let publish = workflow_job(&workflow, "publish");
    assert!(
        publish.contains("reproducibility"),
        "publication does not wait for the reproducibility comparison:\n{publish}"
    );

    // And the local release gate runs it too, so the rule is not something only CI knows.
    let release_check = support::read("scripts/release-check.sh");
    assert!(
        release_check.contains("rebuild-check.sh"),
        "scripts/release-check.sh does not compare two builds, so §46.5 is not part of release \
         qualification:\n{release_check}"
    );
}

// --- package validation (spec §48.1–§48.3, ADR-0531) --------------------------------------------

/// The nine checks §48.2 adds, each named in `scripts/package-check.sh` the way the spec names it.
///
/// A marker and the machinery that carries it, because a comment nobody runs is not a check.
const NEW_PACKAGE_CHECKS: [(&str, &str); 9] = [
    ("binary version equals release version", "ono --version"),
    ("expected path /usr/bin/ono exists", "-x /usr/bin/ono"),
    ("file ownership and mode are correct", "stat -c"),
    ("no private build paths are embedded", "/home/"),
    (
        "package metadata matches the artifact filename",
        "FILE_VERSION",
    ),
    ("uninstall leaves user configuration", ".config/ono"),
    ("reinstall works", "reinstall"),
    ("login-shell smoke behaviour", "getent passwd probe"),
    ("the checksum manifest matches the file", "sha256sum"),
];

#[test]
fn should_run_every_new_package_check_the_specification_lists() {
    let script = support::read("scripts/package-check.sh");
    for (check, machinery) in NEW_PACKAGE_CHECKS {
        assert!(
            script.contains(check),
            "`scripts/package-check.sh` does not run the §48.2 check `{check}`, so a package can \
             be released without it"
        );
        assert!(
            script.contains(machinery),
            "the §48.2 check `{check}` is named and nothing carries it out — `{machinery}` is \
             absent from the script"
        );
    }

    // §48.1: the existing real-install checks stay. The new ones are added beside them, not in
    // place of them.
    for existing in [
        "dpkg-deb --info",
        "rpm -qp",
        "apt-get install",
        "dnf",
        "/etc/shells",
    ] {
        assert!(
            script.contains(existing),
            "the real-install checks of §48.1 no longer run: `{existing}` is gone"
        );
    }

    // §48.4 in the form this script owns: what it validated is recorded by digest, so #110 can
    // compare it with what is published rather than trusting that they are the same build.
    assert!(
        script.contains("package-check.sha256"),
        "package validation does not record the digest of what it installed, so nothing can \
         later prove the published asset is the artifact that was tested (spec §48.4, §62.6)"
    );
}

#[test]
fn should_run_package_validation_on_the_oldest_supported_baseline_as_well_as_a_current_one() {
    let script = support::read("scripts/package-check.sh");

    // §48.3: the oldest supported baseline *as well as* one current representative. The baseline
    // is the binding compatibility proof, so it is the one that is named as such.
    for (label, image) in [
        ("the oldest supported baseline", "debian:bookworm@sha256:"),
        ("a current representative", "debian:trixie@sha256:"),
    ] {
        assert!(
            script.contains(image),
            "package validation names no image for {label} (spec §48.3). Expected a digest-pinned \
             `{image}…` (spec §44.1)"
        );
    }
    assert!(
        script.contains("fedora:latest@sha256:"),
        "the .rpm is validated on no distribution at all (spec §48.1)"
    );

    // The baseline is only a compatibility proof if the binary can actually run there, so the
    // glibc floor is stated and checked rather than assumed from the image name.
    assert!(
        script.contains("GLIBC_FLOOR") && script.contains("2.36"),
        "the script does not state the glibc floor its baseline proves, so `oldest supported` is \
         a claim about an image tag rather than about the binary (spec §48.3):\n{script}"
    );

    // And the .deb path runs in both, rather than the second image being pulled and looked at.
    let runs = script.matches("DEBIAN_IMAGES").count();
    assert!(
        runs >= 2,
        "the two Debian images are named and the checks do not run over both of them (spec \
         §48.3)"
    );

    // The build image's own base is the baseline: a floor the release does not build on is a
    // floor nobody has tested.
    let build = support::read("scripts/package.sh");
    assert!(
        build.contains("bookworm"),
        "the release is built on a base other than the baseline package validation proves, so \
         the two say different things about the oldest supported distribution (spec §48.3):\n\
         {build}"
    );
}
