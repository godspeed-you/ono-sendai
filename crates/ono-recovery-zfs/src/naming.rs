//! Snapshot names Ono generates, and the rules they are held to (Appendix D.2, §43.6).
//!
//! Appendix D.2 asks for `ono-<plan-short-id>-<utc-timestamp>`: collision-resistant because the
//! timestamp is to the second and the plan id is a digest prefix, human-readable because an
//! operator reading `zfs list` should be able to tell which plan put a snapshot there.
//!
//! §43.6 is the other half and it is the one with teeth. A provider-generated name MUST be
//! sanitised and MUST NOT contain user-controlled command syntax. Two things make that true here:
//! [`sanitise`] reduces its input to the characters ZFS accepts in a name component, and nothing
//! in this crate ever builds a command *line* — [`ono_change_core::ToolRunner`] takes an argument
//! vector, so a dataset called `tank/x; rm -rf /` is one argument containing a semicolon rather
//! than two commands. Both are needed: the vector protects the shell, and the sanitiser protects
//! ZFS's own parser, for which `@`, `/`, `#` and `%` are syntax.

use std::sync::Arc;

use jiff::Timestamp;

/// The longest a ZFS name component may be, as OpenZFS's `ZFS_MAX_DATASET_NAME_LEN` fixes it.
///
/// The limit is on the whole `pool/dataset@snapshot` name, so a snapshot part that fits inside it
/// on its own can still overflow once the dataset is in front. The provider keeps its generated
/// part far below the limit and [`is_valid_snapshot_part`] enforces the absolute bound.
pub const MAX_NAME_LEN: usize = 255;

/// The longest sanitised plan-short-id this provider will place in a name.
///
/// `PlanId::short` is eight hexadecimal characters, so the cap only ever bites on a caller that
/// passed something else — and a name is easier to read short than complete.
pub const MAX_SHORT_ID_LEN: usize = 16;

/// The word used where a protection was not created for a plan (Appendix D.2).
///
/// §17.1's just-in-time protection always belongs to a plan. A provider driven directly — the
/// gated real-filesystem harness, an operator creating a recovery point by hand — has no plan id,
/// and names its snapshots so that the two are distinguishable in `zfs list` rather than
/// borrowing an id that does not exist.
pub const NO_PLAN: &str = "manual";

/// Whether `character` is one ZFS accepts inside a dataset or snapshot name component.
///
/// OpenZFS permits alphanumerics and `_ - : .` in a component. Everything else — `/` for the
/// hierarchy, `@` for snapshots, `#` for bookmarks, `%` for internal names, whitespace, and every
/// shell metacharacter — is refused here rather than passed to ZFS to refuse later.
#[must_use]
pub const fn is_permitted(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(character, '_' | '-' | ':' | '.')
}

/// Reduces `text` to the characters ZFS accepts, collapsing every run of the rest to one `_`.
///
/// Collapsing rather than dropping keeps two different inputs from colliding on one output more
/// often than dropping would, and it keeps the result readable. A leading or trailing separator
/// is trimmed, because a name that begins with `-` reads as an option to every tool that ever
/// sees it.
#[must_use]
pub fn sanitise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending = false;
    for character in text.chars() {
        if is_permitted(character) {
            if pending && !out.is_empty() {
                out.push('_');
            }
            pending = false;
            out.push(character);
        } else {
            pending = true;
        }
    }
    let trimmed = out.trim_matches(|character| matches!(character, '_' | '-' | '.' | ':'));
    trimmed.to_owned()
}

/// The `<utc-timestamp>` half of Appendix D.2's name: `YYYYMMDDTHHMMSSZ`.
///
/// Basic ISO 8601, because the extended form's colons are legal in a ZFS name and illegible in a
/// listing. Seconds resolution matches what `zfs list -p` reports for `creation`, so a name and
/// the creation time ZFS records for it agree.
#[must_use]
pub fn timestamp(at: Timestamp) -> String {
    let date = at.strftime("%Y%m%dT%H%M%SZ").to_string();
    sanitise(&date)
}

/// Builds Appendix D.2's `ono-<plan-short-id>-<utc-timestamp>` snapshot part.
///
/// Both halves are sanitised before they meet, so a plan id that arrived from somewhere other
/// than [`ono_change_core::PlanId::short`] cannot introduce `@`, `/` or shell syntax into a name
/// this provider then hands to ZFS (§43.6).
#[must_use]
pub fn snapshot_part(plan_short_id: Option<&str>, at: Timestamp) -> Arc<str> {
    let mut short = sanitise(plan_short_id.unwrap_or(NO_PLAN));
    short.truncate(MAX_SHORT_ID_LEN);
    let short = short.trim_matches('_').to_owned();
    let short = if short.is_empty() {
        NO_PLAN.to_owned()
    } else {
        short
    };
    Arc::from(format!("ono-{short}-{}", timestamp(at)))
}

/// The full `dataset@snapshot` reference a protection action names.
#[must_use]
pub fn full_name(dataset: &str, part: &str) -> Arc<str> {
    Arc::from(format!("{dataset}@{part}"))
}

/// Whether `part` is a name ZFS will accept after the `@`.
///
/// The checks are ZFS's own: non-empty, within the length bound, only permitted characters, and a
/// first character that is alphanumeric so the name cannot be mistaken for an option.
#[must_use]
pub fn is_valid_snapshot_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= MAX_NAME_LEN
        && part.chars().all(is_permitted)
        && part.starts_with(|character: char| character.is_ascii_alphanumeric())
}

/// Whether `name` is a full `dataset@snapshot` reference ZFS will accept.
#[must_use]
pub fn is_valid_snapshot_name(name: &str) -> bool {
    let Some((dataset, part)) = name.split_once('@') else {
        return false;
    };
    name.len() <= MAX_NAME_LEN
        && is_valid_snapshot_part(part)
        && !dataset.is_empty()
        && dataset
            .split('/')
            .all(|component| !component.is_empty() && component.chars().all(is_permitted))
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
    fn should_strip_every_character_zfs_treats_as_syntax() {
        for forbidden in ['@', '/', '#', '%', ' ', '\t'] {
            let cleaned = sanitise(&format!("a{forbidden}b"));
            assert!(
                !cleaned.contains(forbidden),
                "§43.6: `{forbidden}` is ZFS syntax and must not survive sanitisation"
            );
        }
    }

    #[test]
    fn should_never_produce_a_name_that_reads_as_an_option() {
        let part = snapshot_part(Some("--force"), Timestamp::UNIX_EPOCH);
        assert!(
            is_valid_snapshot_part(&part),
            "Appendix D.2: the generated name must comply with ZFS naming rules"
        );
        assert!(part.starts_with("ono-"));
    }
}
