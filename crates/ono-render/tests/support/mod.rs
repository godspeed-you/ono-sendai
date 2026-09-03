//! Helpers shared by the `ono-render` test suites (v0.4.1 §39.1, ADR-0427, ADR-0515).

#![allow(dead_code, reason = "not every helper is used by every test binary")]

use std::sync::Arc;

use ono_value::{MapValue, Value};

pub fn map(pairs: &[(&str, Value)]) -> Value {
    let mut map = MapValue::new();
    for (key, value) in pairs {
        map.insert((*key).into(), value.clone());
    }
    Value::Map(Arc::new(map))
}

/// Removes every ANSI escape sequence, so a painted line can be compared with a plain one.
pub fn strip(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            for escaped in chars.by_ref() {
                if escaped == 'm' {
                    break;
                }
            }
        } else {
            out.push(character);
        }
    }
    out
}
