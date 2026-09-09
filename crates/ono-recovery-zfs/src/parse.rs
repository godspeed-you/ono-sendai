//! Readers for what the ZFS tools actually print (spec v0.6 §13.1, Appendix G.1).
//!
//! Every reader here takes the `-H -p` machine form: tab-separated fields, no header, no column
//! padding, byte counts rather than `1.5G`. The human table is never parsed, because its column
//! widths are a function of the longest name in the pool and a provider that reads it will one
//! day split a dataset called `tank/data customer` down the middle.
//!
//! The refusal readers are the other half. §13.6 forbids Ono from adding the flag that stops ZFS
//! refusing, so the refusal text is a *source of facts*: when `zfs rollback` says which snapshots
//! and bookmarks stand in the way, those names are the enumeration Appendix D.5 asks for, and
//! [`rollback_refusal`] turns them into values rather than into a `-r`.

use std::sync::Arc;

use ono_change_core::error::tool_failed;
use ono_value::ErrorValue;

/// The marker OpenZFS prints when the calling user is not permitted to use the utilities.
///
/// §11.4 counts privilege among the things validation must establish, and §43.4 notes recovery
/// may need more privilege than the mutation did. The refusal arrives on stderr with a non-zero
/// status, so it is a fact about the caller rather than a fact about the pool.
const PERMISSION_MARKER: &str = "permission denied";

/// Splits `-H` output into rows of exactly `columns` tab-separated fields.
///
/// A row of the wrong width is a refusal rather than a row read as far as it goes: Appendix G.4
/// makes an unexpected tool output a reason to degrade, and §56.3 makes guessing at the missing
/// column the thing not to do.
///
/// # Errors
///
/// `recovery.provider_unavailable` when a non-empty line does not carry `columns` fields.
pub fn rows<'text>(
    program: &str,
    text: &'text str,
    columns: usize,
) -> Result<Vec<Vec<&'text str>>, ErrorValue> {
    let mut parsed = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != columns {
            return Err(tool_failed(
                program,
                &format!(
                    "v0.6 Appendix G.4: this provider reads the `-H` machine form and expected \
                     {columns} tab-separated fields, and one line carried {}",
                    fields.len()
                ),
            ));
        }
        parsed.push(fields);
    }
    Ok(parsed)
}

/// One `name<TAB>property<TAB>value` triple of `zfs get -H -p -o name,property,value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    /// The object the property belongs to — a dataset, a snapshot, a bookmark.
    pub object: Arc<str>,
    /// The property name.
    pub name: Arc<str>,
    /// The value, verbatim. `-` is ZFS's word for "no value", and it is kept as it arrived.
    pub value: Arc<str>,
}

impl Property {
    /// Whether the value is ZFS's `-`, which means the property has none.
    #[must_use]
    pub fn is_absent(&self) -> bool {
        self.value.as_ref() == "-"
    }
}

/// Reads `zfs get -H -p -o name,property,value ...` output.
///
/// # Errors
///
/// `recovery.provider_unavailable` when a line does not carry three fields.
pub fn properties(program: &str, text: &str) -> Result<Vec<Property>, ErrorValue> {
    rows(program, text, 3)?
        .into_iter()
        .map(|fields| match fields.as_slice() {
            [object, name, value] => Ok(Property {
                object: Arc::from(*object),
                name: Arc::from(*name),
                value: Arc::from(*value),
            }),
            _ => Err(tool_failed(
                program,
                "a `name,property,value` row did not carry three fields",
            )),
        })
        .collect()
}

/// The value of `property` for `object`, where the listing reported one.
#[must_use]
pub fn property_of<'a>(
    listed: &'a [Property],
    object: &str,
    property: &str,
) -> Option<&'a Property> {
    listed
        .iter()
        .find(|entry| entry.object.as_ref() == object && entry.name.as_ref() == property)
}

/// Reads a `-p` byte or count field, which ZFS prints as a plain decimal or as `-`.
#[must_use]
pub fn number(text: &str) -> Option<u128> {
    if text == "-" {
        return None;
    }
    text.parse::<u128>().ok()
}

/// What `zfs rollback` refused to do, and which objects stood in the way (Appendix D.5, §13.6).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RollbackRefusal {
    /// The newer snapshots ZFS named, in the order it named them.
    pub snapshots: Vec<Arc<str>>,
    /// The bookmarks ZFS named.
    pub bookmarks: Vec<Arc<str>>,
}

impl RollbackRefusal {
    /// Every object the refusal named, snapshots before bookmarks.
    #[must_use]
    pub fn objects(&self) -> Vec<Arc<str>> {
        self.snapshots
            .iter()
            .chain(self.bookmarks.iter())
            .map(Arc::clone)
            .collect()
    }

    /// Whether ZFS named anything at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty() && self.bookmarks.is_empty()
    }
}

/// Reads the refusal `zfs rollback` prints when newer history exists.
///
/// The shape is fixed: a `cannot rollback to ...` line, then `use '-r' to force deletion of the
/// following snapshots and bookmarks:`, then one object per line. A name containing `@` is a
/// snapshot and one containing `#` is a bookmark, which is ZFS's own syntax rather than a
/// convention this provider invented.
#[must_use]
pub fn rollback_refusal(text: &str) -> RollbackRefusal {
    let mut refusal = RollbackRefusal::default();
    let mut listing = false;
    for line in text.lines() {
        if line.starts_with("use '-") {
            listing = true;
            continue;
        }
        if !listing {
            continue;
        }
        let name = line.trim();
        if name.is_empty() {
            continue;
        }
        if name.contains('#') {
            refusal.bookmarks.push(Arc::from(name));
        } else if name.contains('@') {
            refusal.snapshots.push(Arc::from(name));
        }
    }
    refusal
}

/// Reads the refusal `zfs destroy` prints when a snapshot has dependent clones (§37).
///
/// The listed datasets are what `-R` would take with it. §13.6's prohibition is what makes this a
/// reader rather than a retry: the names become a structured refusal, and the flag is never added.
#[must_use]
pub fn destroy_refusal(text: &str) -> Vec<Arc<str>> {
    let mut dependents = Vec::new();
    let mut listing = false;
    for line in text.lines() {
        if line.starts_with("use '-") {
            listing = true;
            continue;
        }
        if !listing {
            continue;
        }
        let name = line.trim();
        if !name.is_empty() {
            dependents.push(Arc::from(name));
        }
    }
    dependents
}

/// Whether ZFS refused because the caller lacks the privilege the utilities need (§11.4, §43.4).
#[must_use]
pub fn is_permission_refusal(text: &str) -> bool {
    text.to_ascii_lowercase().contains(PERMISSION_MARKER)
}

/// Whether ZFS refused because the object named does not exist.
#[must_use]
pub fn is_missing_object(text: &str) -> bool {
    text.contains("does not exist")
}

/// Whether ZFS refused because the object named already exists.
///
/// §13.3 is why this is a fact rather than a failure: one `zfs snapshot -r` creates the snapshot
/// on every descendant dataset, so the sibling protection action finds its own snapshot already
/// there, and Appendix D.1 still wants its identity recorded individually.
#[must_use]
pub fn is_already_present(text: &str) -> bool {
    text.contains("already exists")
}

/// The OpenZFS release `zfs version` reports, as its first line names it.
///
/// The first line is `zfs-<version>`; the second is the kernel module's. Appendix G.4 turns this
/// into the difference between running and degrading, so an unreadable line is `None` and never a
/// version this provider then claims to have validated.
#[must_use]
pub fn version(text: &str) -> Option<Arc<str>> {
    text.lines()
        .next()?
        .trim()
        .strip_prefix("zfs-")
        .map(Arc::from)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_refuse_a_row_of_the_wrong_width_rather_than_read_it_as_far_as_it_goes() {
        let error = rows("zfs", "tank\tfilesystem", 6)
            .expect_err("Appendix G.4: an unexpected tool output degrades rather than guesses");
        assert_eq!(error.code().name(), "recovery.provider_unavailable");
    }

    #[test]
    fn should_read_zfs_absence_as_absence_rather_than_as_zero() {
        assert_eq!(number("-"), None, "§38.2: `-` is unknown, and zero is a claim");
        assert_eq!(number("24576"), Some(24576));
    }
}
