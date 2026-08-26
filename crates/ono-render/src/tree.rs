//! The ASCII tree of spec §22.4.
//!
//! Everything with a shape renders through this: a process tree, a `trace` result, a nested
//! record. It is deliberately ASCII, because spec §22.4 asks for a renderer that "should produce
//! useful output everywhere" — over a serial console, in a CI log, through `less`.

use unicode_width::UnicodeWidthStr;

use crate::Token;
use crate::table::shorten;

/// How sure the provider is that a relationship exists.
///
/// Spec §22.2: "The UI must not visually imply certainty that the provider does not possess."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Confidence {
    /// Observed directly, at observation time.
    #[default]
    Exact,
    /// Derived from evidence that does not prove it.
    Inferred,
}

/// A node and the relationship that reached it.
#[derive(Debug, Clone)]
pub struct TreeNode {
    label: String,
    key: Option<String>,
    token: Token,
    relation: Option<String>,
    confidence: Confidence,
    children: Vec<TreeNode>,
}

impl TreeNode {
    /// A node showing `label`, with no relationship and no children.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            key: None,
            token: Token::Foreground,
            relation: None,
            confidence: Confidence::Exact,
            children: Vec::new(),
        }
    }

    /// Names the field or key this node stands for, drawn before the label as `key: label`.
    #[must_use]
    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Paints the node's label with a semantic token (spec §44).
    #[must_use]
    pub fn with_token(mut self, token: Token) -> Self {
        self.token = token;
        self
    }

    /// The field or key this node stands for, if it has one.
    #[must_use]
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// The token the node's label is painted with.
    #[must_use]
    pub fn token(&self) -> Token {
        self.token
    }

    /// Names the relationship that leads from the parent to this node.
    #[must_use]
    pub fn relation(mut self, relation: impl Into<String>) -> Self {
        self.relation = Some(relation.into());
        self
    }

    /// Records how sure the provider is of that relationship.
    #[must_use]
    pub fn confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = confidence;
        self
    }

    /// Appends a child, in builder style.
    #[must_use]
    pub fn with_child(mut self, child: TreeNode) -> Self {
        self.children.push(child);
        self
    }

    /// Appends a child.
    pub fn push_child(&mut self, child: TreeNode) {
        self.children.push(child);
    }

    /// The node's label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The relationship that reaches this node, if it has one.
    #[must_use]
    pub fn relationship(&self) -> Option<&str> {
        self.relation.as_deref()
    }

    /// How sure the provider is of that relationship.
    #[must_use]
    pub fn certainty(&self) -> Confidence {
        self.confidence
    }

    /// The node's children.
    #[must_use]
    pub fn children(&self) -> &[TreeNode] {
        &self.children
    }
}

/// One drawn line, split into the parts a theme paints differently.
pub(crate) struct TreeLine {
    /// The connectors leading to this node. Painted as a border.
    pub indent: String,
    /// The field or key this node stands for, if it has one.
    pub key: Option<String>,
    /// The node's own text.
    pub label: String,
    /// The token the label is painted with.
    pub token: Token,
}

impl TreeLine {
    /// The whole line, before any shortening.
    fn text(&self) -> String {
        match &self.key {
            Some(key) => format!("{}{key}: {}", self.indent, self.label),
            None => format!("{}{}", self.indent, self.label),
        }
    }
}

/// Renders `root` into terminal lines, no wider than `width`.
pub(crate) fn render(root: &TreeNode, width: usize, max_depth: Option<usize>) -> Vec<String> {
    lines(root, max_depth)
        .iter()
        .map(|line| shorten(&line.text(), width))
        .collect()
}

/// The drawn lines of `root`, before they are fitted to a width.
pub(crate) fn lines(root: &TreeNode, max_depth: Option<usize>) -> Vec<TreeLine> {
    let mut lines = vec![TreeLine {
        indent: String::new(),
        key: root.key.clone(),
        label: root.label.clone(),
        token: root.token,
    }];
    draw_children(root, "", max_depth, 1, &mut lines);
    lines
}

/// Draws a node's children beneath `prefix`.
///
/// `+--` and `|` are used rather than the box-drawing characters because the tree has to be
/// legible on a terminal that has never heard of Unicode, and because spec §22.4 draws it this
/// way.
fn draw_children(
    node: &TreeNode,
    prefix: &str,
    max_depth: Option<usize>,
    depth: usize,
    lines: &mut Vec<TreeLine>,
) {
    if node.children.is_empty() {
        return;
    }

    if max_depth.is_some_and(|limit| depth > limit) {
        let hidden = count_descendants(node);
        lines.push(TreeLine {
            indent: format!("{prefix}+-- "),
            key: None,
            label: format!("... {hidden} more"),
            token: Token::Dim,
        });
        return;
    }

    for child in &node.children {
        let mark = match child.confidence {
            Confidence::Exact => "+--",
            // The marker is textual so it survives a pipe, a monochrome terminal and a reader
            // who cannot distinguish the theme's colours (spec §44).
            Confidence::Inferred => "+~~",
        };
        let arrow = match &child.relation {
            Some(relation) => format!(" {relation} ->"),
            None => String::new(),
        };
        lines.push(TreeLine {
            indent: format!("{prefix}{mark}{arrow} "),
            key: child.key.clone(),
            label: child.label.clone(),
            token: child.token,
        });

        // Every level indents by the same four cells the connector occupies, so a child sits
        // under its parent's label rather than under the connector.
        let child_prefix = format!("{prefix}|   ");
        draw_children(child, &child_prefix, max_depth, depth + 1, lines);
    }
}

fn count_descendants(node: &TreeNode) -> usize {
    node.children.len() + node.children.iter().map(count_descendants).sum::<usize>()
}

/// The width the tree would like, so a caller can decide whether it fits.
#[must_use]
pub fn natural_width(root: &TreeNode) -> usize {
    render(root, usize::MAX, None)
        .iter()
        .map(|line| line.width())
        .max()
        .unwrap_or(0)
}
