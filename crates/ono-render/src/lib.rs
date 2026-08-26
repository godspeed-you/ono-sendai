//! Presentation of Ono values.
//!
//! Spec §5 is emphatic that "the shell language MUST not depend on any specific renderer" and
//! that "providers MUST not format human tables themselves". This crate is where all terminal
//! formatting lives, and it consumes already-rendered cell text: the mapping from a value to
//! the text of a cell belongs to the value model, and the mapping from text to a laid-out
//! terminal belongs here.
//!
//! Spec §10.7 is the other half of the same rule — a table is a rendering strategy, never a
//! value. Laying a [`Table`] out never changes it, so column width, ordering and truncation
//! cannot leak into pipeline semantics.
//!
//! ```
//! use ono_render::{Cell, Column, Layout, Table};
//! let table = Table::new(vec![Column::new("NAME")]).with_row(vec![Cell::new("nginx")]);
//! let lines = Layout::new(40).render(&table);
//! assert_eq!(lines[0].trim(), "NAME");
//! assert_eq!(lines[1].trim(), "nginx");
//! ```

#![forbid(unsafe_code)]

mod presentation;
mod table;
mod theme;

pub use presentation::Presentation;
pub use table::{Align, Cell, Column, Layout, Table};
pub use theme::{Color, Style, Theme, Token, sanitise};
