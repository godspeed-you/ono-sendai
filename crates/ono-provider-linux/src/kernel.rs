//! The running kernel and the operating system it boots, as one `ono.kernel/1` record
//! (spec §8.1's `kernel` target, ADR-0964).
//!
//! Everything comes from the kernel's own files — `/proc/sys/kernel/*`, `/proc/stat`,
//! `/proc/uptime` — and from `os-release(5)`. Nothing is inferred: a file that is not there
//! leaves its field null, and the record says what it read.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use jiff::Timestamp;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{
    Availability, Capability, ObjectRef, Provider, Query, Risk, Selector, TemporalCapabilities,
};
use ono_value::{Duration, ErrorValue, RecordValue, Schema, Value};

use crate::common::provenance;
use crate::{procfs, schemas};

pub const PROVIDER_ID: &str = "linux.kernel";

/// The `os-release(5)` locations, in the order the manual says to try them.
const OS_RELEASE: [&str; 2] = ["etc/os-release", "usr/lib/os-release"];

/// Provider for the `kernel` target.
#[derive(Debug, Clone)]
pub struct KernelProvider {
    root: PathBuf,
}

impl Default for KernelProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl KernelProvider {
    /// The kernel this shell runs on.
    #[must_use]
    pub fn new() -> Self {
        Self::rooted("/")
    }

    /// The kernel whose interfaces are mounted under `root` — `<root>/proc`, `<root>/etc`.
    #[must_use]
    pub fn rooted(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    fn proc_root(&self) -> PathBuf {
        self.root.join("proc")
    }

    /// One line of a `/proc/sys/kernel` file, trimmed; `None` where it cannot be read or is empty.
    fn sys_kernel(&self, name: &str) -> Option<String> {
        let text = fs::read_to_string(self.proc_root().join("sys/kernel").join(name)).ok()?;
        let trimmed = text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    }

    /// `PRETTY_NAME` and `ID` of the first `os-release(5)` file that is readable.
    fn os_release(&self) -> (Option<String>, Option<String>) {
        let Some(text) = OS_RELEASE
            .iter()
            .find_map(|relative| fs::read_to_string(self.root.join(relative)).ok())
        else {
            return (None, None);
        };
        let field = |name: &str| {
            text.lines().find_map(|line| {
                let value = line.strip_prefix(name)?.strip_prefix('=')?.trim();
                let unquoted = value
                    .strip_prefix('"')
                    .and_then(|rest| rest.strip_suffix('"'))
                    .or_else(|| {
                        value
                            .strip_prefix('\'')
                            .and_then(|rest| rest.strip_suffix('\''))
                    })
                    .unwrap_or(value);
                (!unquoted.is_empty()).then(|| unquoted.to_owned())
            })
        };
        (field("PRETTY_NAME"), field("ID"))
    }

    fn record(&self, schema: &Arc<Schema>) -> Result<RecordValue, ErrorValue> {
        let proc_root = self.proc_root();
        let release = self.sys_kernel("osrelease").ok_or_else(|| {
            ErrorValue::new(
                ono_core::ErrorCode::ProviderUnavailable,
                format!(
                    "{} is not readable, so the running kernel cannot be named",
                    proc_root.join("sys/kernel/osrelease").display()
                ),
            )
        })?;
        let (os, os_id) = self.os_release();
        let text = |value: Option<String>| value.map_or(Value::Null, |text| Value::string(&text));
        let booted = procfs::boot_time_seconds(&proc_root)
            .and_then(|seconds| Timestamp::from_second(seconds).ok())
            .map_or(Value::Null, Value::Timestamp);
        let uptime = procfs::uptime_seconds(&proc_root).map_or(Value::Null, |seconds| {
            // Whole nanoseconds: `/proc/uptime` prints hundredths of a second.
            #[allow(clippy::cast_possible_truncation)]
            let nanoseconds = (seconds * 1_000_000_000.0).round() as i128;
            Value::Duration(Duration::from_nanoseconds(nanoseconds))
        });
        let architecture = nix::sys::utsname::uname()
            .ok()
            .map(|name| name.machine().to_string_lossy().into_owned());
        Ok(RecordValue::builder(
            Arc::clone(schema),
            provenance(
                PROVIDER_ID,
                schema.id(),
                "/proc/sys/kernel + /proc/stat + /proc/uptime + os-release(5)",
            ),
        )
        .set("release", Value::string(&release))?
        .set("version", text(self.sys_kernel("version")))?
        .set("architecture", text(architecture))?
        .set("hostname", text(self.sys_kernel("hostname")))?
        .set("os", text(os))?
        .set("os_id", text(os_id))?
        .set("boot_id", text(procfs::boot_id(&proc_root)))?
        .set("booted", booted)?
        .set("uptime", uptime)?
        .build())
    }
}

#[async_trait::async_trait]
impl Provider for KernelProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["kernel"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        schemas::require(&schemas::kernel_id())
            .into_iter()
            .collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("kernel.read", Risk::Read)]
    }

    fn temporal(&self) -> TemporalCapabilities {
        // The kernel as it is now. A reboot is a new boot id, not an event this provider sees.
        TemporalCapabilities {
            current_snapshot: true,
            live_events: false,
            historical_query: false,
            exhaustive_events: false,
            causal_tokens: false,
            checkpointable: true,
            retained_history: None,
        }
    }

    fn availability(&self) -> Availability {
        let osrelease = self.proc_root().join("sys/kernel/osrelease");
        if osrelease.is_file() {
            Availability::Available
        } else {
            Availability::unavailable(format!("{} is not readable", osrelease.display()))
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let schema = schemas::require(&schemas::kernel_id())?;
        let record = self.record(&schema);
        let selectors: Vec<Selector> = query.selectors().to_vec();
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                match record {
                    Ok(record) => {
                        if selectors.iter().all(|selector| selector.matches(&record)) {
                            let _ = sink.send(record.into_value()).await;
                        }
                    }
                    Err(error) => {
                        let _ = sink.fail(error).await;
                    }
                }
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let schema = schemas::require(&schemas::kernel_id())?;
        let record = self.record(&schema)?;
        Ok(if selector.matches(&record) {
            ObjectRef::of(&record).into_iter().collect()
        } else {
            Vec::new()
        })
    }
}
