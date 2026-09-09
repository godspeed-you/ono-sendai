//! What a Btrfs recovery would do to state written after the snapshot (Appendix C.3, C.4).
//!
//! Appendix C.4's example is the whole of this module: a plan wrote `nginx.conf` at 14:03, the
//! operator edited it again at 15:12, and the recovery point is from 14:02. Restoring the file
//! puts back the 14:02 bytes and takes the 15:12 edit away, so the object is `CONFLICTING` and
//! §24.5's gate applies. The recorded fixtures give both sides of exactly that comparison —
//! `snapshot-file.txt` holds `worker_processes 4;` and `live-file.txt` holds
//! `worker_processes 8;` for the same path.
//!
//! Two rules decide a classification, and the second is the one that makes selective restore
//! worth preferring:
//!
//! - an object **in** the restore set whose live content differs from the snapshot is
//!   `CONFLICTING`, because putting the snapshot back discards whatever the difference was;
//! - an object **outside** the restore set is `PRESERVED_BY_METHOD`, because a selective restore
//!   writes only the objects it names. §59.6 and §55.3 case 14 both turn on that being true.
//!
//! Content that cannot be read at all is `UNKNOWN`, which §56.3 turns into a block. It is
//! deliberately not "unchanged": a file the provider could not open is a file it knows nothing
//! about.

use std::path::Path;

use ono_change_core::{NewerStateClass, NewerStateItem};

/// Classifies one object against the snapshot's copy of it (Appendix C.3, C.4).
///
/// `snapshot` and `live` are the bytes on each side, or `None` where the object is absent there.
/// Passing the content rather than the paths keeps the rule a pure function of the two states,
/// which is what lets the gated real-filesystem suite and the recorded fixtures exercise the same
/// classification.
#[must_use]
pub fn classify_object(
    object: &Path,
    in_restore_set: bool,
    snapshot: Option<&[u8]>,
    live: Option<&[u8]>,
) -> NewerStateItem {
    let name = object.to_string_lossy().into_owned();
    if !in_restore_set {
        return NewerStateItem::new(
            name,
            NewerStateClass::PreservedByMethod,
            "a selective restore writes only the objects it names, so this one is left exactly as \
             it is (Appendix C.6)",
        );
    }
    match (snapshot, live) {
        (Some(before), Some(after)) if before == after => NewerStateItem::new(
            name,
            NewerStateClass::PreservedByMethod,
            "the live object already holds what the snapshot holds, so restoring it changes \
             nothing",
        ),
        (Some(_), Some(_)) => NewerStateItem::new(
            name,
            NewerStateClass::Conflicting,
            "the object has changed since the snapshot was taken, and restoring it would discard \
             that change (Appendix C.4)",
        ),
        (Some(_), None) => NewerStateItem::new(
            name,
            NewerStateClass::PreservedByMethod,
            "the object is gone from the live subvolume and present in the snapshot, so restoring \
             it takes nothing away",
        ),
        (None, Some(_)) => NewerStateItem::new(
            name,
            NewerStateClass::Conflicting,
            "the object exists now and the snapshot never held it, so a restore of the captured \
             state cannot put back what is there (Appendix C.6 keeps it rather than deleting it)",
        ),
        (None, None) => NewerStateItem::new(
            name,
            NewerStateClass::Unknown,
            "neither the snapshot nor the live subvolume holds this object, so what recovery \
             would do to it could not be established (§56.3)",
        ),
    }
}

/// The item describing what a whole-subvolume method discards (Appendix C.3, §14.4).
///
/// A replacement or a next-boot switch does not restore named objects; it puts a whole subvolume
/// back, and everything written to the live subvolume since the snapshot goes with it. The
/// evidence that anything *was* written is the generation counter: Btrfs advances it on every
/// commit, so a live subvolume at a higher generation than the snapshot captured has been written
/// to. The files are not enumerated, and this says so rather than implying a count nobody
/// measured.
#[must_use]
pub fn classify_subvolume(
    tree_path: &str,
    live_generation: Option<u64>,
    snapshot_generation: Option<u64>,
) -> Option<NewerStateItem> {
    match (live_generation, snapshot_generation) {
        (Some(live), Some(captured)) if live > captured => Some(NewerStateItem::new(
            format!("everything written in {tree_path} since the snapshot"),
            NewerStateClass::DiscardedByMethod,
            format!(
                "the subvolume is at generation {live} and the snapshot captured generation \
                 {captured}, so it has been written to since; replacing the subvolume discards \
                 every one of those writes. The individual files were not enumerated"
            ),
        )),
        (Some(_), Some(_)) => None,
        _ => Some(NewerStateItem::new(
            format!("everything written in {tree_path} since the snapshot"),
            NewerStateClass::Unknown,
            "the subvolume's generation could not be compared with the snapshot's, so what a \
             replacement would discard could not be established (§56.3)",
        )),
    }
}
