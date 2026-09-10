//! `map --plan`'s proposed-state overlay (§21.2, §21.3, §21.4, Appendix E.7).
//!
//! §21.2 makes the current world the base layer and the plan an overlay on it, so this does not
//! draw a map. It takes the map `ono-spatial-render` already drew — each line, and the node the
//! line draws — and marks the lines whose node the plan touches with §20.3's effect symbol, what
//! the plan expects to happen, and §21.3's coverage: `<->` and the asset where a recovery asset
//! covers the object, and the words `not covered` where none does, because Appendix E.8 forbids a
//! coverage display that shows only the good news.
//!
//! §21.4 is kept by construction: nothing here adds a node. A plan object the map did not draw is
//! listed under the map by the name the plan gave it, which is a fact about the plan rather than
//! a prediction about the world.

use ono_value::{MapValue, Value};

use crate::symbols::{Charset, Symbol};
use crate::{Item, fit, flag, heading, items, strings, text};

/// The drawn map with the plan in `overlay` laid over it.
///
/// `drawn` is `ono_spatial_render::map_lines`'s output as `(text, node)` pairs, and `overlay` is
/// the map the shell attaches to an `ono.spatial-map/1` for `map --plan` — `plan`, `protection`
/// and `objects`, each object with its `object`, `place` (the node id it is drawn as, or null),
/// `covered`, `expectation` and, where known, `covered_by` (the assets' references). An overlay
/// that is not a map leaves the drawing as it was.
#[must_use]
pub fn plan_overlay(
    drawn: &[(&str, Option<&str>)],
    overlay: &Value,
    width: usize,
    charset: Charset,
) -> Vec<String> {
    let Value::Map(overlay) = overlay else {
        return drawn.iter().map(|(line, _)| fit(line, width)).collect();
    };
    let overlay: &MapValue = overlay;
    let objects = items(overlay, "objects");
    let plan: String = text(overlay, "plan")
        .unwrap_or_else(|| "unknown".to_owned())
        .chars()
        .take(crate::SHORT)
        .collect();
    let protection = text(overlay, "protection")
        .unwrap_or_else(|| "unknown".to_owned())
        .to_uppercase()
        .replace('-', "_");
    let mut lines = vec![fit(
        &format!("PLAN {plan} overlay  protection {protection}"),
        width,
    )];
    let mut placed = vec![false; objects.len()];
    for (line, node) in drawn {
        let touched = node.and_then(|node| {
            objects
                .iter()
                .position(|object| text(object, "place").as_deref() == Some(node))
        });
        match touched {
            Some(index) => {
                placed[index] = true;
                lines.push(fit(
                    &format!("{}  {}", line.trim_end(), marks(&objects[index], charset)),
                    width,
                ));
            }
            None => lines.push(fit(line, width)),
        }
    }
    let elsewhere: Vec<&Item> = objects
        .iter()
        .zip(&placed)
        .filter(|(_, placed)| !**placed)
        .map(|(object, _)| object)
        .collect();
    if !elsewhere.is_empty() {
        heading(&mut lines, "not on this map");
        for object in elsewhere {
            let name = text(object, "object").unwrap_or_else(|| "unnamed".to_owned());
            lines.push(fit(&format!("  {name}  {}", marks(object, charset)), width));
        }
    }
    lines
}

/// `~ replacement expected  <-> zfs:rpool/ROOT@ono-a82f` — the effect, then the coverage.
fn marks(object: &Item, charset: Charset) -> String {
    let expectation = text(object, "expectation").unwrap_or_else(|| "unknown".to_owned());
    let symbol = match expectation.as_str() {
        "created" => Symbol::Addition,
        "removed" => Symbol::Removal,
        "modified" | "replacement expected" => Symbol::Modification,
        "interruption possible" | "outward call" => Symbol::Risk,
        _ => Symbol::Unknown,
    };
    let mut marks = format!("{} {expectation}", symbol.glyph(charset));
    if flag(object, "covered") {
        let assets = strings(object, "covered_by");
        let by = if assets.is_empty() {
            "covered".to_owned()
        } else {
            assets.join(", ")
        };
        marks.push_str(&format!(
            "  {} {by}",
            Symbol::RecoveryAvailable.glyph(charset)
        ));
    } else {
        marks.push_str("  not covered");
    }
    marks
}
