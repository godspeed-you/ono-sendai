//! The two blocks §13 asks the ZFS provider to be able to show (§13.4, §13.8).
//!
//! §13.4's block is the one that carries an argument. It names the target, the dataset that
//! actually holds it, the snapshot that protects it — and then, under `NOT PROTECTED BY`, the
//! snapshots an operator might otherwise have assumed covered it. A protection claim that only
//! ever says what *is* covered leaves the reader to work out the rest, and the whole of §13.4 is
//! that they will work it out wrongly.
//!
//! Rendering lives here rather than in a renderer crate because both blocks are statements about
//! ZFS semantics: which dataset is the boundary, and what a snapshot of a different dataset does
//! not reach. The text is a value the caller may print, log or fold into a larger view.

use std::sync::Arc;

use ono_change_core::{RecoveryAsset, RecoveryValidation};

use crate::layout::Layout;

/// §13.4's worked example, as a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryReport {
    target: Arc<str>,
    dataset: Arc<str>,
    snapshot: Arc<str>,
    not_protected_by: Vec<Arc<str>>,
}

impl BoundaryReport {
    /// Builds the report for `target` from a reading of the pool.
    ///
    /// `not_protected_by` is evidence rather than illustration: it names the snapshots that
    /// really exist, under the same name, on datasets that are not the one holding the target. In
    /// §13.4's own example that is the root dataset's snapshot beside `tank/data`'s.
    #[must_use]
    pub fn of(layout: &Layout, target: &str, snapshot_part: &str) -> Option<Self> {
        let dataset = layout.dataset_of_path(target)?;
        let snapshot: Arc<str> = Arc::from(format!("{}@{snapshot_part}", dataset.name));
        let not_protected_by = layout
            .snapshots()
            .iter()
            .filter(|other| other.dataset != dataset.name && other.short.as_ref() == snapshot_part)
            .map(|other| Arc::clone(&other.name))
            .collect();
        Some(Self {
            target: Arc::from(target),
            dataset: Arc::clone(&dataset.name),
            snapshot,
            not_protected_by,
        })
    }

    /// The path the plan targets.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// The dataset that actually holds it (Appendix B.8).
    #[must_use]
    pub fn dataset(&self) -> &str {
        &self.dataset
    }

    /// The snapshot that protects it.
    #[must_use]
    pub fn snapshot(&self) -> &str {
        &self.snapshot
    }

    /// The snapshots that do not, whatever their names suggest (§13.4).
    #[must_use]
    pub fn not_protected_by(&self) -> &[Arc<str>] {
        &self.not_protected_by
    }

    /// §13.4's block.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = format!(
            "TARGET\n  {}\n\nPERSISTENCE\n  dataset {}\n\nRECOVERY\n  snapshot {}\n",
            self.target, self.dataset, self.snapshot
        );
        if !self.not_protected_by.is_empty() {
            text.push_str("\nNOT PROTECTED BY\n");
            for other in &self.not_protected_by {
                text.push_str("  ");
                text.push_str(other);
                text.push('\n');
            }
        }
        text
    }
}

/// §13.8's `RECOVERY ASSET` block for one ZFS snapshot.
///
/// The `retained` line and the `excluded` list are the two an operator reads for consequences:
/// §37.1 fixes the retention, and §13.4's separate datasets, §33.1's process state and §34's
/// network sessions are what a dataset snapshot does not hold. §11.5 is why the type line says
/// "ZFS snapshot" rather than "backup".
#[must_use]
pub fn render_asset(asset: &RecoveryAsset) -> String {
    let dataset = asset.reference().split_once('@').map_or_else(
        || asset.scope().domain().to_owned(),
        |(head, _)| head.to_owned(),
    );
    let retained = format!(
        "{}h after verification",
        asset.retention().window().as_secs() / 3600
    );
    let created = asset.validation().map_or_else(
        || "just-in-time before mutation".to_owned(),
        |validation| format!("checked {}", validation.at()),
    );
    let mut text = format!(
        "RECOVERY ASSET\n  type          ZFS snapshot\n  dataset       {dataset}\n  snapshot      \
         {}\n  consistency   {}\n  created       {created}\n  restore       {}\n  retained      \
         {retained}\n  state         {}\n",
        asset.reference(),
        asset.consistency().as_str(),
        asset.restore_method().as_str(),
        asset.state().as_str(),
    );
    if !asset.exclusions().is_empty() {
        text.push_str("\nexcluded\n");
        for exclusion in asset.exclusions() {
            text.push_str(&format!(
                "  {:<14}{}\n",
                exclusion.subject(),
                exclusion.reason()
            ));
        }
    }
    if let Some(validation) = asset.validation()
        && !validation.is_complete()
    {
        text.push_str("\nnot validated\n");
        for failure in RecoveryValidation::failures(validation) {
            text.push_str("  ");
            text.push_str(failure);
            text.push('\n');
        }
    }
    text
}
