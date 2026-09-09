//! What a Btrfs recovery scope names (§11.2, §14.1, Appendix D.6).
//!
//! §11.2 requires a path to be mapped to its containing persistence domain before protection is
//! claimed, and forbids the domain from being a path. For Btrfs the domain is Appendix D.6's pair
//! — `filesystem_uuid` and `source_subvol_id` — because a subvolume ID is unique inside one
//! filesystem and means nothing outside it. The tree path travels with them: it is not part of
//! the identity, it is what makes the identity legible, and every comparison in this crate is
//! made on the id.
//!
//! The three are carried through [`ono_change_core::RecoveryScope`] as one text so that a scope
//! read back out of the store resolves to the same subvolume it did when it was written. The
//! spelling is `<filesystem-uuid>:<subvolume-id>:<tree-path>`, which is unambiguous because a
//! Btrfs UUID and a subvolume ID never contain a colon.

use std::sync::Arc;

/// The subvolume a recovery scope names (§11.2, Appendix D.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubvolumeRef {
    filesystem: Arc<str>,
    id: u64,
    tree_path: Arc<str>,
}

impl SubvolumeRef {
    /// Names subvolume `id` on filesystem `filesystem`, visible in the tree at `tree_path`.
    #[must_use]
    pub fn new(filesystem: impl Into<Arc<str>>, id: u64, tree_path: impl Into<Arc<str>>) -> Self {
        Self {
            filesystem: filesystem.into(),
            id,
            tree_path: tree_path.into(),
        }
    }

    /// The filesystem UUID — Appendix D.6's `filesystem_uuid`.
    #[must_use]
    pub fn filesystem(&self) -> &str {
        &self.filesystem
    }

    /// The stable subvolume ID — Appendix D.6's `source_subvol_id` (§14.1).
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// The subvolume's path inside the filesystem tree, such as `@var`.
    #[must_use]
    pub fn tree_path(&self) -> &str {
        &self.tree_path
    }

    /// The text a [`ono_change_core::RecoveryScope`] carries as its domain.
    #[must_use]
    pub fn reference(&self) -> String {
        format!("{}:{}:{}", self.filesystem, self.id, self.tree_path)
    }

    /// Reads a reference back, or `None` when the text is not one.
    ///
    /// `None` rather than a partial reading: §56.3 makes a scope that cannot be resolved a reason
    /// to block, and half a subvolume identity is not an identity.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (filesystem, rest) = text.split_once(':')?;
        let (id, tree_path) = rest.split_once(':')?;
        if filesystem.is_empty() || tree_path.is_empty() {
            return None;
        }
        Some(Self::new(filesystem, id.parse().ok()?, tree_path))
    }

    /// The sentence naming this subvolume for a person (Appendix B.10).
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "subvolume {} ({}) on Btrfs filesystem {}",
            self.id, self.tree_path, self.filesystem
        )
    }
}
