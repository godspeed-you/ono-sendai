//! What a decoder made of a buffer, and the record builder every decoder shares.

use std::sync::Arc;

use jiff::Timestamp;
use ono_value::{ErrorValue, Provenance, RecordValue, Schema, Value};

/// The objects a decoder read out of a buffer, and the problems it met while reading it.
///
/// The two travel together because they are two halves of one answer (spec §16.5): a dump that
/// held ten interfaces and one message this crate could not read must report ten interfaces
/// *and* the message, never nine-and-silence or a bare failure. The pipeline sends the records
/// down the value channel and the errors down the error channel beside it.
#[derive(Debug, Default)]
pub struct Decoded {
    records: Vec<RecordValue>,
    errors: Vec<ErrorValue>,
}

impl Decoded {
    /// An empty result.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The objects that were read.
    #[must_use]
    pub fn records(&self) -> &[RecordValue] {
        &self.records
    }

    /// The problems met while reading, in the order they were met.
    #[must_use]
    pub fn errors(&self) -> &[ErrorValue] {
        &self.errors
    }

    /// The two halves, for a caller that streams them.
    #[must_use]
    pub fn into_parts(self) -> (Vec<RecordValue>, Vec<ErrorValue>) {
        (self.records, self.errors)
    }

    /// Adds an object.
    pub(crate) fn push(&mut self, record: RecordValue) {
        self.records.push(record);
    }

    /// Adds a problem.
    pub(crate) fn fail(&mut self, error: ErrorValue) {
        self.errors.push(error);
    }

    /// Takes over everything another decode produced.
    ///
    /// One `get socket` is four `sock_diag` dumps and one `get route` is two; each is decoded on
    /// its own — a dump ends with `NLMSG_DONE`, and concatenating buffers would hide everything
    /// after the first one — and the answers are joined here.
    pub(crate) fn absorb(&mut self, other: Decoded) {
        let (records, errors) = other.into_parts();
        self.records.extend(records);
        self.errors.extend(errors);
    }

    /// Builds a record of `schema` and adds it, or adds the reason it could not be built.
    pub(crate) fn record(
        &mut self,
        schema: &Arc<Schema>,
        source: &str,
        provider: &str,
        fields: Vec<(&str, Value)>,
        extras: Vec<(&str, Value)>,
    ) {
        match build(schema, source, provider, fields, extras) {
            Ok(record) => self.push(record),
            Err(error) => self.fail(error),
        }
    }
}

/// Assembles one record, with the provenance spec §25.2 requires on every observation.
pub(crate) fn build(
    schema: &Arc<Schema>,
    source: &str,
    provider: &str,
    fields: Vec<(&str, Value)>,
    extras: Vec<(&str, Value)>,
) -> Result<RecordValue, ErrorValue> {
    let provenance = Provenance::local(provider, schema.id().clone())
        .from_source(source)
        .observed_at(Timestamp::now());
    let mut builder = RecordValue::builder(Arc::clone(schema), provenance);
    for (name, value) in fields {
        builder = builder.set(name, value)?;
    }
    for (name, value) in extras {
        builder = builder.set_extra(name, value);
    }
    Ok(builder.build())
}
