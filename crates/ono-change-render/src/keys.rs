//! Appendix E.3's inspector key bindings.
//!
//! Appendix E.3 calls these *reference* bindings and says they "MAY be configurable, but these
//! meanings should guide discoverability". The table is therefore data rather than a `match` in
//! an event loop: `help` lists it, the CLI binds it, and a user who rebinds `P` still finds
//! protection under whatever they bound it to, because the *meaning* is what is named here.
//!
//! §62.10 is why the table lives in this crate and not in `ono-cli`: v0.6 is too complex to live
//! as more branches inside the CLI integration crate. The event loop is the CLI's — it owns the
//! terminal — and the vocabulary of what an inspector can do is the view layer's.

use crate::{display_width, fit};

/// What one inspector binding does (Appendix E.3).
///
/// Every action is a *view* change or a navigation. `Apply` is the exception and it is the one
/// that opens §40's gate rather than performing anything itself, because §2.1 keeps the inspector
/// side-effect free right up to the moment the operator crosses the gate deliberately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InspectorAction {
    /// Move the selection within the current list.
    Move,
    /// Open the selected item (§20.2's actions, targets and impact nodes).
    Inspect,
    /// Expand or collapse the selected group (Appendix E.2's collapsed lines).
    Toggle,
    /// The impact graph of §9.
    Impact,
    /// The coverage matrix of §10.3, with its exclusions (Appendix E.8).
    Protection,
    /// The recovery preview of §24.4, which plans and changes nothing (§5.8).
    RecoveryPreview,
    /// The verification contracts of §23.1.
    Verification,
    /// The spatial projection of the proposed state (§21, `map --plan`).
    Map,
    /// The temporal context of §22 (`timeline`).
    Timeline,
    /// Apply, which opens §19.4's gate where the plan requires one.
    Apply,
    /// Leave the current pane.
    Back,
}

impl InspectorAction {
    /// The sentence `help` prints for the action, which is the meaning Appendix E.3 fixes.
    #[must_use]
    pub const fn meaning(self) -> &'static str {
        match self {
            InspectorAction::Move => "move",
            InspectorAction::Inspect => "inspect selected item",
            InspectorAction::Toggle => "expand/collapse",
            InspectorAction::Impact => "impact",
            InspectorAction::Protection => "protection",
            InspectorAction::RecoveryPreview => "recovery preview",
            InspectorAction::Verification => "verification contracts",
            InspectorAction::Map => "map --plan",
            InspectorAction::Timeline => "timeline context",
            InspectorAction::Apply => "apply (opens gate if required)",
            InspectorAction::Back => "back",
        }
    }
}

/// One reference binding: the keys, and what they do (Appendix E.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    keys: &'static str,
    action: InspectorAction,
}

impl Binding {
    /// The keys as Appendix E.3 writes them, which is what `help` shows the user.
    #[must_use]
    pub const fn keys(self) -> &'static str {
        self.keys
    }

    /// What pressing them does.
    #[must_use]
    pub const fn action(self) -> InspectorAction {
        self.action
    }
}

/// The reference bindings of Appendix E.3, in the order the appendix lists them.
///
/// The order is the appendix's rather than alphabetical, because it is a teaching order: move,
/// open, expand, then the panes, then apply, then back.
pub const BINDINGS: &[Binding] = &[
    Binding {
        keys: "j/k or arrows",
        action: InspectorAction::Move,
    },
    Binding {
        keys: "Enter",
        action: InspectorAction::Inspect,
    },
    Binding {
        keys: "Space",
        action: InspectorAction::Toggle,
    },
    Binding {
        keys: "I",
        action: InspectorAction::Impact,
    },
    Binding {
        keys: "P",
        action: InspectorAction::Protection,
    },
    Binding {
        keys: "R",
        action: InspectorAction::RecoveryPreview,
    },
    Binding {
        keys: "V",
        action: InspectorAction::Verification,
    },
    Binding {
        keys: "M",
        action: InspectorAction::Map,
    },
    Binding {
        keys: "T",
        action: InspectorAction::Timeline,
    },
    Binding {
        keys: "A",
        action: InspectorAction::Apply,
    },
    Binding {
        keys: "Esc",
        action: InspectorAction::Back,
    },
];

/// The binding for `action`, where the reference table has one.
#[must_use]
pub fn binding_for(action: InspectorAction) -> Option<Binding> {
    BINDINGS
        .iter()
        .copied()
        .find(|binding| binding.action() == action)
}

/// The key help Appendix E.3's bindings read as, laid out at `width` columns.
#[must_use]
pub fn key_help(width: usize) -> Vec<String> {
    let column = BINDINGS
        .iter()
        .map(|binding| display_width(binding.keys()))
        .max()
        .unwrap_or(0)
        + 3;
    BINDINGS
        .iter()
        .map(|binding| {
            fit(
                &crate::labelled(binding.keys(), binding.action().meaning(), column),
                width,
            )
        })
        .collect()
}
