//! The Ono `service` provider: systemd units, read through D-Bus.
//!
//! Spec §23.3 is one sentence and it is the whole design: "Use systemd D-Bus APIs where
//! available rather than shelling out to `systemctl` and parsing text." Nothing in this crate
//! runs a program or reads its output. Every field of `ono.service/1` comes from
//! `org.freedesktop.systemd1` — `Manager.ListUnits`, `Manager.LoadUnit` and
//! `org.freedesktop.DBus.Properties.GetAll` — and from nowhere else (spec §50).
//!
//! The crate also carries the [`JournalProvider`] for the `journal` and `log` targets. The
//! journal has no D-Bus surface; that provider runs `journalctl --output=json` — the machine
//! format systemd documents — through the decoder of the v0.3 systemd adapter pack (ADR-0085).
//!
//! # Being honest about not being there
//!
//! systemd is absent from a great many machines Ono runs on: a Docker container, a WSL session,
//! a BSD-adjacent system, anything using another init. [`SystemdProvider::connect`] detects that
//! by looking for the D-Bus system bus socket and then reading
//! `org.freedesktop.systemd1.Manager.Version` — not by looking for a `systemctl` binary, which
//! is present in plenty of containers where systemd is not running at all.
//!
//! Where no manager answers, the provider reports
//! [`Availability::Unavailable`](ono_provider_api::Availability::Unavailable) with a reason and
//! refuses to answer queries. It does *not* return an empty stream, because an empty stream says
//! "this machine has no services", which is a different and false claim. Keeping ignorance apart
//! from absence is what the value model exists for (spec §10.5, §35.3).
//!
//! # The seam
//!
//! [`SystemdBus`] is the D-Bus surface this provider uses, as a trait. One implementation talks
//! to the real system bus; the crate's tests supply another that replays recorded systemd
//! responses, so the positive path is a tested contract on machines where systemd is not
//! running. That is a fake of the system being read, in the sense AGENTS.md §11 permits, and not
//! a mock of anything this crate wrote.
//!
//! # What a record carries
//!
//! Every field of `ono.service/1` (spec §28.3), plus the provider extensions of spec §10.4 that
//! the workflows of spec §33.2 and §41.4 need to investigate a failure:
//!
//! | Extension | What it is |
//! |---|---|
//! | `systemd.load_state` | `loaded`, `masked`, `not-found` |
//! | `systemd.unit_file_state` | `enabled`, `disabled`, `masked`, `static`, … |
//! | `systemd.memory` | `MemoryCurrent`, or null where accounting is off |
//! | `systemd.tasks` | `TasksCurrent`, or null where accounting is off |
//! | `systemd.result` | `exit-code`, `signal`, `timeout`, `oom-kill`, … |
//! | `systemd.exit_code` | `ExecMainStatus`, the status the main process left with |
//!
//! Unknown is null throughout. systemd's two sentinels — `MainPID = 0` and
//! `MemoryCurrent = u64::MAX` — are read as "there is none" and "I do not know", never copied
//! through as numbers (spec §35.3).

#![forbid(unsafe_code)]

mod bus;
mod dbus;
mod journal;
mod provider;
mod record;

pub use bus::{BusError, JobKind, SystemdBus, UnitListing, UnitProperties};
pub use dbus::SystemBus;
pub use journal::{JOURNAL_PROVIDER_ID, JournalProvider};
pub use provider::{PROVIDER_ID, SystemdProvider};
pub use record::{service_schema, unit_name_candidates};
