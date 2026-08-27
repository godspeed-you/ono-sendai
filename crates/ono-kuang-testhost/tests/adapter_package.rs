//! The test host validates a declarative adapter package before it is loaded (spec v0.3 §1.45,
//! §2.3, ADR-0065): manifest, packs, fixtures, the executables it may run, and what the
//! default-deny policy does to it.

#![allow(
    clippy::expect_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::PathBuf;

fn example() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ono-kuang-sdk/examples/adapter-package/dev.example.users")
        .canonicalize()
        .expect("the SDK ships the example adapter package")
}

#[test]
fn should_accept_the_sdks_example_adapter_package() {
    let report = ono_kuang_testhost::check_adapter_package(&example());
    assert!(report.problems.is_empty(), "got {:#?}", report.problems);
    assert_eq!(
        report.adapters,
        vec!["dev.example.users.getent-passwd".to_owned()]
    );
    assert!(
        !report.enabled_by_default,
        "default-deny: process.exec must be granted before the pack influences structured output"
    );
    assert!(report.enabled_when_granted);
}

#[test]
fn should_refuse_a_package_whose_adapter_names_an_undeclared_executable() {
    let scratch = ono_testkit::scratch();
    for entry in walkdir(&example()) {
        let relative = entry.strip_prefix(example()).expect("under the example");
        let text = std::fs::read(&entry).expect("readable");
        // The pack stays as shipped; the manifest's grant no longer covers what it runs.
        let text = if relative.to_string_lossy() == "manifest.yaml" {
            String::from_utf8_lossy(&text)
                .replace("executables: [getent]", "executables: [id]")
                .into_bytes()
        } else {
            text
        };
        let target = scratch.path().join(relative);
        std::fs::create_dir_all(target.parent().expect("a parent")).expect("dirs");
        std::fs::write(target, text).expect("written");
    }
    let report = ono_kuang_testhost::check_adapter_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("getent") && p.contains("executables")),
        "spec v0.3 §1.22: an adapter may only run what its package declares, got {:#?}",
        report.problems
    );
}

fn walkdir(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}
