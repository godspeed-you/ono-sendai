//! What `get mount` and `get filesystem` answer (spec §23.5, §28.6).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers shared between the cases below sit outside a `#[test]` function, where a \
              failed precondition should still abort loudly"
)]

mod common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{drain, find, records};
use ono_provider_api::{Provider, Query, Selector};
use ono_provider_linux::StorageProvider;
use ono_value::{FieldAccess, Value};

/// A fixture root holding a `proc/self/mountinfo` and a `dev/disk` symlink farm.
struct StorageFixture {
    root: tempfile::TempDir,
}

impl StorageFixture {
    fn new(lines: &str) -> Self {
        let root = tempfile::tempdir().expect("a temporary directory");
        fs::create_dir_all(root.path().join("proc/self")).expect("the proc tree");
        fs::write(root.path().join("proc/self/mountinfo"), lines).expect("the mount table");
        Self { root }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    /// Records a UUID for a device, the way udev's symlink farm does.
    fn uuid(&self, device: &str, uuid: &str) -> &Self {
        self.link("by-uuid", device, uuid)
    }

    /// Records a label for a device.
    fn label(&self, device: &str, label: &str) -> &Self {
        self.link("by-label", device, label)
    }

    fn link(&self, kind: &str, device: &str, name: &str) -> &Self {
        let directory = self.root.path().join("dev/disk").join(kind);
        fs::create_dir_all(&directory).expect("the disk directory");
        fs::create_dir_all(self.root.path().join("dev")).expect("the dev directory");
        let device_path = self.root.path().join("dev").join(device);
        fs::write(&device_path, "").expect("a stand-in for the device node");
        symlink(format!("../../{device}"), directory.join(name)).expect("the udev link");
        self
    }

    /// A directory inside the fixture, usable as a mount point that `statvfs` can answer for.
    fn mount_point(&self, name: &str) -> PathBuf {
        let path = self.root.path().join(name);
        fs::create_dir_all(&path).expect("the mount point");
        path
    }
}

fn provider(fixture: &StorageFixture) -> StorageProvider {
    StorageProvider::rooted(fixture.path())
}

#[tokio::test]
async fn should_report_every_declared_field_of_a_mount() {
    let fixture = StorageFixture::new("");
    let data = fixture.mount_point("data");
    let lines = format!(
        "36 35 8:1 / {} rw,noatime shared:1 - ext4 /dev/sdb1 rw,errors=remount-ro\n\
         37 35 0:44 / /proc rw,nosuid - proc proc rw\n",
        data.display()
    );
    fs::write(fixture.path().join("proc/self/mountinfo"), lines).expect("the mount table");

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("mount"))
            .expect("a snapshot"),
    )
    .await;
    let records = records(&collected);
    let mount = find(&records, "source", "/dev/sdb1").expect("the fixture declares /dev/sdb1");

    assert_eq!(mount.get("target"), Some(&Value::Path(Arc::from(data))));
    assert_eq!(mount.get("filesystem"), Some(&Value::string("ext4")));
    assert_eq!(
        mount.get("options"),
        Some(&Value::list([
            Value::string("rw"),
            Value::string("noatime"),
            Value::string("errors=remount-ro"),
        ])),
        "options stay one element per option, mount and superblock options merged in that order"
    );
    assert_eq!(mount.get("read_only"), Some(&Value::Bool(false)));
    assert_eq!(mount.get("device"), Some(&Value::string("8:1")));
    assert_eq!(mount.provenance().provider(), "linux.mountinfo");
    assert_eq!(
        mount.provenance().source(),
        Some("/proc/self/mountinfo"),
        "the mount table is the kernel's, not /etc/mtab"
    );

    let pseudo = find(&records, "source", "proc").expect("the fixture declares procfs");
    assert_eq!(
        pseudo.access("device"),
        FieldAccess::Unknown,
        "an anonymous device has no block device behind it, and 0:44 would suggest one"
    );
}

#[tokio::test]
async fn should_mark_a_read_only_mount_as_read_only() {
    let fixture = StorageFixture::new("");
    let target = fixture.mount_point("ro");
    fs::write(
        fixture.path().join("proc/self/mountinfo"),
        format!(
            "40 35 8:2 / {} ro,relatime - squashfs /dev/loop0 ro\n",
            target.display()
        ),
    )
    .expect("the mount table");

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("mount"))
            .expect("a snapshot"),
    )
    .await;
    assert_eq!(
        records(&collected)[0].get("read_only"),
        Some(&Value::Bool(true))
    );
}

#[tokio::test]
async fn should_decode_the_octal_escapes_a_mount_point_with_a_space_carries() {
    let fixture = StorageFixture::new("");
    let target = fixture.mount_point("my data");
    let escaped = target.display().to_string().replace(' ', "\\040");
    fs::write(
        fixture.path().join("proc/self/mountinfo"),
        format!("41 35 8:3 / {escaped} rw - ext4 /dev/sdc1 rw\n"),
    )
    .expect("the mount table");

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("mount"))
            .expect("a snapshot"),
    )
    .await;
    assert_eq!(
        records(&collected)[0].get("target"),
        Some(&Value::Path(Arc::from(target))),
        "a mount point with a space is a path with a space, not a path called `my\\\\040data`"
    );
}

#[tokio::test]
async fn should_report_capacity_uuid_and_label_for_a_mounted_filesystem() {
    let fixture = StorageFixture::new("");
    let target = fixture.mount_point("data");
    fixture.uuid("sdb1", "b0f1a2c3-1111-2222-3333-444455556666");
    fixture.label("sdb1", "backups");
    fs::write(
        fixture.path().join("proc/self/mountinfo"),
        format!(
            "36 35 8:1 / {} rw,noatime - ext4 /dev/sdb1 rw\n",
            target.display()
        ),
    )
    .expect("the mount table");

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("filesystem"))
            .expect("a snapshot"),
    )
    .await;
    let filesystem = &records(&collected)[0];

    assert_eq!(filesystem.get("source"), Some(&Value::string("/dev/sdb1")));
    assert_eq!(filesystem.get("type"), Some(&Value::string("ext4")));
    assert_eq!(
        filesystem.get("uuid"),
        Some(&Value::Uuid(
            ono_value::Uuid::parse("b0f1a2c3-1111-2222-3333-444455556666").expect("a uuid")
        ))
    );
    assert_eq!(filesystem.get("label"), Some(&Value::string("backups")));
    assert_eq!(filesystem.get("read_only"), Some(&Value::Bool(false)));

    let size = filesystem
        .get("size")
        .and_then(|value| value.as_byte_size().ok())
        .expect("a mounted filesystem reports its capacity");
    let used = filesystem
        .get("used")
        .and_then(|value| value.as_byte_size().ok())
        .expect("a mounted filesystem reports what is in use");
    let available = filesystem
        .get("available")
        .and_then(|value| value.as_byte_size().ok())
        .expect("a mounted filesystem reports what is available");
    assert!(size.bytes() > 0);
    assert!(used.bytes() <= size.bytes());
    assert!(
        available.bytes() <= size.bytes().saturating_sub(used.bytes()),
        "available is what this user may have, which reserved blocks are not part of"
    );
}

#[tokio::test]
async fn should_leave_the_uuid_null_when_no_udev_link_records_one() {
    let fixture = StorageFixture::new("");
    let target = fixture.mount_point("plain");
    fs::write(
        fixture.path().join("proc/self/mountinfo"),
        format!("36 35 0:50 / {} rw - tmpfs tmpfs rw\n", target.display()),
    )
    .expect("the mount table");

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("filesystem"))
            .expect("a snapshot"),
    )
    .await;
    let filesystem = &records(&collected)[0];
    assert_eq!(filesystem.access("uuid"), FieldAccess::Unknown);
    assert_eq!(filesystem.access("label"), FieldAccess::Unknown);
    assert_eq!(filesystem.access("device"), FieldAccess::Unknown);
}

#[tokio::test]
async fn should_report_a_mount_point_it_cannot_measure_as_an_error_rather_than_as_zero() {
    let fixture =
        StorageFixture::new("36 35 8:9 / /gone-between-read-and-measure rw - ext4 /dev/sdz9 rw\n");

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("filesystem"))
            .expect("a snapshot"),
    )
    .await;
    let filesystem = &records(&collected)[0];

    assert!(
        filesystem.access("size").is_failed(),
        "a capacity that could not be measured is an error, never a zero anyone would filter on"
    );
    assert!(filesystem.access("available").is_failed());
    assert_eq!(
        filesystem.get("source"),
        Some(&Value::string("/dev/sdz9")),
        "the fields that could be read are still reported (spec §16.5)"
    );
}

#[tokio::test]
async fn should_answer_once_per_filesystem_when_one_is_mounted_twice() {
    let fixture = StorageFixture::new("");
    let first = fixture.mount_point("first");
    let second = fixture.mount_point("second");
    fs::write(
        fixture.path().join("proc/self/mountinfo"),
        format!(
            "36 35 8:1 / {} rw - ext4 /dev/sdb1 rw\n42 35 8:1 /sub {} rw - ext4 /dev/sdb1 rw\n",
            first.display(),
            second.display()
        ),
    )
    .expect("the mount table");

    let mounts = drain(
        provider(&fixture)
            .snapshot(&Query::target("mount"))
            .expect("a snapshot"),
    )
    .await;
    assert_eq!(
        records(&mounts).len(),
        2,
        "a bind mount is two mounts, because a mount is a place"
    );

    let filesystems = drain(
        provider(&fixture)
            .snapshot(&Query::target("filesystem"))
            .expect("a snapshot"),
    )
    .await;
    assert_eq!(
        records(&filesystems).len(),
        1,
        "a bind mount is one filesystem, because a filesystem is a thing"
    );
}

#[tokio::test]
async fn should_answer_for_one_mount_point_when_a_selector_names_it() {
    let fixture = StorageFixture::new("");
    let wanted = fixture.mount_point("wanted");
    let other = fixture.mount_point("other");
    fs::write(
        fixture.path().join("proc/self/mountinfo"),
        format!(
            "36 35 8:1 / {} rw - ext4 /dev/sdb1 rw\n37 35 8:2 / {} rw - ext4 /dev/sdb2 rw\n",
            wanted.display(),
            other.display()
        ),
    )
    .expect("the mount table");

    let query = Query::target("mount").with(Selector::field(
        "target",
        Value::Path(Arc::from(wanted.clone())),
    ));
    let collected = drain(provider(&fixture).snapshot(&query).expect("a snapshot")).await;
    let records = records(&collected);

    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].get("target"),
        Some(&Value::Path(Arc::from(wanted)))
    );
}

#[tokio::test]
async fn should_report_the_provider_as_unavailable_when_there_is_no_mount_table() {
    let empty = tempfile::tempdir().expect("a temporary directory");
    let provider = StorageProvider::rooted(empty.path());
    assert!(!provider.availability().is_available());
}

#[tokio::test]
async fn should_find_the_root_mount_on_the_real_system() {
    let collected = drain(
        StorageProvider::new()
            .snapshot(&Query::target("mount"))
            .expect("this machine has a mount table"),
    )
    .await;
    let records = records(&collected);

    let root = records
        .iter()
        .find(|record| record.get("target") == Some(&Value::Path(Arc::from(Path::new("/")))))
        .expect("every running system has a root mount");
    assert!(matches!(root.get("filesystem"), Some(Value::String(_))));
    assert!(
        matches!(root.get("options"), Some(Value::List(options)) if !options.is_empty()),
        "a mount always has at least one option"
    );
}

#[test]
fn should_claim_the_mount_and_filesystem_targets_with_the_registry_capability_ids() {
    let provider = StorageProvider::new();
    assert_eq!(provider.targets(), ["mount", "filesystem"]);
    let ids: Vec<String> = provider
        .capabilities()
        .iter()
        .map(|capability| capability.id().to_owned())
        .collect();
    assert_eq!(ids, ["mount.list", "filesystem.list", "mount.manage"]);
}

// --- filesystems that are not mounted (storage.yaml `--mounted`, ADR-0097) -------------------

impl StorageFixture {
    /// Records what the kernel and udev know about a block device that carries a filesystem
    /// signature: its number under `sys/class/block`, and udev's probe result in its database.
    fn block_device(&self, device: &str, number: &str, fs_type: &str) -> &Self {
        let class = self.root.path().join("sys/class/block").join(device);
        fs::create_dir_all(&class).expect("the sysfs class entry");
        fs::write(class.join("dev"), format!("{number}\n")).expect("the device number");
        let udev = self.root.path().join("run/udev/data");
        fs::create_dir_all(&udev).expect("the udev database");
        fs::write(
            udev.join(format!("b{number}")),
            format!("S:disk/by-uuid/x\nE:ID_FS_TYPE={fs_type}\nE:ID_FS_USAGE=filesystem\n"),
        )
        .expect("the udev record");
        self
    }
}

#[tokio::test]
async fn should_list_an_unmounted_filesystem_with_a_null_target() {
    let fixture = StorageFixture::new("");
    let target = fixture.mount_point("data");
    fixture
        .uuid("sdb1", "b0f1a2c3-1111-2222-3333-444455556666")
        .block_device("sdb1", "8:17", "ext4")
        .uuid("sdc1", "c0f1a2c3-1111-2222-3333-444455556666")
        .label("sdc1", "spare")
        .block_device("sdc1", "8:33", "xfs");
    fs::write(
        fixture.path().join("proc/self/mountinfo"),
        format!(
            "36 35 8:17 / {} rw,noatime - ext4 /dev/sdb1 rw\n",
            target.display()
        ),
    )
    .expect("the mount table");

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("filesystem"))
            .expect("a snapshot"),
    )
    .await;
    let records = records(&collected);
    assert_eq!(
        records.len(),
        2,
        "one mounted and one unmounted filesystem, got {records:?}"
    );
    let spare =
        find(&records, "source", "/dev/sdc1").expect("the unmounted device is a filesystem too");
    assert_eq!(spare.get("type"), Some(&Value::string("xfs")));
    assert_eq!(spare.get("label"), Some(&Value::string("spare")));
    assert_eq!(
        spare.get("uuid"),
        Some(&Value::Uuid(
            ono_value::Uuid::parse("c0f1a2c3-1111-2222-3333-444455556666").expect("a uuid")
        ))
    );
    assert_eq!(spare.access("target"), FieldAccess::Unknown);
    assert_eq!(spare.access("size"), FieldAccess::Unknown);
    assert_eq!(spare.access("read_only"), FieldAccess::Unknown);
    assert_eq!(spare.get("device"), Some(&Value::string("/dev/sdc1")));
}

#[tokio::test]
async fn should_restrict_to_mounted_or_unmounted_filesystems_when_asked() {
    let fixture = StorageFixture::new("");
    let target = fixture.mount_point("data");
    fixture
        .uuid("sdb1", "b0f1a2c3-1111-2222-3333-444455556666")
        .block_device("sdb1", "8:17", "ext4")
        .uuid("sdc1", "c0f1a2c3-1111-2222-3333-444455556666")
        .block_device("sdc1", "8:33", "xfs");
    fs::write(
        fixture.path().join("proc/self/mountinfo"),
        format!(
            "36 35 8:17 / {} rw,noatime - ext4 /dev/sdb1 rw\n",
            target.display()
        ),
    )
    .expect("the mount table");

    let unmounted = drain(
        provider(&fixture)
            .snapshot(&Query::target("filesystem").option("mounted", Value::Bool(false)))
            .expect("a snapshot"),
    )
    .await;
    let unmounted = records(&unmounted);
    assert_eq!(unmounted.len(), 1, "got {unmounted:?}");
    assert_eq!(
        unmounted[0].get("source"),
        Some(&Value::string("/dev/sdc1"))
    );

    let mounted = drain(
        provider(&fixture)
            .snapshot(&Query::target("filesystem").option("mounted", Value::Bool(true)))
            .expect("a snapshot"),
    )
    .await;
    let mounted = records(&mounted);
    assert_eq!(mounted.len(), 1, "got {mounted:?}");
    assert_eq!(mounted[0].get("source"), Some(&Value::string("/dev/sdb1")));
}

#[tokio::test]
async fn should_not_report_a_device_whose_filesystem_type_udev_did_not_record() {
    // `type` is required: a device with a uuid link but no udev record is not described, rather
    // than described with an invented type (spec §35.3).
    let fixture = StorageFixture::new("");
    fixture.uuid("sdd1", "d0f1a2c3-1111-2222-3333-444455556666");
    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("filesystem"))
            .expect("a snapshot"),
    )
    .await;
    assert!(records(&collected).is_empty(), "got {collected:?}");
}

// --- persistent definitions and mount units (ADR-0099) ---------------------------------------

use ono_provider_api::{Action, ObjectId};
use ono_provider_systemd::{BusError, JobKind, SystemdBus, UnitListing, UnitProperties};
use ono_value::{ActionStatus, SchemaId};

fn mount_action(operation: &str, target: &str) -> Action {
    Action::new(
        "mount",
        operation,
        ObjectId::new(
            SchemaId::new("ono.mount", 1),
            [Value::Path(Arc::from(Path::new(target)))],
        ),
    )
}

fn error_code(outcome: &ono_provider_api::ActionOutcome) -> String {
    outcome
        .error()
        .map(|error| error.code().code().to_owned())
        .unwrap_or_default()
}

#[tokio::test]
async fn should_append_a_definition_to_fstab_when_adding_a_mount() {
    let fixture = StorageFixture::new("");
    fs::create_dir_all(fixture.path().join("etc")).expect("etc");
    fs::write(
        fixture.path().join("etc/fstab"),
        "UUID=abc / ext4 defaults 0 1\n",
    )
    .expect("the table");
    let action = Action::new(
        "mount",
        "add",
        ObjectId::new(
            SchemaId::new("ono.mount", 1),
            [Value::string("tmpfs"), Value::string("/mnt/my data")],
        ),
    )
    .with("source", Value::string("tmpfs"))
    .with("target", Value::Path(Arc::from(Path::new("/mnt/my data"))))
    .with("type", Value::string("tmpfs"))
    .with(
        "option",
        Value::list([Value::string("size=1m"), Value::string("mode=0700")]),
    );

    let outcome = provider(&fixture).act(&action).await.expect("attempted");
    assert_eq!(outcome.status(), ActionStatus::Success, "{outcome:?}");
    assert!(outcome.changed());
    let table = fs::read_to_string(fixture.path().join("etc/fstab")).expect("the table");
    assert_eq!(
        table,
        "UUID=abc / ext4 defaults 0 1\ntmpfs\t/mnt/my\\040data\ttmpfs\tsize=1m,mode=0700\t0\t0\n",
        "fstab(5): one more line, the space escaped as getmntent decodes it"
    );

    let again = provider(&fixture).act(&action).await.expect("attempted");
    assert_eq!(again.status(), ActionStatus::Failed);
    assert_eq!(
        error_code(&again),
        "Ono-Sendai-E0303",
        "a second definition of the same target is io.already_exists, got {again:?}"
    );
}

#[tokio::test]
async fn should_remove_only_the_definition_of_the_named_target() {
    let fixture = StorageFixture::new("");
    fs::create_dir_all(fixture.path().join("etc")).expect("etc");
    fs::write(
        fixture.path().join("etc/fstab"),
        "# comment\nUUID=abc / ext4 defaults 0 1\ntmpfs /mnt/data tmpfs defaults 0 0\n",
    )
    .expect("the table");

    let outcome = provider(&fixture)
        .act(&mount_action("remove", "/mnt/data"))
        .await
        .expect("attempted");
    assert_eq!(outcome.status(), ActionStatus::Success, "{outcome:?}");
    assert_eq!(
        fs::read_to_string(fixture.path().join("etc/fstab")).expect("the table"),
        "# comment\nUUID=abc / ext4 defaults 0 1\n",
        "the other lines, comments included, stay byte for byte"
    );

    let missing = provider(&fixture)
        .act(&mount_action("remove", "/mnt/data"))
        .await
        .expect("attempted");
    assert_eq!(missing.status(), ActionStatus::Failed);
    assert_eq!(
        error_code(&missing),
        "Ono-Sendai-E0301",
        "a target with no definition is io.not_found, got {missing:?}"
    );
}

#[tokio::test]
async fn should_resolve_a_defined_but_unmounted_mount_by_its_target() {
    let fixture = StorageFixture::new("36 35 8:1 / / rw - ext4 /dev/sda1 rw\n");
    fs::create_dir_all(fixture.path().join("etc")).expect("etc");
    fs::write(
        fixture.path().join("etc/fstab"),
        "/dev/sda1 / ext4 defaults 0 1\n/dev/sdb1 /mnt/data ext4 ro 0 2\n",
    )
    .expect("the table");
    let provider = provider(&fixture);

    let defined = provider
        .resolve(&Selector::field(
            "target",
            Value::Path(Arc::from(Path::new("/mnt/data"))),
        ))
        .await
        .expect("resolved");
    assert_eq!(
        defined.len(),
        1,
        "the definition is an object, got {defined:?}"
    );
    assert_eq!(
        defined[0].id().values(),
        [Value::Path(Arc::from(Path::new("/mnt/data")))]
    );

    let root = provider
        .resolve(&Selector::field(
            "target",
            Value::Path(Arc::from(Path::new("/"))),
        ))
        .await
        .expect("resolved");
    assert_eq!(
        root.len(),
        1,
        "a mount that is both active and defined is one object, got {root:?}"
    );
}

/// A service manager that answers every job the same way.
#[derive(Debug)]
struct RecordedManager {
    answer: Result<(), BusError>,
    jobs: std::sync::Mutex<Vec<(String, JobKind)>>,
}

#[async_trait::async_trait]
impl SystemdBus for RecordedManager {
    async fn manager_version(&self) -> Result<String, BusError> {
        Ok("257".to_owned())
    }
    async fn list_units(&self) -> Result<Vec<UnitListing>, BusError> {
        Ok(Vec::new())
    }
    async fn unit_properties(&self, _unit: &str) -> Result<Option<UnitProperties>, BusError> {
        Ok(None)
    }
    async fn queue_job(&self, unit: &str, job: JobKind) -> Result<(), BusError> {
        self.jobs
            .lock()
            .expect("the job log")
            .push((unit.to_owned(), job));
        self.answer.clone()
    }
    async fn set_unit_file_enabled(&self, _unit: &str, _enabled: bool) -> Result<bool, BusError> {
        Ok(false)
    }
}

#[tokio::test]
async fn should_start_and_stop_a_mount_through_its_systemd_mount_unit() {
    let manager = Arc::new(RecordedManager {
        answer: Ok(()),
        jobs: std::sync::Mutex::new(Vec::new()),
    });
    let fixture = StorageFixture::new("");
    let provider = provider(&fixture).with_units(Arc::clone(&manager) as Arc<dyn SystemdBus>);

    let started = provider
        .act(&mount_action("start", "/mnt/data"))
        .await
        .expect("attempted");
    assert_eq!(started.status(), ActionStatus::Success, "{started:?}");
    let stopped = provider
        .act(&mount_action("stop", "/"))
        .await
        .expect("attempted");
    assert_eq!(stopped.status(), ActionStatus::Success, "{stopped:?}");
    assert_eq!(
        *manager.jobs.lock().expect("the job log"),
        [
            ("mnt-data.mount".to_owned(), JobKind::Start),
            ("-.mount".to_owned(), JobKind::Stop)
        ],
        "systemd.unit(5): the mount unit is the escaped path with the `.mount` suffix"
    );
}

#[tokio::test]
async fn should_report_the_service_manager_s_refusal_as_the_row_s_error() {
    let manager = Arc::new(RecordedManager {
        answer: Err(BusError::PermissionDenied(
            "Interactive authentication required".to_owned(),
        )),
        jobs: std::sync::Mutex::new(Vec::new()),
    });
    let fixture = StorageFixture::new("");
    let provider = provider(&fixture).with_units(manager as Arc<dyn SystemdBus>);
    let outcome = provider
        .act(&mount_action("start", "/mnt/data"))
        .await
        .expect("attempted");
    assert_eq!(outcome.status(), ActionStatus::Failed);
    assert_eq!(error_code(&outcome), "Ono-Sendai-E0302", "{outcome:?}");
}

#[tokio::test]
async fn should_report_the_propagation_peer_group_of_a_shared_mount() {
    // `mountinfo(5)`'s optional fields carry the propagation state, and `shared:N` is what makes
    // two mounts peers: a mount under one appears under the other. Two bind mounts of one shared
    // mount carry the same group; a private mount carries none, and that is an absence the
    // kernel states rather than something unknown (ADR-0236, spec §35.3).
    let fixture = StorageFixture::new("");
    let one = fixture.mount_point("one");
    let two = fixture.mount_point("two");
    let private = fixture.mount_point("private");
    let lines = format!(
        "36 35 8:1 / {one} rw shared:7 - ext4 /dev/sdb1 rw\n\
         37 35 8:1 / {two} rw shared:7 master:3 - ext4 /dev/sdb1 rw\n\
         38 35 8:2 / {private} rw - ext4 /dev/sdb2 rw\n",
        one = one.display(),
        two = two.display(),
        private = private.display(),
    );
    fs::write(fixture.path().join("proc/self/mountinfo"), lines).expect("the mount table");

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("mount"))
            .expect("a snapshot"),
    )
    .await;
    let records = records(&collected);

    for target in [&one, &two] {
        let mount = records
            .iter()
            .find(|record| record.get("target") == Some(&Value::Path(Arc::from(target.clone()))))
            .unwrap_or_else(|| panic!("the fixture declares {}", target.display()));
        assert_eq!(
            mount.get("peer_group"),
            Some(&Value::Int(7)),
            "`shared:7` is the propagation peer group, and both bind mounts are in it"
        );
    }

    let solitary = records
        .iter()
        .find(|record| record.get("target") == Some(&Value::Path(Arc::from(private.clone()))))
        .expect("the fixture declares a private mount");
    assert_eq!(
        solitary.access("peer_group"),
        FieldAccess::Unknown,
        "a private mount propagates nothing, so it is in no peer group"
    );
}
