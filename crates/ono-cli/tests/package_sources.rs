//! The repositories a package manager reads its index from, and the refresh that acts on one
//! (issue #17, ADR-0562, ADR-0565). Contract: `docs/contracts/commands/package.yaml`,
//! `docs/contracts/schemas/package-source.v1.yaml`, `docs/contracts/providers/linux-packages.yaml`.
//!
//! Every test runs the real `ono` binary against fake managers on an otherwise empty `PATH`:
//! the assumption pinned here is that the shell reads apt's sources through
//! `apt-get indextargets --format` and zypper's through `zypper --xmlout lr` — machine formats
//! (spec §31.58, ADR-0115) — and that what a refresh changed is read from the index file, not
//! from the manager's prose. A refresh needs root (spec §17.2), so what an unprivileged test can
//! prove is the dry run, the refusal, and `explain`.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::PathBuf;

use ono_testkit::{Scratch, scratch};

mod support;
use support::{executable, ono_with_path, rows, text};

/// Fake apt on `<scratch>/bin`: dpkg-query with one package; apt-get whose `update
/// --print-uris` names two sources' index files, whose `indextargets` labels the one that has
/// been fetched, and which refuses everything else as it would for a non-root; and apt-config
/// pointing `Dir::State::lists` at `<scratch>/lists`.
fn fake_apt(directory: &Scratch) -> PathBuf {
    let bin = directory.path().join("bin");
    std::fs::create_dir_all(&bin).expect("create the fake PATH");
    let lists = directory.path().join("lists");
    std::fs::create_dir_all(&lists).expect("create the index directory");
    let ubuntu_index =
        lists.join("archive.ubuntu.com_ubuntu_dists_noble_main_binary-amd64_Packages");
    std::fs::write(&ubuntu_index, "Package: curl\n").expect("write an index");
    // The docker source has no index yet: it was added and never refreshed.

    executable(
        &bin.join("dpkg-query"),
        "#!/bin/sh\nprintf 'curl\\t8.5.0-2\\tinstall ok installed\\n'\nexit 0\n",
    );
    let apt_get = format!(
        concat!(
            "#!/bin/sh\n",
            "case \"$1 $2\" in\n",
            "  '--version '*) echo 'apt 2.7.14 (amd64)'; exit 0;;\n",
            "  'update --print-uris')\n",
            "    printf \"'http://archive.ubuntu.com/ubuntu/dists/noble/InRelease' archive.ubuntu.com_ubuntu_dists_noble_InRelease 0 \\n\"\n",
            "    printf \"'http://archive.ubuntu.com/ubuntu/dists/noble/main/binary-amd64/Packages.xz' archive.ubuntu.com_ubuntu_dists_noble_main_binary-amd64_Packages 0 \\n\"\n",
            "    printf \"'http://archive.ubuntu.com/ubuntu/dists/noble/main/i18n/Translation-en.xz' archive.ubuntu.com_ubuntu_dists_noble_main_i18n_Translation-en 0 \\n\"\n",
            "    printf \"'https://download.docker.com/linux/ubuntu/dists/noble/stable/binary-amd64/Packages.gz' download.docker.com_linux_ubuntu_dists_noble_stable_binary-amd64_Packages 0 \\n\"\n",
            "    exit 0;;\n",
            "  'indextargets '*)\n",
            "    printf 'http://archive.ubuntu.com/ubuntu/\\tnoble\\tmain\\tUbuntu\\tUbuntu\\t{ubuntu}\\n'\n",
            "    exit 0;;\n",
            "esac\n",
            "echo 'E: Could not open lock file /var/lib/apt/lists/lock - open (13: Permission denied)' >&2\n",
            "exit 100\n",
        ),
        ubuntu = ubuntu_index.display(),
    );
    executable(&bin.join("apt-get"), &apt_get);
    executable(
        &bin.join("apt-config"),
        &format!(
            "#!/bin/sh\nprintf \"LISTS='{}/'\\n\"\nexit 0\n",
            lists.display()
        ),
    );
    bin
}

/// Fake rpm and zypper on `<scratch>/bin`: zypper lists two repositories in its XML.
fn fake_zypper(directory: &Scratch) -> PathBuf {
    let bin = directory.path().join("bin");
    std::fs::create_dir_all(&bin).expect("create the fake PATH");
    executable(
        &bin.join("rpm"),
        "#!/bin/sh\nprintf 'curl\\t8.6.0-8.fc40\\n'\nexit 0\n",
    );
    executable(
        &bin.join("zypper"),
        concat!(
            "#!/bin/sh\n",
            "case \"$*\" in\n",
            "  *lr*)\n",
            "  printf '<?xml version=\"1.0\"?>\\n<stream>\\n<repo-list>\\n'\n",
            "  printf '<repo alias=\"repo-oss\" name=\"Main Repository\" type=\"rpm-md\" enabled=\"1\" autorefresh=\"1\"><url>http://download.opensuse.org/tumbleweed/repo/oss/</url></repo>\\n'\n",
            "  printf '<repo alias=\"repo-debug\" name=\"Debug Repository\" type=\"NONE\" enabled=\"0\" autorefresh=\"0\"><url>http://download.opensuse.org/debug/tumbleweed/repo/oss/</url></repo>\\n'\n",
            "  printf '</repo-list>\\n</stream>\\n'\n",
            "  exit 0;;\n",
            "esac\n",
            "echo 'Root privileges are required for refreshing services.' >&2\n",
            "exit 5\n",
        ),
    );
    bin
}

#[test]
fn should_list_apt_sources_one_per_repository_suite_and_component() {
    let directory = scratch();
    let bin = fake_apt(&directory);

    let run = ono_with_path(&bin, "get package-source | to json");
    run.assert_success();
    let listed = rows(&run);
    assert_eq!(
        listed.len(),
        2,
        "three index files, two sources; got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    let ubuntu = &listed[0];
    assert_eq!(text(ubuntu, "id"), "archive.ubuntu.com/ubuntu/noble/main");
    assert_eq!(text(ubuntu, "name"), "Ubuntu");
    assert_eq!(text(ubuntu, "url"), "http://archive.ubuntu.com/ubuntu/");
    assert_eq!(
        text(ubuntu, "provider"),
        "dpkg",
        "ADR-0115: the database that answered"
    );
    assert_eq!(ubuntu["enabled"].as_bool(), Some(true));
    assert!(
        ubuntu["refreshed"].is_string(),
        "the index file exists, so its time is known; got {ubuntu:?}"
    );
    let docker = &listed[1];
    assert_eq!(
        text(docker, "id"),
        "download.docker.com/linux/ubuntu/noble/stable"
    );
    assert!(
        docker["name"].is_null(),
        "no index fetched, so `indextargets` has no label for it; got {docker:?}"
    );
    assert!(
        docker["refreshed"].is_null(),
        "no index has been fetched for it, and null says so rather than a guess; got {docker:?}"
    );
}

#[test]
fn should_resolve_one_apt_source_by_its_id() {
    let directory = scratch();
    let bin = fake_apt(&directory);

    let run = ono_with_path(
        &bin,
        "get package-source download.docker.com/linux/ubuntu/noble/stable | select id | to json",
    );
    run.assert_success();
    let listed = rows(&run);
    assert_eq!(listed.len(), 1, "got {:?}", run.stdout());
    assert_eq!(
        text(&listed[0], "id"),
        "download.docker.com/linux/ubuntu/noble/stable"
    );
}

#[test]
fn should_say_what_a_refresh_would_run_when_asked_for_a_dry_run() {
    let directory = scratch();
    let bin = fake_apt(&directory);

    let run = ono_with_path(
        &bin,
        "refresh package-source archive.ubuntu.com/ubuntu/noble/main --dry-run | to json",
    );
    run.assert_success();
    let results = rows(&run);
    assert_eq!(
        results.len(),
        1,
        "one result for one source; got {:?}",
        run.stdout()
    );
    assert_eq!(text(&results[0], "status"), "skipped");
    assert_eq!(results[0]["changed"].as_bool(), Some(false));
    assert!(
        text(&results[0], "message").contains("apt-get update"),
        "the dry run names the command it would run; got {:?}",
        results[0]
    );
}

#[test]
fn should_refuse_a_refresh_with_the_privilege_it_needs_when_not_root() {
    let directory = scratch();
    let bin = fake_apt(&directory);

    // Spec §16.5, §17.2, ADR-0006: one failed row with E0302, and the exit status carries it.
    let run = ono_with_path(
        &bin,
        "refresh package-source archive.ubuntu.com/ubuntu/noble/main | to json",
    );
    assert_eq!(
        run.status().code(),
        1,
        "stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    let results = rows(&run);
    assert_eq!(results.len(), 1, "got {:?}", run.stdout());
    assert_eq!(text(&results[0], "status"), "failed");
    assert!(
        run.stdout().contains("Ono-Sendai-E0302"),
        "the refusal is the structured permission error; got {:?}",
        run.stdout()
    );
    assert!(
        text(&results[0], "message").contains("root"),
        "the refusal says what it needs; got {:?}",
        results[0]
    );
}

#[test]
fn should_name_the_provider_and_the_privilege_when_explaining_a_refresh() {
    let directory = scratch();
    let bin = fake_apt(&directory);

    let run = ono_with_path(
        &bin,
        "explain refresh package-source archive.ubuntu.com/ubuntu/noble/main",
    );
    run.assert_success();
    let shown = run.stdout();
    assert!(shown.contains("elevated"), "got {shown:?}");
    assert!(shown.contains("linux.packages"), "got {shown:?}");
    assert!(shown.contains("mutate"), "got {shown:?}");
}

#[test]
fn should_list_zypper_repositories_with_their_alias_and_enabled_flag() {
    let directory = scratch();
    let bin = fake_zypper(&directory);

    let run = ono_with_path(&bin, "get package-source | to json");
    run.assert_success();
    let listed = rows(&run);
    assert_eq!(
        listed.len(),
        2,
        "got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    assert_eq!(text(&listed[0], "id"), "repo-oss");
    assert_eq!(text(&listed[0], "name"), "Main Repository");
    assert_eq!(
        text(&listed[0], "url"),
        "http://download.opensuse.org/tumbleweed/repo/oss/"
    );
    assert_eq!(text(&listed[0], "provider"), "rpm");
    assert_eq!(listed[0]["enabled"].as_bool(), Some(true));
    assert_eq!(listed[1]["enabled"].as_bool(), Some(false));
}

#[test]
fn should_say_what_zypper_would_run_when_asked_for_a_dry_run() {
    let directory = scratch();
    let bin = fake_zypper(&directory);

    let run = ono_with_path(&bin, "refresh package-source repo-oss --dry-run | to json");
    run.assert_success();
    let results = rows(&run);
    assert_eq!(results.len(), 1, "got {:?}", run.stdout());
    assert_eq!(text(&results[0], "status"), "skipped");
    assert!(
        text(&results[0], "message").contains("zypper refresh repo-oss"),
        "got {:?}",
        results[0]
    );
}

#[test]
fn should_refuse_to_list_sources_when_no_package_manager_is_on_the_path() {
    let directory = scratch();
    let empty = directory.path().join("empty-bin");
    std::fs::create_dir_all(&empty).expect("create an empty PATH");

    // Having no manager is not the same as having no sources (spec §35.3).
    let run = ono_with_path(&empty, "get package-source | to json");
    assert_ne!(run.status().code(), 0, "stdout {:?}", run.stdout());
    assert!(
        run.stderr().contains("Ono-Sendai-E0401"),
        "the refusal is structured; got {:?}",
        run.stderr()
    );
}
