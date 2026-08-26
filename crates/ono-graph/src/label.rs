//! What a node is called in a drawing.
//!
//! Spec §22.4 and §33.1 draw objects as `nginx.service`, `process/921 nginx`, `tcp/:443` and
//! `/etc/nginx/nginx.conf` — one short form per kind of object, not one generic form for all of
//! them. The label is therefore chosen per schema here, once, rather than by each renderer.

use ono_value::{RecordValue, Value, canonical_text};

/// The text spec §22.4 draws for `record`.
pub(crate) fn label_of(record: &RecordValue) -> String {
    match record.schema_id().name() {
        "ono.process" => {
            let name = text(record, "name").unwrap_or_else(|| "process".to_owned());
            match text(record, "pid") {
                Some(pid) => format!("process/{pid} {name}"),
                None => name,
            }
        }
        // A unit's name already identifies it and already carries its type as a suffix, so
        // prefixing it with the schema would only repeat what `nginx.service` says.
        "ono.service" | "ono.user" | "ono.group" | "ono.interface" => {
            text(record, "name").unwrap_or_else(|| generic(record))
        }
        "ono.socket" | "ono.connection" => {
            let protocol = text(record, "protocol").unwrap_or_else(|| "socket".to_owned());
            match record.get("local").map(endpoint_text) {
                Some(local) if !local.is_empty() => format!("{protocol}/{local}"),
                _ => generic(record),
            }
        }
        "ono.endpoint" => endpoint_label(record),
        "ono.file" | "ono.dir" => text(record, "path").unwrap_or_else(|| generic(record)),
        "ono.mount" => text(record, "target").unwrap_or_else(|| generic(record)),
        _ => generic(record),
    }
}

/// The label for an object whose schema this crate has no special form for: the schema's short
/// name and the first identity value, which is always enough to tell two of them apart.
fn generic(record: &RecordValue) -> String {
    let short = record
        .schema_id()
        .name()
        .strip_prefix("ono.")
        .unwrap_or_else(|| record.schema_id().name())
        .to_owned();
    let identity = record
        .schema()
        .identity()
        .iter()
        .find(|field| &***field != "provider")
        .and_then(|field| record.get(field))
        .and_then(|value| canonical_text(value).ok());
    match identity {
        Some(identity) => format!("{short}/{identity}"),
        None => short,
    }
}

/// `10.4.2.11:5432`, `:443` for a wildcard bind, or the path of a Unix socket.
///
/// A wildcard address renders as nothing at all, because spec §22.4 draws a listening socket as
/// `tcp/:443`: what matters about it is the port, and `0.0.0.0` is the absence of a restriction
/// rather than an address anything is reachable at.
pub(crate) fn endpoint_text(value: &Value) -> String {
    value.as_record().map(endpoint_label).unwrap_or_default()
}

/// The same, for an endpoint already in hand as a record.
pub(crate) fn endpoint_label(endpoint: &RecordValue) -> String {
    if let Some(path) = text(endpoint, "path") {
        return path;
    }
    let address = match endpoint.get("address") {
        Some(Value::Ip(address)) if address.is_unspecified() => String::new(),
        Some(Value::Ip(address)) => address.to_string(),
        _ => String::new(),
    };
    let host = text(endpoint, "host");
    let port = text(endpoint, "port");
    let shown = host.unwrap_or(address);
    match port {
        Some(port) => format!("{shown}:{port}"),
        None => shown,
    }
}

/// A field as its canonical text, or `None` when it is unknown or unreadable.
fn text(record: &RecordValue, field: &str) -> Option<String> {
    match record.get(field) {
        None | Some(Value::Null) | Some(Value::Error(_)) => None,
        Some(value) => canonical_text(value).ok(),
    }
}
