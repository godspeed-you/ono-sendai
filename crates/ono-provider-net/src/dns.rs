//! `resolve dns`: the system resolver, asked explicitly.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{Availability, Capability, ObjectRef, Provider, Query, Risk, Selector};
use ono_value::{ErrorValue, RecordValue, Schema, Value};

use crate::resolver::{RecordType, addresses_of, name_of};
use crate::schema::{build, dns_record_id, require};

/// How long one resolution may take before the provider reports the resolver as unreachable.
///
/// The resolver has timeouts of its own (`resolv.conf`'s `timeout` times `attempts` times the
/// number of servers), and they can add up to more than an interactive shell should ever wait
/// (spec §34). The bound turns "still waiting" into a structured, retryable answer; the lookup
/// thread finishes on its own.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(15);

/// Names and addresses, through `getaddrinfo(3)` and `getnameinfo(3)`.
///
/// A query that is an address performs a reverse lookup and answers with a `PTR` record; any
/// other query answers with the `A` and `AAAA` records the resolver returns. `--type` keeps one
/// kind. `--server` is refused: the system resolver answers from `resolv.conf` and NSS, and
/// asking a particular server needs a DNS client this build does not include (ADR-0087).
#[derive(Debug, Clone, Copy, Default)]
pub struct DnsProvider;

impl DnsProvider {
    /// A provider asking this machine's resolver.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// What one `resolve dns` asks for.
#[derive(Debug, Clone)]
struct Lookup {
    query: String,
    address: Option<IpAddr>,
    wanted: Option<RecordType>,
}

impl Lookup {
    fn from_query(query: &Query) -> Result<Self, ErrorValue> {
        if query.option_value("server").is_some() {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                "`--server` asks a particular nameserver, and the system resolver answers only \
                 from resolv.conf and NSS",
            )
            .with_help(
                "leave `--server` out to ask the resolver every program on this machine uses",
            ));
        }
        let subject = query
            .selectors()
            .iter()
            .find_map(|selector| match selector {
                Selector::Field { name, value } if name == "query" => Some(value),
                _ => None,
            })
            .ok_or_else(|| missing_query())?;
        Self::of(subject, query.option_value("type"))
    }

    fn of(subject: &Value, wanted: Option<&Value>) -> Result<Self, ErrorValue> {
        let (query, address) = match subject {
            Value::Ip(address) => (address.to_string(), Some(*address)),
            Value::String(text) if !text.is_empty() => {
                (text.to_string(), text.parse::<IpAddr>().ok())
            }
            Value::String(_) => return Err(missing_query()),
            other => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!(
                        "`resolve dns` takes a name or an address, not a {}",
                        other.type_name()
                    ),
                ));
            }
        };
        let wanted = match wanted {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(RecordType::parse(text)?),
            Some(other) => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`--type` names a record type, not a {}", other.type_name()),
                ));
            }
        };
        Ok(Self {
            query,
            address,
            wanted,
        })
    }

    /// Performs the lookup on the calling thread.
    fn run(&self, schema: &Arc<Schema>) -> Result<Vec<RecordValue>, ErrorValue> {
        let mut records = Vec::new();
        if let Some(address) = self.address {
            // An address asks for its name (network.yaml: "An address performs a reverse
            // lookup"); asking it for an address record is a question with no answer.
            if self.wanted.is_some_and(|wanted| wanted != RecordType::Ptr) {
                return Ok(records);
            }
            let name = name_of(address)?;
            records.push(record(
                schema,
                "getnameinfo(3)",
                &name,
                RecordType::Ptr,
                address,
            )?);
            return Ok(records);
        }
        if self.wanted == Some(RecordType::Ptr) {
            return Ok(records);
        }
        let addresses = addresses_of(&self.query)?;
        if addresses.is_empty() {
            return Err(ErrorValue::new(
                ErrorCode::IoNotFound,
                format!("the resolver answered nothing for `{}`", self.query),
            ));
        }
        for address in addresses {
            let kind = RecordType::of(address);
            if self.wanted.is_some_and(|wanted| wanted != kind) {
                continue;
            }
            records.push(record(
                schema,
                "getaddrinfo(3)",
                &self.query,
                kind,
                address,
            )?);
        }
        Ok(records)
    }

    /// The lookup on a blocking thread, bounded by [`LOOKUP_TIMEOUT`].
    async fn run_bounded(self, schema: Arc<Schema>) -> Result<Vec<RecordValue>, ErrorValue> {
        let query = self.query.clone();
        let lookup = tokio::task::spawn_blocking(move || self.run(&schema));
        match tokio::time::timeout(LOOKUP_TIMEOUT, lookup).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(joined)) => Err(ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("the resolver thread stopped before answering for `{query}`: {joined}"),
            )),
            Err(_) => Err(ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!(
                    "the resolver did not answer for `{query}` within {}s",
                    LOOKUP_TIMEOUT.as_secs()
                ),
            )
            .with_help("no nameserver is answering; the name may still exist")
            .with_retryable(true)),
        }
    }
}

fn missing_query() -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        "`resolve dns` needs a name or an address to resolve",
    )
    .with_help("as in `resolve dns example.com` or `resolve dns 10.4.2.11`")
}

fn record(
    schema: &Arc<Schema>,
    source: &str,
    name: &str,
    kind: RecordType,
    address: IpAddr,
) -> Result<RecordValue, ErrorValue> {
    build(
        schema,
        crate::RESOLVER_PROVIDER,
        source,
        vec![
            ("name", Value::string(name)),
            ("type", Value::string(kind.as_str())),
            ("address", Value::Ip(address)),
        ],
    )
}

#[async_trait::async_trait]
impl Provider for DnsProvider {
    fn id(&self) -> &str {
        crate::RESOLVER_PROVIDER
    }

    fn targets(&self) -> &[&str] {
        &["dns"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        require(&dns_record_id()).map_or_else(|_| Vec::new(), |schema| vec![schema])
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("dns.resolve", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        // The C library is always there; whether a nameserver answers is a per-lookup matter,
        // and one lookup's failure is reported as that lookup's error, retryable.
        Availability::Available
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let schema = require(&dns_record_id())?;
        let lookup = Lookup::from_query(query)?;
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                match lookup.run_bounded(schema).await {
                    Ok(records) => {
                        for record in records {
                            if sink.send(record.into_value()).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        let _ = sink.fail(error).await;
                    }
                }
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let schema = require(&dns_record_id())?;
        let Selector::Field { name, value } = selector else {
            return Ok(Vec::new());
        };
        if name != "query" {
            return Ok(Vec::new());
        }
        let records = Lookup::of(value, None)?.run_bounded(schema).await?;
        Ok(records.iter().filter_map(ObjectRef::of).collect())
    }
}
