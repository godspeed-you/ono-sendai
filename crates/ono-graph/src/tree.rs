//! Drawing a graph as the ASCII tree of spec §22.4.
//!
//! A graph becomes a tree by walking from its roots and marking an object that has already been
//! drawn instead of descending into it again. Nothing is drawn twice, nothing is invented, and a
//! cycle is a line rather than a hang.
//!
//! There is no second renderer here: the shape is [`ono_render::TreeNode`], and the terminal
//! work — width, colour, truncation — belongs to `ono-render` alone (spec §5).

use std::collections::HashSet;

use ono_provider_api::ObjectId;
use ono_render::{Token, TreeNode};

use crate::graph::{Graph, Node};

impl Graph {
    /// The graph as one tree per root, ready for [`ono_render::Layout::render_tree`].
    ///
    /// The roots are the objects nothing points at — the trace's own starting points, in the
    /// order they were reached. A graph in which everything is pointed at (a cycle traced from
    /// inside it) is drawn from the object the trace started at.
    ///
    /// ```
    /// use ono_graph::Graph;
    /// assert!(Graph::new().trees().is_empty());
    /// ```
    #[must_use]
    #[allow(
        clippy::mutable_key_type,
        reason = "an ObjectId hashes on its schema and on its identity values rendered once at \
                  construction, never on the `Value`s themselves; the interior mutability the \
                  lint sees is a regex cache inside `Value`, which no key here can reach and \
                  which cannot change a hash"
    )]
    pub fn trees(&self) -> Vec<TreeNode> {
        let mut drawn: HashSet<ObjectId> = HashSet::new();
        let mut trees: Vec<TreeNode> = self
            .roots()
            .into_iter()
            .map(|id| self.draw(&id, &mut drawn))
            .collect();

        // A walk that stopped early says so under the first thing it drew, where a reader who
        // is counting children will look. Silence would make a bounded answer look complete.
        if let Some(message) = self.truncation().message()
            && let Some(first) = trees.first_mut()
        {
            first.push_child(TreeNode::new(format!("... {message}")).with_token(Token::Dim));
        }
        trees
    }

    /// The objects a drawing starts from.
    fn roots(&self) -> Vec<ObjectId> {
        let unreached: Vec<ObjectId> = self
            .nodes()
            .iter()
            .map(Node::id)
            .filter(|id| !self.edges().iter().any(|edge| edge.to() == *id))
            .cloned()
            .collect();
        if !unreached.is_empty() {
            return unreached;
        }
        // Everything is pointed at, so the drawing starts where the trace did.
        self.root()
            .cloned()
            .or_else(|| self.nodes().first().map(|node| node.id().clone()))
            .into_iter()
            .collect()
    }

    /// One object and everything below it.
    #[allow(
        clippy::mutable_key_type,
        reason = "see `Graph::trees`: an ObjectId's hash is fixed at construction"
    )]
    fn draw(&self, id: &ObjectId, drawn: &mut HashSet<ObjectId>) -> TreeNode {
        let label = self
            .node(id)
            .map_or_else(|| id.to_string(), |node| node.label().to_owned());
        if !drawn.insert(id.clone()) {
            // The object is elsewhere in the drawing. Repeating its subtree would claim there are
            // two of it, which is exactly what keying nodes by identity exists to prevent.
            return TreeNode::new(format!("{label} (already shown)")).with_token(Token::Dim);
        }

        let mut node = TreeNode::new(label);
        for edge in self.edges_from(id) {
            let child = self
                .draw(edge.to(), drawn)
                .relation(edge.relation())
                .confidence(edge.confidence());
            node.push_child(child);
        }
        for failure in self.failures_of(id) {
            node.push_child(
                TreeNode::new(format!(
                    "! {}: {}",
                    failure.error().code().name(),
                    failure.error().message()
                ))
                .with_token(Token::ErrorCode),
            );
        }
        node
    }
}
