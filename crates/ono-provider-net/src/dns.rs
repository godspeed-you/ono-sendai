//! `resolve dns`: the system resolver, asked explicitly.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{Availability, Capability, ObjectRef, Provider, Query, Risk, Selector};
use ono_value::{ErrorValue, RecordValue, Schema, Value};

use crate::nameserver;
use crate::resolver::{RecordType, addresses_of, name_of};
use crate::schema::{build, dns_record_id, require};

/// How long one resolution may take before the provider reports the resolver as unreachable.
///
/// The resolver has timeouts of its own (`resolv.conf`'s `timeout` times `attempts` times the
/// number of servers), and they can add up to more than an interactive shell should ever wait
/// (spec §34). The bound turns "still waiting" into a structured, retryable answer; the lookup
/// thread finishes on its own.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(15);

/// Where a nameserver listens unless `--port` says otherwise.
const DNS_PORT: u16 = 53;

/// Names and addresses, through `getaddrinfo(3)` and `getnameinfo(3)` — or through one named
/// nameserver.
///
/// A query that is an address performs a reverse lookup and answers with a `PTR` record; any
/// other query answers with the `A` and `AAAA` records the resolver returns. `--type` keeps one
/// kind. `--server` asks that nameserver directly instead, over the crate's own DNS client
/// (ADR-0240): what this host resolves and what a given server says are different questions,
/// and `getaddrinfo(3)` can only answer the first.
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
    /// The nameserver to ask instead of the system resolver (`--server`).
    server: Option<IpAddr>,
    /// The port that nameserver listens on (`--port`, 53 unless given).
    port: u16,
}

impl Lookup {
    fn from_query(query: &Query) -> Result<Self, ErrorValue> {
        let server = match query.option_value("server") {
            None | Some(Value::Null) => None,
            Some(Value::Ip(address)) => Some(*address),
            Some(Value::String(text)) => Some(text.parse::<IpAddr>().map_err(|_| {
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`--server` names a nameserver by address, and `{text}` is not one"),
                )
            })?),
            Some(other) => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`--server` is an address, not a {}", other.type_name()),
                ));
            }
        };
        let port = match query.option_value("port") {
            None | Some(Value::Null) => DNS_PORT,
            Some(Value::Port(port)) => *port,
            Some(Value::Int(port)) => u16::try_from(*port).map_err(|_| {
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`--port` is a port number, and {port} is not one"),
                )
            })?,
            Some(other) => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`--port` is a port, not a {}", other.type_name()),
                ));
            }
        };
        if server.is_none() && query.option_value("port").is_some() {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                "`--port` says where the nameserver listens, and no nameserver was named",
            )
            .with_help("write `--server <address> --port <port>`, or leave both out"));
        }
        let subject = query
            .selectors()
            .iter()
            .find_map(|selector| match selector {
                Selector::Field { name, value } if name == "query" => Some(value),
                _ => None,
            })
            .ok_or_else(missing_query)?;
        let mut lookup = Self::of(subject, query.option_value("type"))?;
        lookup.server = server;
        lookup.port = port;
        Ok(lookup)
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
            server: None,
            port: DNS_PORT,
        })
    }

    /// The lookup as one named nameserver answers it (ADR-0240).
    fn ask_server(
        &self,
        server: IpAddr,
        schema: &Arc<Schema>,
    ) -> Result<Vec<RecordValue>, ErrorValue> {
        let source = format!("{server}:{}", self.port);
        let mut records = Vec::new();
        if let Some(address) = self.address {
            if self.wanted.is_some_and(|wanted| wanted != RecordType::Ptr) {
                return Ok(records);
            }
            let question = nameserver::reverse_name(address);
            for answer in nameserver::ask(server, self.port, &question, RecordType::Ptr)? {
                let Some(name) = answer.target else { continue };
                records.push(record(schema, &source, &name, RecordType::Ptr, address)?);
            }
            return Ok(records);
        }
        if self.wanted == Some(RecordType::Ptr) {
            return Ok(records);
        }
        // Without `--type` a name is asked for both address families, as the system resolver
        // asks for both. One family answering nothing is not a failure while the other does.
        let kinds: Vec<RecordType> = match self.wanted {
            Some(kind) => vec![kind],
            None => vec![RecordType::A, RecordType::Aaaa],
        };
        let mut refusal: Option<ErrorValue> = None;
        for kind in kinds {
            match nameserver::ask(server, self.port, &self.query, kind) {
                Ok(answers) => {
                    for answer in answers {
                        let Some(address) = answer.address else {
                            continue;
                        };
                        records.push(record(schema, &source, &self.query, kind, address)?);
                    }
                }
                Err(error) => refusal = Some(error),
            }
        }
        match refusal {
            // Every question this lookup asked was refused, and the refusal is the answer.
            Some(error) if records.is_empty() => Err(error),
            _ => Ok(records),
        }
    }

    /// Performs the lookup on the calling thread.
    fn run(&self, schema: &Arc<Schema>) -> Result<Vec<RecordValue>, ErrorValue> {
        if let Some(server) = self.server {
            return self.ask_server(server, schema);
        }
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
