//! `get kernel`: the running kernel, the operating system it boots and the uptime, as one
//! `ono.kernel/1` record read from the kernel's own files (spec §8.1, ADR-0964, #207).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers shared between the cases below sit outside a `#[test]` function, where a \
              failed precondition should still abort loudly"
)]

mod common;

use std::fs;

use common::{
    FIXTURE_BOOT_ID, FIXTURE_BOOT_TIME, FIXTURE_UPTIME_SECONDS, ProcFixture, drain, records,
};
use ono_provider_api::{Availability, Provider, Query};
use ono_provider_linux::KernelProvider;
use ono_value::{FieldAccess, Value};

/// Gives the proc fixture the `sys/kernel` files a Linux host publishes, and an `os-release`.
fn kernel_files(fixture: &ProcFixture, release: &str, os_release: Option<&str>) {
    let sys = fixture.proc().join("sys/kernel");
    fs::create_dir_all(&sys).expect("the proc/sys/kernel directory");
    fs::write(sys.join("osrelease"), format!("{release}\n")).expect("osrelease");
    fs::write(
        sys.join("version"),
        "#1 SMP PREEMPT_DYNAMIC Tue Oct  7 09:00:00 UTC 2026\n",
    )
    .expect("version");
    fs::write(sys.join("hostname"), "fixture-host\n").expect("hostname");
    if let Some(text) = os_release {
        let etc = fixture.root().join("etc");
        fs::create_dir_all(&etc).expect("the etc directory");
        fs::write(etc.join("os-release"), text).expect("os-release");
    }
}

#[tokio::test]
async fn should_report_the_kernel_the_system_and_the_uptime_from_the_kernels_own_files() {
    let fixture = ProcFixture::new();
    kernel_files(
        &fixture,
        "6.8.0-45-generic",
        Some("PRETTY_NAME=\"Fixture Linux 1.0 (Test)\"\nNAME=\"Fixture Linux\"\nID=fixture\n"),
    );

    let collected = drain(
        KernelProvider::rooted(fixture.root())
            .snapshot(&Query::target("kernel"))
            .expect("a snapshot"),
    )
    .await;
    let records = records(&collected);
    assert_eq!(records.len(), 1, "one host runs one kernel");
    let kernel = &records[0];

    assert_eq!(
        kernel.get("release"),
        Some(&Value::string("6.8.0-45-generic"))
    );
    assert_eq!(
        kernel.get("version"),
        Some(&Value::string(
            "#1 SMP PREEMPT_DYNAMIC Tue Oct  7 09:00:00 UTC 2026"
        ))
    );
    assert_eq!(kernel.get("hostname"), Some(&Value::string("fixture-host")));
    assert_eq!(
        kernel.get("os"),
        Some(&Value::string("Fixture Linux 1.0 (Test)")),
        "PRETTY_NAME, unquoted"
    );
    assert_eq!(kernel.get("os_id"), Some(&Value::string("fixture")));
    assert_eq!(kernel.get("boot_id"), Some(&Value::string(FIXTURE_BOOT_ID)));
    assert_eq!(
        kernel
            .get("booted")
            .and_then(|value| value.as_timestamp().ok())
            .map(|timestamp| timestamp.as_second()),
        Some(FIXTURE_BOOT_TIME),
        "btime, as an instant"
    );
    assert_eq!(
        kernel
            .get("uptime")
            .and_then(|value| value.as_duration().ok())
            .map(|duration| duration.nanoseconds()),
        Some(i128::from(FIXTURE_UPTIME_SECONDS) * 1_000_000_000),
        "/proc/uptime, as a duration"
    );
    assert!(
        matches!(kernel.get("architecture"), Some(Value::String(_))),
        "the machine's architecture, from uname(2)"
    );
    assert_eq!(kernel.provenance().provider(), "linux.kernel");
}

#[tokio::test]
async fn should_leave_the_system_null_where_no_os_release_is_readable() {
    // §35.3: unknown is null, never invented. A minimal container has a kernel and no
    // os-release.
    let fixture = ProcFixture::new();
    kernel_files(&fixture, "6.8.0-45-generic", None);

    let collected = drain(
        KernelProvider::rooted(fixture.root())
            .snapshot(&Query::target("kernel"))
            .expect("a snapshot"),
    )
    .await;
    let kernel = &records(&collected)[0];
    assert_eq!(kernel.access("os"), FieldAccess::Unknown);
    assert_eq!(kernel.access("os_id"), FieldAccess::Unknown);
    assert_eq!(
        kernel.get("release"),
        Some(&Value::string("6.8.0-45-generic"))
    );
}

#[tokio::test]
async fn should_identify_the_record_by_the_release_where_proc_publishes_no_boot_id() {
    // A restricted /proc withholds the boot id; the record still has an identity (ADR-0553's
    // fallback), and it says the boot id is unknown rather than making one up.
    let fixture = ProcFixture::without_boot_id();
    kernel_files(&fixture, "6.1.0-26-amd64", None);

    let collected = drain(
        KernelProvider::rooted(fixture.root())
            .snapshot(&Query::target("kernel"))
            .expect("a snapshot"),
    )
    .await;
    let kernel = &records(&collected)[0];
    assert_eq!(kernel.access("boot_id"), FieldAccess::Unknown);
    let identity = kernel.identity().to_string();
    assert!(
        identity.contains("6.1.0-26-amd64"),
        "the release names the kernel when the boot id cannot, got {identity}"
    );
}

#[test]
fn should_be_unavailable_where_proc_has_no_kernel_release() {
    let fixture = ProcFixture::new();
    assert!(
        matches!(
            KernelProvider::rooted(fixture.root()).availability(),
            Availability::Unavailable { .. }
        ),
        "no osrelease, no kernel to describe"
    );
}

#[tokio::test]
async fn should_find_the_running_kernel_on_the_real_system() {
    let collected = drain(
        KernelProvider::new()
            .snapshot(&Query::target("kernel"))
            .expect("a snapshot"),
    )
    .await;
    let records = records(&collected);
    assert_eq!(records.len(), 1);
    let expected = fs::read_to_string("/proc/sys/kernel/osrelease").expect("this is Linux");
    assert_eq!(
        records[0].get("release"),
        Some(&Value::string(expected.trim()))
    );
}
