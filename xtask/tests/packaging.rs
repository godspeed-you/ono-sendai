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

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits in the workspace")
        .to_path_buf()
}

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
    const DIRINDEXES: i32 = 1116;
    const BASENAMES: i32 = 1117;
    const DIRNAMES: i32 = 1118;

    const LEAD_MAGIC: [u8; 4] = [0xed, 0xab, 0xee, 0xdb];
    const HEADER_MAGIC: [u8; 4] = [0x8e, 0xad, 0xe8, 0x01];
    const LEAD_LEN: usize = 96;

    #[derive(Debug)]
    pub enum Value {
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
