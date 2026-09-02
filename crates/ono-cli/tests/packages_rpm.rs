//! Outcome tests for the package family on rpm-based systems: Red Hat's `dnf`/`yum` and SUSE's
//! `zypper`, over the one package database both families share.
//!
//! Contract: `docs/spec/commands/package.yaml`, `docs/spec/providers/linux-packages.yaml`,
//! schema `ono.package/1`. Narrative: spec §9.1 (the Package table), §16.5 (partial failure),
//! §17.2 (elevation is explicit and visible), §31.58 and AGENTS.md §6 (a provider asks a tool
//! for an explicit machine-readable mode and never parses its human listing), §35.3 (unknown is
//! null, never fabricated), §43 (error taxonomy). ADR-0422.
//!
//! Every test runs unprivileged, offline and deterministically: no package manager on this
//! machine is ever touched. A scratch `bin/` holding executable fakes is the whole `PATH`, and
//! what those fakes answer is exactly what rpm, dnf and zypper document as their machine modes:
//!
//! - `rpm -qa --queryformat '%{NAME}\t%{VERSION}-%{RELEASE}\n'` for the installed set, and
//!   `rpm -q --queryformat … <name>` for one package — non-zero meaning "not installed", which
//!   is rpm's documented answer to that question and an ordinary empty result here;
//! - `dnf repoquery --queryformat '%{name}\t%{summary}\n' '*term*'` for the repositories;
//! - `zypper --xmlout … search --type package <term>` for the repositories on SUSE, where the
//!   XML `solvable` elements are the answer and a `srcpackage` is not a package;
//! - `dnf install -y` / `zypper --non-interactive install` for the changes, refused before they
//!   run when the shell is not root (spec §17.2).
//!
//! A fake that prints something else is a provider defect (E0403), never a source of invented
//! records. Every test asserts what the user sees — stdout through `| to json`, the exit status,
//! the structured error code — never how a stage is wired (AGENTS.md §11).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::PathBuf;

use ono_testkit::{Scratch, scratch};
use serde_yaml_ng::Value;

mod support;
use support::{assert_failed_row, executable, ono_with_path, text};

// --- helpers ------------------------------------------------------------------------------------

/// Parses the JSON document `to json` wrote as the stream's values.
fn rows(run: &ono_testkit::Run) -> Vec<Value> {
    let text = run.stdout().trim().to_owned();
    let stderr = run.stderr();
    let document: Value = serde_yaml_ng::from_str(&text).unwrap_or_else(|error| {
        panic!("`to json` must emit a JSON document, got {text:?} ({error}); stderr: {stderr:?}")
    });
    document
        .as_sequence()
        .unwrap_or_else(|| panic!("spec §33.5: `to json` emits the stream as an array, got {text:?}; stderr: {stderr:?}"))
        .clone()
}

/// The names of the records a listing or a search produced, sorted.
fn names(run: &ono_testkit::Run) -> Vec<String> {
    let mut found: Vec<String> = rows(run).iter().map(|row| text(row, "name")).collect();
    found.sort();
    found
}

/// The one `ono.action-result/1` row a single-target mutation emits.
fn single_result(run: &ono_testkit::Run) -> Value {
    let mut rows = rows(run);
    assert_eq!(
        rows.len(),
        1,
        "spec §11.5: one ActionResult per target, got {:?}",
        run.stdout()
    );
    rows.remove(0)
}

/// The stderr of a run that must have been refused at the provider boundary.
fn provider_boundary_error(run: &ono_testkit::Run, code: &str) -> String {
    let stderr = run.stderr().to_owned();
    assert!(
        !run.status().is_success(),
        "a provider error is an error: the exit status is non-zero, got {:?}; stderr: {stderr:?}",
        run.status()
    );
    assert!(
        stderr.contains(code),
        "spec §43: the refusal carries {code}, got {stderr:?}"
    );
    stderr
}

// --- the fake managers --------------------------------------------------------------------------

/// What the fake `rpm` prints for the installed set.
#[derive(Clone, Copy)]
enum Listing {
    /// Two installed packages in the `NAME\tVERSION-RELEASE` query format.
    TwoPackages,
    /// One package rpm lists once per architecture: two lines, one package.
    TwoArchesOfOne,
    /// Bytes that are not a listing in any format.
    Garbage,
}

/// Which repository front end sits beside `rpm` on the `PATH`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Frontend {
    /// Red Hat's, as Fedora, RHEL and their rebuilds ship it.
    Dnf,
    /// Red Hat's older name for it, as RHEL still installs it.
    Yum,
    /// SUSE's.
    Zypper,
    /// Both families' front ends on one machine.
    DnfAndZypper,
    /// The database and nothing that can reach a repository.
    None,
}

/// Writes fake rpm-family executables into `<scratch>/bin` and returns that directory.
///
/// Every fake prints with `printf`, which is a shell builtin: the fake `PATH` holds these fakes
/// and nothing else, so a fake that shelled out would be testing whether coreutils is reachable.
fn fake_managers(directory: &Scratch, listing: Listing, frontend: Frontend) -> PathBuf {
    let bin = directory.path().join("bin");
    std::fs::create_dir_all(&bin).expect("create the fake PATH");

    let listed = match listing {
        Listing::TwoPackages => {
            "printf 'curl\\t8.6.0-8.fc40\\n'\nprintf 'nginx\\t1.24.0-6.fc40\\n'\n"
        }
        Listing::TwoArchesOfOne => {
            "printf 'glibc\\t2.39-5.fc40\\n'\nprintf 'glibc\\t2.39-5.fc40\\n'\n"
        }
        Listing::Garbage => "printf '\\377\\376 not a listing at all ~~~ %s\\n' \"$*\"\n",
    };
    // `rpm -qa` lists everything; `rpm -q <name>` answers for the one name it was given, and
    // exits non-zero for a package the database does not have.
    executable(
        &bin.join("rpm"),
        &format!(
            concat!(
                "#!/bin/sh\n",
                "case \"$1\" in\n",
                "  --version) echo 'RPM version 4.19.1.1'; exit 0;;\n",
                "esac\n",
                "case \"$*\" in\n",
                "  *-qa*) {}exit 0;;\n",
                "esac\n",
                "eval name=\"\\${{$#}}\"\n",
                "case \"$name\" in\n",
                "  curl) printf 'curl\\t8.6.0-8.fc40\\n'; exit 0;;\n",
                "  nginx) printf 'nginx\\t1.24.0-6.fc40\\n'; exit 0;;\n",
                "esac\n",
                "echo \"package $name is not installed\"\n",
                "exit 1\n",
            ),
            listed
        ),
    );

    // `dnf repoquery --queryformat` answers `name<TAB>summary` per line; anything this fake is
    // asked to change fails the way an unprivileged dnf does.
    let dnf = concat!(
        "#!/bin/sh\n",
        "case \"$1\" in\n",
        "  --version) echo '4.19.0'; exit 0;;\n",
        "esac\n",
        "case \"$*\" in\n",
        "  *repoquery*)\n",
        "  printf 'curl\\tA utility for getting files from servers\\n'\n",
        "  printf 'curl-minimal\\tA conservative build of curl\\n'\n",
        "  exit 0;;\n",
        "esac\n",
        "echo 'Error: This command has to be run with superuser privileges' >&2\n",
        "exit 1\n",
    );
    // `zypper --xmlout search` answers a solvable list. A `srcpackage` is a source package and
    // not a package, and a search result carries no summary at all.
    let zypper = concat!(
        "#!/bin/sh\n",
        "case \"$*\" in\n",
        "  *search*)\n",
        "  printf '<?xml version=\"1.0\"?>\\n'\n",
        "  printf '<stream><search-result version=\"0.0\"><solvable-list>\\n'\n",
        "  printf '<solvable status=\"not-installed\" name=\"curl\" kind=\"package\"/>\\n'\n",
        "  printf '<solvable status=\"installed\" name=\"curl-doc\" kind=\"package\"/>\\n'\n",
        "  printf '<solvable name=\"curl-source\" kind=\"srcpackage\"/>\\n'\n",
        "  printf '</solvable-list></search-result></stream>\\n'\n",
        "  exit 0;;\n",
        "esac\n",
        "echo 'Root privileges are required for installing packages.' >&2\n",
        "exit 4\n",
    );

    match frontend {
        Frontend::Dnf => executable(&bin.join("dnf"), dnf),
        Frontend::Yum => executable(&bin.join("yum"), dnf),
        Frontend::Zypper => executable(&bin.join("zypper"), zypper),
        Frontend::DnfAndZypper => {
            executable(&bin.join("dnf"), dnf);
            executable(&bin.join("zypper"), zypper);
        }
        Frontend::None => {}
    }
    bin
}

// --- enumeration --------------------------------------------------------------------------------

#[test]
fn should_list_installed_packages_when_rpm_answers_in_its_query_format() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages, Frontend::Dnf);

    let run = ono_with_path(&bin, "get package | select name version | to json");
    run.assert_success();
    let mut listed: Vec<(String, String)> = rows(&run)
        .iter()
        .map(|row| (text(row, "name"), text(row, "version")))
        .collect();
    listed.sort();
    assert_eq!(
        listed,
        [
            ("curl".to_owned(), "8.6.0-8.fc40".to_owned()),
            ("nginx".to_owned(), "1.24.0-6.fc40".to_owned()),
        ],
        "both packages rpm listed are records, got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_name_rpm_as_the_provider_of_a_package_it_lists() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages, Frontend::Dnf);

    // ADR-0115: the `provider` field is the database that answered, and on both Red Hat and
    // SUSE that database is rpm. Identity is `provider + name`.
    let run = ono_with_path(&bin, "get package curl | to json");
    run.assert_success();
    let row = single_result(&run);
    assert_eq!(text(&row, "name"), "curl");
    assert_eq!(
        text(&row, "provider"),
        "rpm",
        "the record names the database that answered, got {row:?}"
    );
    assert_eq!(
        row["installed"].as_bool(),
        Some(true),
        "everything `rpm -q` answers for is installed, got {row:?}"
    );
}

#[test]
fn should_answer_with_nothing_when_rpm_does_not_have_the_named_package() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages, Frontend::Dnf);

    // `rpm -q` exits non-zero for a package that is not installed. That is its documented
    // answer to the question, not a failure: the stream is empty and the status is success.
    let run = ono_with_path(&bin, "get package absent | to json");
    run.assert_success();
    assert!(
        rows(&run).is_empty(),
        "a package rpm does not have is no record, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_answer_once_for_a_package_rpm_lists_once_per_architecture() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoArchesOfOne, Frontend::Dnf);

    // `ono.package/1` is identified by `provider + name`, so two architectures of one package
    // are one object: emitting both would put two objects with one identity into the pipeline,
    // and a mutation would then act on it twice.
    let run = ono_with_path(&bin, "get package | to json");
    run.assert_success();
    assert_eq!(
        names(&run),
        ["glibc"],
        "one package, whatever the architecture count, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_report_a_schema_violation_when_rpm_prints_garbage() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::Garbage, Frontend::Dnf);

    // AGENTS.md §6: a listing outside the declared machine format is a provider defect (E0403),
    // never a source of records named after whatever the bytes happened to say.
    let run = ono_with_path(&bin, "get package | to json");
    provider_boundary_error(&run, "Ono-Sendai-E0403");
    assert!(
        !run.stdout().contains("not a listing"),
        "nothing is fabricated from bytes outside the format, got {:?}",
        run.stdout()
    );
}

// --- search -------------------------------------------------------------------------------------

#[test]
fn should_search_the_repositories_with_dnf_when_finding_a_package() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages, Frontend::Dnf);

    let run = ono_with_path(&bin, "find package curl | to json");
    run.assert_success();
    assert_eq!(
        names(&run),
        ["curl", "curl-minimal"],
        "the hits repoquery answered are the records, got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    let row = rows(&run).remove(0);
    assert_eq!(
        text(&row, "description"),
        "A utility for getting files from servers",
        "the summary repoquery answered is the description, got {row:?}"
    );
}

#[test]
fn should_search_the_repositories_with_yum_when_that_is_the_front_end() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages, Frontend::Yum);

    let run = ono_with_path(&bin, "find package curl | to json");
    run.assert_success();
    assert_eq!(
        names(&run),
        ["curl", "curl-minimal"],
        "`yum` is Red Hat's other name for the same front end, got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_search_the_repositories_with_zypper_when_finding_a_package_on_suse() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages, Frontend::Zypper);

    let run = ono_with_path(&bin, "find package curl | to json");
    run.assert_success();
    assert_eq!(
        names(&run),
        ["curl", "curl-doc"],
        "the solvables zypper listed as packages are the records, and a `srcpackage` is not \
         one, got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    let row = rows(&run).remove(0);
    assert!(
        row["description"].is_null(),
        "spec §35.3: a zypper search result carries no summary, so the description is null \
         rather than invented, got {row:?}"
    );
}

#[test]
fn should_search_through_zypper_when_both_front_ends_are_on_the_path() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages, Frontend::DnfAndZypper);

    // A machine carrying zypper is a SUSE machine — Fedora and RHEL never ship it, while dnf
    // is installable anywhere — so zypper's presence is what decides (ADR-0422).
    let run = ono_with_path(&bin, "find package curl | to json");
    run.assert_success();
    assert_eq!(
        names(&run),
        ["curl", "curl-doc"],
        "zypper answered, got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_refuse_to_search_when_the_database_has_no_front_end() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages, Frontend::None);

    // rpm knows what is installed and nothing about repositories. The refusal names what would
    // answer, and the listing keeps working.
    let run = ono_with_path(&bin, "find package curl | to json");
    let stderr = provider_boundary_error(&run, "Ono-Sendai-E0402");
    for wanted in ["dnf", "zypper"] {
        assert!(
            stderr.contains(wanted),
            "the refusal names the front end it looked for ({wanted}), got {stderr:?}"
        );
    }

    let listing = ono_with_path(&bin, "get package | to json");
    listing.assert_success();
    assert_eq!(
        names(&listing),
        ["curl", "nginx"],
        "rpm alone still answers what is installed, got {:?}",
        listing.stdout()
    );
}

// --- mutations ----------------------------------------------------------------------------------

#[test]
fn should_fail_with_permission_denied_when_adding_a_package_unprivileged_on_an_rpm_system() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages, Frontend::Dnf);

    // Spec §17.2: the elevation is stated before anything runs, as one failed row (spec §16.5).
    let run = ono_with_path(&bin, "add package foo | to json");
    assert_failed_row(&single_result(&run), "ono.package.add", "Ono-Sendai-E0302");
    run.assert_status(1);
}

#[test]
fn should_fail_with_permission_denied_when_removing_a_package_unprivileged_on_suse() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages, Frontend::Zypper);

    let run = ono_with_path(&bin, "remove package curl | to json");
    assert_failed_row(
        &single_result(&run),
        "ono.package.remove",
        "Ono-Sendai-E0302",
    );
    run.assert_status(1);
}

#[test]
fn should_refuse_to_purge_a_package_on_an_rpm_system() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages, Frontend::Dnf);

    // rpm has no purge: a removal leaves a modified configuration file behind as `.rpmsave`.
    // Accepting the flag and doing an ordinary removal would be a lie about what happened.
    let run = ono_with_path(&bin, "remove package curl --purge true | to json");
    assert_failed_row(
        &single_result(&run),
        "ono.package.remove",
        "Ono-Sendai-E0402",
    );
    run.assert_status(1);
}

#[test]
fn should_refuse_to_change_a_package_when_the_database_has_no_front_end() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages, Frontend::None);

    // rpm can install a file; it cannot resolve a name against a repository. The refusal is the
    // provider's, before the privilege question, because no front end is a fact about this
    // machine and the uid is not what is missing.
    let run = ono_with_path(&bin, "add package foo | to json");
    assert_failed_row(&single_result(&run), "ono.package.add", "Ono-Sendai-E0402");
    run.assert_status(1);
}

#[test]
fn should_name_the_rpm_provider_and_the_privilege_when_explaining_an_install() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages, Frontend::Dnf);

    let run = ono_with_path(&bin, "explain add package foo");
    run.assert_success();
    let plan = run.stdout().to_owned();
    assert!(
        plan.contains("linux.packages.rpm"),
        "the plan names the provider that would carry the install out, got {plan:?}"
    );
    assert!(
        plan.contains("elevated"),
        "package.yaml: an install requires elevated privilege and the plan says so, got {plan:?}"
    );
}
