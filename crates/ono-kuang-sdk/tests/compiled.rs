//! A component runs from the artifact `kuang-compile` wrote, and from nothing else (ADR-0870).
//!
//! The shell links the WebAssembly runtime without a compiler. These are the outcomes that makes
//! observable at the host boundary: a component nobody compiled, one compiled by another engine
//! or for another architecture, one whose artifact is damaged, and one whose artifact someone else
//! could have written are each refused with `load.component_not_compiled`, naming the command
//! that fixes it — and never crash the host.
//!
//! The component these tests compile is the smallest one there is, the eight-byte preamble: the
//! refusals are decided before anything is instantiated, so none of them needs the example
//! package built for `wasm32-wasip2`. That the tool's artifact loads and runs is the wasm-tier
//! conformance suite's job, which compiles the real component through this same tool.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "AGENTS.md §16: a test states its preconditions directly"
)]

use std::path::{Path, PathBuf};

use ono_kuang_protocol::{KuangError, KuangErrorCode};
use ono_kuang_testhost::TestHost;

const COMPILE: &str = env!("CARGO_BIN_EXE_kuang-compile");

/// The empty component: the magic, the component-model version, the component layer.
const EMPTY_COMPONENT: &[u8] = b"\0asm\x0d\x00\x01\x00";

fn manifest() -> String {
    r#"
format: kuang-package/1
package:
  id: dev.example.empty
  name: empty
  version: 0.1.0
  description: A component with nothing in it.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64, linux-arm64]
runtime:
  kind: wasm-component
  entry: runtime/empty.wasm
  memory_max: 64MiB
  cpu_budget: interactive
  startup: lazy
roles: [provider]
network:
  outbound: none
"#
    .to_owned()
}

/// A package directory holding `component` as its entry, and an empty store beside it.
struct Scene {
    root: tempfile::TempDir,
}

impl Scene {
    fn with(component: &[u8]) -> Self {
        let root = tempfile::tempdir().expect("a scratch directory");
        std::fs::create_dir_all(root.path().join("package/runtime")).expect("the package");
        std::fs::write(root.path().join("package/runtime/empty.wasm"), component)
            .expect("the component");
        Self { root }
    }

    fn component(&self) -> PathBuf {
        self.root.path().join("package/runtime/empty.wasm")
    }

    fn store(&self) -> PathBuf {
        self.root.path().join("store")
    }

    /// Runs the SDK's tool on the component, as `install plugin` does.
    fn compile(&self) -> PathBuf {
        self.compile_into(&self.store())
    }

    /// Runs the SDK's tool on the component, into `store`.
    fn compile_into(&self, store: &Path) -> PathBuf {
        let run = std::process::Command::new(COMPILE)
            .arg("--store")
            .arg(store)
            .arg(self.component())
            .output()
            .expect("kuang-compile runs");
        assert!(
            run.status.success(),
            "kuang-compile compiles a component: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        let printed = String::from_utf8(run.stdout).expect("a path");
        let artifact = PathBuf::from(printed.trim());
        assert!(
            artifact.starts_with(store) && artifact.is_file(),
            "kuang-compile prints the artifact it wrote into the store, got {artifact:?}"
        );
        artifact
    }

    async fn load(&self) -> Result<ono_kuang_supervisor::LoadedPlugin, KuangError> {
        TestHost::new(self.component(), &manifest())
            .compiled(self.store())
            .load()
            .await
    }

    /// The refusal the load ends with; a load that succeeds, or fails another way, fails the test.
    async fn refusal(&self) -> KuangError {
        match self.load().await {
            Ok(_) => panic!("the component loaded, and it had no artifact this host may map"),
            Err(error) => {
                assert_eq!(
                    error.code(),
                    KuangErrorCode::LoadComponentNotCompiled,
                    "ADR-0870: the refusal is `load.component_not_compiled`, got {error:?}"
                );
                error
            }
        }
    }
}

/// Whether `directory`'s group is one only its owner belongs to: the private group a
/// `USERGROUPS` system gives every user, which the ownership rule treats as the owner.
fn group_is_private(directory: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = std::fs::metadata(directory).expect("the directory");
    let group = std::process::Command::new("getent")
        .args(["group", &metadata.gid().to_string()])
        .output()
        .map(|run| String::from_utf8_lossy(&run.stdout).trim().to_owned())
        .unwrap_or_default();
    let user = std::process::Command::new("id")
        .arg("-un")
        .output()
        .map(|run| String::from_utf8_lossy(&run.stdout).trim().to_owned())
        .unwrap_or_default();
    let fields: Vec<&str> = group.split(':').collect();
    fields.len() == 4
        && fields[0] == user
        && fields[3]
            .split(',')
            .all(|member| member.is_empty() || member == user)
}

fn running_as_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|run| String::from_utf8_lossy(&run.stdout).trim() == "0")
}

fn reason_of(error: &KuangError) -> &str {
    error
        .metadata()
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .expect("the refusal says why in its metadata")
}

/// The refusal names the exact step that fixes it, with the component's own path.
fn assert_names_the_compile_step(error: &KuangError, component: &Path) {
    let command = format!("kuang-compile {}", component.display());
    assert!(
        error.message().contains(&command),
        "the refusal names the command to run, `{command}`: {:?}",
        error.message()
    );
    assert_eq!(
        error
            .metadata()
            .get("command")
            .and_then(serde_json::Value::as_str),
        Some(command.as_str()),
        "and carries it as a fact a script can run"
    );
}

/// Where in the artifact the engine stamped its version and target: the offset of the release
/// in the version (`wasmtime-47.0.4`, ADR-0916) and of the architecture, found by the shape
/// wasmtime writes (a zero, the version's length and text, the target triple's length and text).
fn engine_stamp(artifact: &[u8]) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
    const PREFIX: &[u8] = b"wasmtime-";
    let arch = std::env::consts::ARCH.as_bytes();
    for at in 2..artifact.len().saturating_sub(PREFIX.len()) {
        if &artifact[at..at + PREFIX.len()] != PREFIX || artifact[at - 2] != 0 {
            continue;
        }
        let length = usize::from(artifact[at - 1]);
        let version_end = at + length;
        let release = at + PREFIX.len()..version_end;
        let triple = version_end + 1;
        if version_end < artifact.len()
            && artifact[release.clone()]
                .iter()
                .all(|byte| byte.is_ascii_digit() || *byte == b'.')
            && artifact.get(triple..triple + arch.len()) == Some(arch)
        {
            return (release, triple..triple + arch.len());
        }
    }
    panic!("the artifact carries the engine's version and target")
}

#[tokio::test]
async fn should_refuse_a_component_nobody_compiled_and_name_the_compile_step() {
    let scene = Scene::with(EMPTY_COMPONENT);
    let refused = scene.refusal().await;
    assert_eq!(reason_of(&refused), "missing");
    assert_names_the_compile_step(&refused, &scene.component());
    // An artifact is compiled for the engine and the architecture, and travels to every machine
    // of that architecture (ADR-0914): the help must not say it belongs to this one.
    let help = refused.help().unwrap_or_default();
    assert!(
        help.contains("architecture") && !help.contains("machine"),
        "{help:?}"
    );
}

#[tokio::test]
async fn should_load_a_component_once_the_compile_step_has_run() {
    let scene = Scene::with(EMPTY_COMPONENT);
    scene.compile();
    // The empty component exports no `wasi:cli/run`, so it cannot be started; what matters is
    // that the artifact was accepted, and the refusal of this ADR is not what stops it.
    if let Err(error) = scene.load().await {
        assert_ne!(
            error.code(),
            KuangErrorCode::LoadComponentNotCompiled,
            "the artifact the tool wrote is one the host maps: {error:?}"
        );
    }
}

#[tokio::test]
async fn should_refuse_a_changed_component_until_it_is_compiled_again() {
    // A package upgrade replaces the component; the artifact of the old bytes is not the new
    // one's. A custom section appended is the smallest change a component can undergo.
    let scene = Scene::with(EMPTY_COMPONENT);
    scene.compile();
    let mut changed = EMPTY_COMPONENT.to_vec();
    changed.extend_from_slice(b"\x00\x05\x04note");
    std::fs::write(scene.component(), &changed).expect("the upgraded component");
    let refused = scene.refusal().await;
    assert_eq!(reason_of(&refused), "missing");
    scene.compile();
    if let Err(error) = scene.load().await {
        assert_ne!(error.code(), KuangErrorCode::LoadComponentNotCompiled);
    }
}

#[tokio::test]
async fn should_refuse_an_artifact_another_engine_version_wrote_and_name_the_compile_step() {
    // A shell upgrade that changes the engine: the artifact is there, and stamped with a
    // version this engine is not.
    let scene = Scene::with(EMPTY_COMPONENT);
    let artifact = scene.compile();
    let mut bytes = std::fs::read(&artifact).expect("the artifact");
    let (version, _) = engine_stamp(&bytes);
    for digit in &mut bytes[version] {
        if digit.is_ascii_digit() {
            *digit = if *digit == b'0' { b'1' } else { *digit - 1 };
        }
    }
    std::fs::write(&artifact, &bytes).expect("the stale artifact");
    let refused = scene.refusal().await;
    assert_eq!(reason_of(&refused), "incompatible");
    assert!(
        refused.message().contains("version"),
        "the engine's own reason is in the refusal: {:?}",
        refused.message()
    );
    assert_names_the_compile_step(&refused, &scene.component());

    scene.compile();
    if let Err(error) = scene.load().await {
        assert_ne!(
            error.code(),
            KuangErrorCode::LoadComponentNotCompiled,
            "compiling again replaces the stale artifact: {error:?}"
        );
    }
}

#[tokio::test]
async fn should_refuse_an_artifact_compiled_for_another_architecture() {
    let foreign: &[u8] = match std::env::consts::ARCH {
        "x86_64" => b"mips64",
        "aarch64" => b"powerpc",
        other => panic!("this suite names a foreign architecture for {other}"),
    };
    let scene = Scene::with(EMPTY_COMPONENT);
    let artifact = scene.compile();
    let mut bytes = std::fs::read(&artifact).expect("the artifact");
    let (_, arch) = engine_stamp(&bytes);
    bytes[arch].copy_from_slice(foreign);
    std::fs::write(&artifact, &bytes).expect("the foreign artifact");
    let refused = scene.refusal().await;
    assert_eq!(reason_of(&refused), "incompatible");
    assert!(
        refused.message().contains("architecture"),
        "the engine says what did not match: {:?}",
        refused.message()
    );
    assert_names_the_compile_step(&refused, &scene.component());
}

#[tokio::test]
async fn should_refuse_a_truncated_or_garbled_artifact_rather_than_crash() {
    let scene = Scene::with(EMPTY_COMPONENT);
    let artifact = scene.compile();
    let whole = std::fs::read(&artifact).expect("the artifact");
    for (what, damaged) in [
        ("empty", Vec::new()),
        ("truncated to its header", whole[..64].to_vec()),
        ("truncated to half", whole[..whole.len() / 2].to_vec()),
        (
            "not an artifact",
            b"this is not an ELF file at all".to_vec(),
        ),
    ] {
        std::fs::write(&artifact, &damaged).expect("the damaged artifact");
        let refused = scene.refusal().await;
        assert_eq!(reason_of(&refused), "incompatible", "{what}");
        assert_names_the_compile_step(&refused, &scene.component());
    }
}

#[tokio::test]
async fn should_refuse_an_artifact_other_users_could_have_written() {
    use std::os::unix::fs::PermissionsExt as _;
    let scene = Scene::with(EMPTY_COMPONENT);
    let artifact = scene.compile();

    std::fs::set_permissions(&artifact, std::fs::Permissions::from_mode(0o666))
        .expect("a world-writable artifact");
    let refused = scene.refusal().await;
    assert_eq!(reason_of(&refused), "untrusted");
    assert!(
        refused.message().contains(&artifact.display().to_string()),
        "the refusal names the artifact it will not map: {:?}",
        refused.message()
    );

    std::fs::set_permissions(&artifact, std::fs::Permissions::from_mode(0o644))
        .expect("the artifact back to its owner");
    std::fs::set_permissions(scene.store(), std::fs::Permissions::from_mode(0o777))
        .expect("a world-writable store");
    assert_eq!(reason_of(&scene.refusal().await), "untrusted");
}

#[tokio::test]
async fn should_refuse_an_artifact_that_is_a_symbolic_link() {
    // The artifact is opened without following a link: a link in the store would make the
    // file whose owner and mode were examined a different one from the file that is mapped.
    let scene = Scene::with(EMPTY_COMPONENT);
    let artifact = scene.compile();
    let elsewhere = scene.root.path().join("elsewhere.cwasm");
    std::fs::rename(&artifact, &elsewhere).expect("the artifact moved");
    std::os::unix::fs::symlink(&elsewhere, &artifact).expect("a link in its place");
    let refused = scene.refusal().await;
    assert_eq!(reason_of(&refused), "untrusted");
    assert!(
        refused.message().contains("symbolic link")
            && refused.message().contains(&artifact.display().to_string()),
        "the refusal names the link it will not follow: {:?}",
        refused.message()
    );
}

#[tokio::test]
async fn should_refuse_a_store_that_is_a_symbolic_link() {
    let scene = Scene::with(EMPTY_COMPONENT);
    scene.compile();
    let real = scene.root.path().join("real-store");
    std::fs::rename(scene.store(), &real).expect("the store moved");
    std::os::unix::fs::symlink(&real, scene.store()).expect("a link in its place");
    let refused = scene.refusal().await;
    assert_eq!(reason_of(&refused), "untrusted");
    assert!(
        refused.message().contains("symbolic link")
            && refused
                .message()
                .contains(&scene.store().display().to_string()),
        "the refusal names the store it will not follow: {:?}",
        refused.message()
    );
}

#[tokio::test]
async fn should_refuse_an_artifact_below_a_directory_others_can_write() {
    // Whoever may write a directory above the store may rename the store away and put their
    // own in its place, owner, mode and all. Every directory up to the root is examined, and
    // only a root-owned sticky one (`/tmp`, above every scene of this suite) is exempt.
    use std::os::unix::fs::PermissionsExt as _;
    let scene = Scene::with(EMPTY_COMPONENT);
    let open = scene.root.path().join("open");
    let store = open.join("store");
    let artifact = scene.compile_into(&store);
    for mode in [0o777, 0o1777, 0o775] {
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(mode))
            .expect("a directory above the store others can write");
        let refused = match TestHost::new(scene.component(), &manifest())
            .compiled(&store)
            .load()
            .await
        {
            Ok(_) => panic!("mode {mode:o}: an artifact below a writable directory was mapped"),
            Err(error) => error,
        };
        if mode == 0o775 && group_is_private(&open) {
            // A group only its owner belongs to is the owner (ADR-0915); the unit tests of the
            // rule cover a shared group, which this machine may not offer the test.
            assert_ne!(refused.code(), KuangErrorCode::LoadComponentNotCompiled);
            continue;
        }
        assert_eq!(
            refused.code(),
            KuangErrorCode::LoadComponentNotCompiled,
            "mode {mode:o}: {refused:?}"
        );
        assert_eq!(reason_of(&refused), "untrusted", "mode {mode:o}");
        assert!(
            refused.message().contains(&format!("`{}`", open.display()))
                && refused
                    .message()
                    .contains("writable by users other than its owner"),
            "mode {mode:o}: the refusal names the directory: {:?}",
            refused.message()
        );
    }
    assert!(artifact.is_file());
}

#[test]
fn should_refuse_to_write_below_a_directory_others_can_write() {
    use std::os::unix::fs::PermissionsExt as _;
    let scene = Scene::with(EMPTY_COMPONENT);
    let open = scene.root.path().join("open");
    std::fs::create_dir(&open).expect("a directory");
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777))
        .expect("writable by everybody");
    let run = std::process::Command::new(COMPILE)
        .arg("--store")
        .arg(open.join("store"))
        .arg(scene.component())
        .output()
        .expect("kuang-compile runs");
    assert!(!run.status.success(), "nothing is written below it");
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(
        said.contains(&format!("`{}`", open.display()))
            && said.contains("writable by users other than its owner"),
        "{said:?}"
    );
    assert!(!open.join("store").exists(), "not even the store");
}

#[test]
fn should_refuse_to_write_through_a_symbolic_link() {
    // A store planted as a link would send a root-run install's artifact wherever the link
    // points; the tool follows none on its way to the store.
    let scene = Scene::with(EMPTY_COMPONENT);
    let target = scene.root.path().join("target");
    std::fs::create_dir(&target).expect("a directory");
    std::os::unix::fs::symlink(&target, scene.store()).expect("a store that is a link");
    let run = std::process::Command::new(COMPILE)
        .arg("--store")
        .arg(scene.store())
        .arg(scene.component())
        .output()
        .expect("kuang-compile runs");
    assert!(!run.status.success(), "nothing is written through the link");
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(said.contains("symbolic link"), "{said:?}");
    assert_eq!(
        std::fs::read_dir(&target).expect("the target").count(),
        0,
        "and nothing lands where it points"
    );
}

#[tokio::test]
async fn should_refuse_a_store_it_cannot_read_as_unreadable_not_as_another_engine() {
    // Permission denied is not the engine refusing an artifact, and `kuang-compile` would not
    // fix it: the refusal says what happened.
    use std::os::unix::fs::PermissionsExt as _;
    if running_as_root() {
        ono_testkit::skipped(
            ono_testkit::SkipReason::MissingPrivilege,
            "root reads a store whatever its mode, so permission is never denied",
        );
        return;
    }
    let scene = Scene::with(EMPTY_COMPONENT);
    scene.compile();
    std::fs::set_permissions(scene.store(), std::fs::Permissions::from_mode(0o000))
        .expect("a store nobody may search");
    let refused = scene.refusal().await;
    std::fs::set_permissions(scene.store(), std::fs::Permissions::from_mode(0o755))
        .expect("the store back");
    assert_eq!(reason_of(&refused), "unreadable", "{:?}", refused.message());
    assert!(
        refused.message().contains("ermission denied") && !refused.message().contains("engine"),
        "the refusal says the store could not be read, not that the engine refused it: {:?}",
        refused.message()
    );
}

#[test]
fn should_write_an_artifact_only_its_owner_can_change_whatever_the_umask() {
    use std::os::unix::fs::PermissionsExt as _;
    let scene = Scene::with(EMPTY_COMPONENT);
    let artifact = scene.compile();
    for path in [artifact.as_path(), scene.store().as_path()] {
        let mode = std::fs::metadata(path)
            .expect("written")
            .permissions()
            .mode();
        assert_eq!(mode & 0o022, 0, "{} is {mode:o}", path.display());
    }
}

#[test]
fn should_refuse_to_write_into_a_store_the_shell_would_not_read() {
    use std::os::unix::fs::PermissionsExt as _;
    let scene = Scene::with(EMPTY_COMPONENT);
    std::fs::create_dir(scene.store()).expect("a store");
    std::fs::set_permissions(scene.store(), std::fs::Permissions::from_mode(0o775))
        .expect("a group-writable store");
    let run = std::process::Command::new(COMPILE)
        .arg("--store")
        .arg(scene.store())
        .arg(scene.component())
        .output()
        .expect("kuang-compile runs");
    assert!(
        !run.status.success(),
        "an artifact the shell would refuse to map is not written"
    );
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(
        said.contains("writable by users other than its owner"),
        "{said:?}"
    );
    assert_eq!(
        std::fs::read_dir(scene.store()).expect("the store").count(),
        0,
        "and nothing is left in the store"
    );
}

#[test]
fn should_refuse_to_compile_what_is_not_a_component() {
    let scene = Scene::with(b"not webassembly");
    let run = std::process::Command::new(COMPILE)
        .arg("--store")
        .arg(scene.store())
        .arg(scene.component())
        .output()
        .expect("kuang-compile runs");
    assert!(
        !run.status.success(),
        "a file that is not a component is refused"
    );
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(
        said.contains(&scene.component().display().to_string()),
        "the refusal names the file: {said:?}"
    );
    assert!(
        !scene.store().exists() || std::fs::read_dir(scene.store()).expect("a store").count() == 0,
        "and nothing is left in the store"
    );
}

#[test]
fn should_answer_a_call_without_a_component_with_its_usage() {
    let run = std::process::Command::new(COMPILE)
        .output()
        .expect("kuang-compile runs");
    assert_eq!(run.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&run.stderr).contains("--store"));
}
