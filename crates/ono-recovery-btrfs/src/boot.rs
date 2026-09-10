//! How the next boot selects its root subvolume (§14.6, §56.2, Appendix D.9).
//!
//! A next-boot recovery of the root changes what boots, and Btrfs offers two ways to do that
//! which are *not* interchangeable. `btrfs subvolume set-default` changes the subvolume a mount
//! with no `subvol=` option lands in. A rename changes which subvolume answers to a name. Which of
//! the two the next boot honours is decided by the boot entry, not by the filesystem:
//!
//! - `rootflags=subvol=@` on the kernel command line — what `grub-mkconfig` writes for a root on
//!   a subvolume — selects the root **by name**, and the default subvolume is never consulted;
//! - `rootflags=subvolid=256` selects it **by id**, and neither a rename nor a new default
//!   changes what boots;
//! - no `subvol` flag at all mounts the filesystem's **default** subvolume.
//!
//! So the provider reads the boot entry before it chooses, and §56.3 turns an entry it cannot see
//! into a refusal. The kernel command line of the running boot is the entry it can see, and it
//! only speaks for this filesystem when its `root=` names it. The root's own `/etc/fstab` is read
//! beside it, because a `/` line that names another subvolume contradicts the steering.

use std::sync::Arc;

/// Where the running kernel's command line is read from.
pub const KERNEL_CMDLINE: &str = "/proc/cmdline";

/// How the next boot selects the root subvolume of one filesystem (§14.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootSelection {
    /// The boot entry names the subvolume by its tree path, so a rename steers it.
    ByName {
        /// The tree path, without leading or trailing `/`.
        tree_path: Arc<str>,
        /// The sentence saying where that was read.
        evidence: Arc<str>,
    },
    /// The boot entry names the subvolume by id, so nothing the filesystem offers steers it.
    ById {
        /// The id it names.
        id: u64,
        /// The sentence saying where that was read.
        evidence: Arc<str>,
    },
    /// The boot entry names no subvolume, so the filesystem's default subvolume boots.
    ByDefault {
        /// The tree path the root's `/etc/fstab` names for `/`, where it names one.
        also_named: Option<Arc<str>>,
        /// The sentence saying where that was read.
        evidence: Arc<str>,
    },
    /// How the next boot selects its root could not be established (§56.3).
    Unobservable {
        /// Why, in a sentence a person can act on.
        reason: Arc<str>,
    },
}

/// Reads how the next boot selects the root of the filesystem `filesystem_uuid` (§14.6).
///
/// `cmdline` is the kernel command line, `fstab` the root subvolume's own `/etc/fstab`, and
/// `devices` the device paths the filesystem's mounts show, so a `root=/dev/…` can be tied to it
/// as well as a `root=UUID=…`.
#[must_use]
pub fn boot_selection(
    cmdline: Option<&str>,
    fstab: Option<&str>,
    filesystem_uuid: &str,
    devices: &[&str],
) -> BootSelection {
    let Some(cmdline) = cmdline else {
        return unobservable(format!(
            "the kernel command line at {KERNEL_CMDLINE} could not be read"
        ));
    };
    let tokens: Vec<&str> = cmdline.split_whitespace().collect();
    // The kernel honours the last occurrence of a parameter, so the last one is read.
    let Some(root) = tokens
        .iter()
        .rev()
        .find_map(|token| token.strip_prefix("root="))
    else {
        return unobservable(
            "the kernel command line carries no `root=`, so which filesystem it boots is not \
             stated"
                .to_owned(),
        );
    };
    let applies = root.strip_prefix("UUID=").map_or_else(
        || devices.contains(&root),
        |uuid| uuid.eq_ignore_ascii_case(filesystem_uuid),
    );
    if !applies {
        return unobservable(format!(
            "the kernel command line boots `root={root}`, which is not Btrfs filesystem \
             {filesystem_uuid}; the boot entry that starts this filesystem's root is not visible \
             from here"
        ));
    }
    let flags = tokens
        .iter()
        .rev()
        .find_map(|token| token.strip_prefix("rootflags="))
        .map(Flags::read)
        .unwrap_or_default();
    let listed = fstab
        .and_then(|text| root_entry(text, filesystem_uuid))
        .map(Flags::read)
        .unwrap_or_default();
    if let Some(raw) = flags.subvolid.as_deref().or(listed.subvolid.as_deref()) {
        let origin = if flags.subvolid.is_some() {
            "on the kernel command line"
        } else {
            "on the `/` line of the root's /etc/fstab"
        };
        return match raw.parse::<u64>() {
            Ok(id) => BootSelection::ById {
                id,
                evidence: Arc::from(format!("`subvolid={raw}` {origin} selects subvolume {id}")),
            },
            Err(_) => unobservable(format!("`subvolid={raw}` {origin} is not a subvolume id")),
        };
    }
    match (flags.subvol.as_deref(), listed.subvol.as_deref()) {
        (Some(booted), Some(listed)) if normalise(booted) != normalise(listed) => {
            unobservable(format!(
                "the kernel command line selects `{booted}` and the root's /etc/fstab names \
                 `{listed}` for `/`; the two disagree about what this root is"
            ))
        }
        (Some(booted), _) => BootSelection::ByName {
            tree_path: Arc::from(normalise(booted)),
            evidence: Arc::from(format!(
                "`rootflags=subvol={booted}` on the kernel command line for filesystem \
                 {filesystem_uuid} selects the root by name"
            )),
        },
        (None, listed) => BootSelection::ByDefault {
            also_named: listed.map(|path| Arc::from(normalise(path))),
            evidence: Arc::from(format!(
                "the kernel command line for filesystem {filesystem_uuid} names no subvolume, so \
                 the next boot mounts the filesystem's default subvolume"
            )),
        },
    }
}

fn unobservable(reason: String) -> BootSelection {
    BootSelection::Unobservable {
        reason: Arc::from(reason),
    }
}

/// A subvolume path without the leading and trailing `/` a boot entry may spell it with.
fn normalise(path: &str) -> &str {
    path.trim_matches('/')
}

/// The options of the `/` line of `fstab` when it is a Btrfs line of this filesystem.
fn root_entry<'a>(fstab: &'a str, filesystem_uuid: &str) -> Option<&'a str> {
    fstab
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.split_whitespace().collect::<Vec<&str>>())
        .find(|fields| fields.get(1) == Some(&"/") && fields.get(2) == Some(&"btrfs"))
        .filter(|fields| {
            fields.first().is_some_and(|device| {
                device
                    .strip_prefix("UUID=")
                    .is_none_or(|uuid| uuid.eq_ignore_ascii_case(filesystem_uuid))
            })
        })
        .and_then(|fields| fields.get(3).copied())
}

/// The subvolume a comma-separated option list selects.
#[derive(Debug, Default)]
struct Flags {
    subvol: Option<String>,
    subvolid: Option<String>,
}

impl Flags {
    fn read(options: &str) -> Self {
        let mut flags = Self::default();
        for option in options.split(',') {
            if let Some(value) = option.strip_prefix("subvolid=") {
                flags.subvolid = Some(value.to_owned());
            } else if let Some(value) = option.strip_prefix("subvol=") {
                flags.subvol = Some(value.to_owned());
            }
        }
        flags
    }
}
