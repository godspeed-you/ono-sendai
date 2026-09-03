//! Views and lenses (spec §31.27, §31.28; ADR-0572): the package submits trees, the host draws
//! them and owns every byte on the terminal, and the package drives the view by events.
//!
//! The supervisor holds the lifecycle — the view table, the `ui.view` check on every call, the
//! validation of every tree, the events forwarded to the package — and what draws is a
//! [`ViewHost`] the loader is handed: the shell's runs a terminal, the test host's records.

use serde_json::Value as Json;
use tokio::sync::mpsc;

pub use ono_kuang_protocol::{VIEW_COMPONENTS, ViewContribution, ViewEvent, ViewSize};

/// A view the host has taken: where its trees go, and how to end it.
pub trait MountedView: Send + Sync {
    /// The terminal's size at mount.
    fn size(&self) -> ViewSize;
    /// Draws `tree`, already validated and the host's to sanitise.
    ///
    /// # Errors
    ///
    /// When the tree cannot be laid out; the view is torn down.
    fn submit(&self, tree: &Json) -> Result<(), String>;
    /// Tears the view down and restores the terminal.
    fn close(&self);
}

/// What takes a view: a terminal, or a recorder under test.
pub trait ViewHost: Send + Sync + std::fmt::Debug {
    /// Opens `view` for `package`, sending its events into `events`. `None` when nothing can
    /// take a view — output is redirected — and the package falls back (spec §31.28).
    ///
    /// # Errors
    ///
    /// When a terminal exists and refuses.
    fn open(
        &self,
        package: &str,
        view: &ViewContribution,
        events: mpsc::Sender<ViewEvent>,
    ) -> Result<Option<Box<dyn MountedView>>, String>;
}

/// The host of a session with no terminal to give: every view falls back.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoViews;

impl ViewHost for NoViews {
    fn open(
        &self,
        _package: &str,
        _view: &ViewContribution,
        _events: mpsc::Sender<ViewEvent>,
    ) -> Result<Option<Box<dyn MountedView>>, String> {
        Ok(None)
    }
}

/// How deep a tree may nest: a screen shows nothing deeper, and a deeper one is a defect.
pub const MAX_DEPTH: usize = 8;

/// Checks a tree against the component list and the nesting limit (spec §31.27).
///
/// # Errors
///
/// What is wrong with it, for `view.protocol_error`.
pub fn validate_tree(tree: &Json, depth: usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("the tree nests deeper than {MAX_DEPTH} levels"));
    }
    let Some(object) = tree.as_object() else {
        return Err("a node is not an object".to_owned());
    };
    let Some(component) = object.get("component").and_then(Json::as_str) else {
        return Err("a node names no `component`".to_owned());
    };
    if !VIEW_COMPONENTS.contains(&component) {
        return Err(format!(
            "`{component}` is not a view component; the components are {}",
            VIEW_COMPONENTS.join(", ")
        ));
    }
    match component {
        "Tabs" => {
            for tab in object
                .get("tabs")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(body) = tab.get("body") {
                    validate_tree(body, depth + 1)?;
                }
            }
        }
        "Split" => {
            let panes = object
                .get("panes")
                .and_then(Json::as_array)
                .ok_or_else(|| "a `Split` needs `panes`".to_owned())?;
            if panes.is_empty() || panes.len() > 4 {
                return Err("a `Split` holds one to four panes".to_owned());
            }
            for pane in panes {
                validate_tree(pane, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
