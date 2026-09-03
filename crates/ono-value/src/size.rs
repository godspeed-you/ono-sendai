//! The deterministic approximate retained-size estimator of spec v0.4.1 §21.2.
//!
//! Spec v0.4.1 §2.4 makes a byte bound mandatory wherever a collection is retained, queued or
//! materialized and its elements can be any size, and §65.6 names the alternative — `N` values
//! each holding an arbitrary payload — as a defect rather than a limit. Enforcing a byte bound
//! needs a number, and `Value` has no cheap exact one: it sits behind `Arc`s that other values
//! may share, and the allocator's real footprint is neither observable nor stable.
//!
//! So the shell spends every byte budget through one function, and that function is
//! *deterministic* and *approximate*, in that order.

use std::collections::HashSet;
use std::sync::Arc;

use crate::{ErrorValue, MapValue, Provenance, RecordValue, Value, ValueRef};

/// The nesting depth [`estimated_size`] descends before it stops charging what lies below.
///
/// The same figure the YAML serializer refuses beyond ([`crate::MAX_YAML_DEPTH`]), as §21.2 asks:
/// "cap recursion using the same or stricter depth rules used for serialization". A value nested
/// deeper than this cannot be serialized, so it cannot cross a boundary where its size is the
/// question.
pub const MAX_ESTIMATE_DEPTH: usize = crate::MAX_YAML_DEPTH;

/// The bookkeeping every heap allocation behind an `Arc` carries: a strong and a weak count.
const ARC_HEADER: u64 = 2 * size_of::<usize>() as u64;

/// What one value costs wherever it is stored: in a list, in a map entry, in a record field.
const VALUE_SLOT: u64 = size_of::<Value>() as u64;

/// An approximate count of the bytes `value` retains, in bytes, for the budgets of §21.
///
/// # What it counts
///
/// - the slot every value occupies, wherever it is stored — a list element, a map entry, a
///   record field — so that a million nulls is not free;
/// - the payload of every string, byte string, path and regex pattern it reaches, at its exact
///   length;
/// - list elements, map keys and entries, record fields and extras, error messages, help,
///   metadata and cause chains, and record provenance, recursively;
/// - one `Arc` header per distinct heap allocation reached;
/// - each shared allocation **once**: a list holding a hundred clones of one string costs one
///   string, because that is what it retains.
///
/// # What it deliberately does not count
///
/// - allocator overhead, alignment padding and spare `Vec` capacity — none of the three is
///   observable from safe Rust, and a figure that changed with the allocator would not be
///   deterministic;
/// - the compiled automaton behind a [`crate::RegexValue`]: its size is an implementation detail
///   of the regex engine, so the pattern text stands in for it;
/// - the [`crate::Schema`] a record is bound to. A schema is provider metadata shared by every
///   record a provider produces, not the record's payload; charging each of a million records for
///   one schema would make the estimate say more about the provider than about the data;
/// - anything nested deeper than [`MAX_ESTIMATE_DEPTH`].
///
/// The result is therefore a *logical payload* figure and does not equal allocator RSS (§21.2).
/// For a value whose payload dominates its structure it stays within a factor of two of the bytes
/// that payload really occupies.
///
/// # Determinism
///
/// The same value answers the same number on every call, in every process: nothing here reads a
/// clock, a hash seed or an address as a quantity. Pointer identity is used only to decide
/// whether an allocation has already been charged inside *this* traversal, and the total is a
/// sum, so the order allocations are met in cannot change it.
///
/// ```
/// use ono_value::{Value, estimated_size};
///
/// let text = Value::string(&"x".repeat(1 << 16));
/// let copies = Value::list((0..100).map(|_| Value::string(&"x".repeat(1 << 16))));
/// let clones = Value::list((0..100).map(|_| text.clone()));
///
/// // A hundred clones retain one string, and the estimate says so; a hundred separately
/// // allocated strings retain a hundred, and it says that too.
/// assert!(estimated_size(&clones) < 2 * estimated_size(&text));
/// assert!(estimated_size(&copies) > 50 * estimated_size(&clones));
/// ```
#[must_use]
pub fn estimated_size(value: &Value) -> u64 {
    let mut estimator = Estimator {
        seen: HashSet::new(),
        total: 0,
    };
    estimator.visit(value, 0);
    estimator.total
}

/// One traversal's running total and the allocations it has already charged.
struct Estimator {
    seen: HashSet<usize>,
    total: u64,
}

impl Estimator {
    fn charge(&mut self, bytes: u64) {
        self.total = self.total.saturating_add(bytes);
    }

    /// Whether this traversal has not yet charged the allocation at `address`.
    ///
    /// Empty payloads are never deduplicated: two distinct empty allocations may share an address
    /// because there is nothing to point at, and treating them as one would mean the second's
    /// header went uncharged.
    fn first_sighting(&mut self, address: usize, payload: usize) -> bool {
        payload == 0 || self.seen.insert(address)
    }

    /// Charges an `Arc`-backed byte payload, header included, once per traversal.
    fn charge_bytes(&mut self, address: usize, payload: usize) {
        if self.first_sighting(address, payload) {
            self.charge(ARC_HEADER + payload as u64);
        }
    }

    fn visit(&mut self, value: &Value, depth: usize) {
        self.charge(VALUE_SLOT);
        match value {
            // Scalars live entirely in the slot already charged.
            Value::Null
            | Value::Bool(_)
            | Value::Int(_)
            | Value::Float(_)
            | Value::Decimal(_)
            | Value::Timestamp(_)
            | Value::Duration(_)
            | Value::ByteSize(_)
            | Value::Percent(_)
            | Value::Uuid(_)
            | Value::Ip(_)
            | Value::IpNetwork(_)
            | Value::Port(_) => {}
            Value::String(text) => self.charge_bytes(text.as_ptr() as usize, text.len()),
            Value::Bytes(bytes) => self.charge_bytes(bytes.as_ptr() as usize, bytes.len()),
            Value::Path(path) => {
                let encoded = path.as_os_str().as_encoded_bytes();
                self.charge_bytes(encoded.as_ptr() as usize, encoded.len());
            }
            Value::Regex(regex) => {
                let pattern = regex.source();
                if self.first_sighting(Arc::as_ptr(regex) as usize, pattern.len() + 1) {
                    self.charge(ARC_HEADER + size_of::<crate::RegexValue>() as u64);
                    self.charge(pattern.len() as u64);
                }
            }
            Value::List(items) => {
                if self.first_sighting(items.as_ptr() as usize, items.len()) {
                    self.charge(ARC_HEADER);
                    if depth < MAX_ESTIMATE_DEPTH {
                        for item in items.iter() {
                            self.visit(item, depth + 1);
                        }
                    }
                }
            }
            Value::Map(map) => {
                if self.first_sighting(Arc::as_ptr(map) as usize, map.len() + 1) {
                    self.charge(ARC_HEADER + size_of::<MapValue>() as u64);
                    self.visit_map(map, depth);
                }
            }
            Value::Record(record) => {
                if self.first_sighting(Arc::as_ptr(record) as usize, 1) {
                    self.charge(ARC_HEADER + size_of::<RecordValue>() as u64);
                    self.visit_record(record, depth);
                }
            }
            Value::Error(error) => {
                if self.first_sighting(Arc::as_ptr(error) as usize, 1) {
                    self.charge(ARC_HEADER + size_of::<ErrorValue>() as u64);
                    self.visit_error(error, depth);
                }
            }
        }
    }

    fn visit_map(&mut self, map: &MapValue, depth: usize) {
        if depth >= MAX_ESTIMATE_DEPTH {
            return;
        }
        for (key, value) in map.iter() {
            self.charge(size_of::<Arc<str>>() as u64);
            self.charge_bytes(key.as_ptr() as usize, key.len());
            self.visit(value, depth + 1);
        }
    }

    fn visit_record(&mut self, record: &RecordValue, depth: usize) {
        self.visit_provenance(record.provenance());
        if depth >= MAX_ESTIMATE_DEPTH {
            return;
        }
        for index in 0..record.schema().field_count() {
            if let Some(field) = record.field_at(index) {
                self.visit(field, depth + 1);
            }
        }
        self.visit_map(record.extra(), depth);
    }

    /// A record's provenance is retained with it, so its strings are its bytes (spec §25.2).
    ///
    /// The [`crate::SchemaId`] is left out for the same reason the schema itself is: it names a
    /// contract every record of that provider shares.
    fn visit_provenance(&mut self, provenance: &Provenance) {
        self.charge(provenance.provider().len() as u64);
        self.charge(provenance.source().map_or(0, str::len) as u64);
        if let crate::Link::Remote(host) = provenance.link() {
            self.charge(host.len() as u64);
        }
        if let Some(trace) = provenance.adapter() {
            self.charge(size_of::<crate::AdapterTrace>() as u64);
            self.charge(trace.adapter().len() as u64);
            self.charge(trace.adapter_version().len() as u64);
            self.charge(trace.executable().as_os_str().len() as u64);
            self.charge(trace.executable_version().map_or(0, str::len) as u64);
            self.charge(trace.user_invocation().len() as u64);
            self.charge(trace.actual_invocation().len() as u64);
            self.charge(trace.decoder().len() as u64);
            self.charge(trace.stability().len() as u64);
            for (field, exactness) in trace.exactness() {
                self.charge((field.len() + exactness.len()) as u64);
            }
            for limit in trace.limits() {
                self.charge(limit.len() as u64);
            }
        }
    }

    fn visit_error(&mut self, error: &ErrorValue, depth: usize) {
        self.charge(error.message().len() as u64);
        self.charge(error.help().map_or(0, str::len) as u64);
        if let Some(target) = error.target() {
            self.visit_value_ref(target, depth);
        }
        if depth >= MAX_ESTIMATE_DEPTH {
            return;
        }
        self.visit_map(error.metadata(), depth);
        // The cause is reached through a borrow rather than its `Arc`, so a shared cause is
        // charged once per error that names it. A cause chain is short and its strings are short;
        // over-charging here is the safe direction (§21.2).
        if let Some(cause) = error.cause() {
            self.charge(ARC_HEADER + size_of::<ErrorValue>() as u64);
            self.visit_error(cause, depth + 1);
        }
    }

    fn visit_value_ref(&mut self, target: &ValueRef, depth: usize) {
        match target {
            ValueRef::Path(path) => {
                let encoded = path.as_os_str().as_encoded_bytes();
                self.charge_bytes(encoded.as_ptr() as usize, encoded.len());
            }
            ValueRef::Name(name) => self.charge_bytes(name.as_ptr() as usize, name.len()),
            ValueRef::Object { identity, .. } => {
                if self.first_sighting(Arc::as_ptr(identity) as usize, identity.len() + 1) {
                    self.charge(ARC_HEADER + size_of::<MapValue>() as u64);
                    self.visit_map(identity, depth);
                }
            }
        }
    }
}
