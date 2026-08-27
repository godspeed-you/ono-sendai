//! `test port`: the shell connects, times the attempt, and reports what it found.

use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{Availability, Capability, ObjectRef, Provider, Query, Risk, Selector};
use ono_value::{ErrorValue, RecordValue, Schema, Value};

use crate::resolver::addresses_of;
use crate::schema::{build, probe_result_id, require};

/// How long a probe waits when `--timeout` is not given.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Reachability probes over TCP and UDP.
///
/// A probe's finding is data (ADR-0087 §3): a refused port answers with `reachable: false` and
/// the operating system's reason, a silent one with `reachable: null`, and the run succeeds
/// either way. Only a probe that could not be attempted — a host the resolver cannot map, a
/// socket the kernel would not open — fails the stream.
#[derive(Debug, Clone, Copy, Default)]
pub struct PortProvider;

impl PortProvider {
    /// A provider that probes from this machine.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// The transports a probe can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transport {
    Tcp,
    Udp,
}

impl Transport {
    fn parse(text: &str) -> Result<Self, ErrorValue> {
        match text.to_ascii_lowercase().as_str() {
            "tcp" => Ok(Transport::Tcp),
            "udp" => Ok(Transport::Udp),
            other => Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{other}` is not a transport `test port` can probe"),
            )
            .with_help("`--protocol` takes tcp or udp")),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Transport::Tcp => "tcp",
            Transport::Udp => "udp",
        }
    }
}

/// What one `test port` asks for.
#[derive(Debug, Clone)]
struct Probe {
    host: String,
    port: u16,
    transport: Transport,
    timeout: Duration,
}

/// What the attempt found.
#[derive(Debug)]
struct Finding {
    reachable: Option<bool>,
    elapsed: Duration,
    error: Option<String>,
}

impl Probe {
    fn from_query(query: &Query) -> Result<Self, ErrorValue> {
        let mut host = None;
        let mut port = None;
        for selector in query.selectors() {
            if let Selector::Field { name, value } = selector {
                match name.as_str() {
                    "host" => host = Some(host_text(value)?),
                    "port" => port = Some(port_number(value)?),
                    _ => {}
                }
            }
        }
        let (Some(host), Some(port)) = (host, port) else {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                "`test port` needs a host and a port to probe",
            )
            .with_help("as in `test port 10.4.2.11 5432 --timeout 2s`"));
        };
        let transport = match query.option_value("protocol") {
            None | Some(Value::Null) => Transport::Tcp,
            Some(Value::String(text)) => Transport::parse(text)?,
            Some(other) => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!(
                        "`--protocol` names a transport, not a {}",
                        other.type_name()
                    ),
                ));
            }
        };
        let timeout = match query.option_value("timeout") {
            None | Some(Value::Null) => DEFAULT_TIMEOUT,
            Some(Value::Duration(duration)) => {
                let nanoseconds = u64::try_from(duration.nanoseconds().max(0)).unwrap_or(u64::MAX);
                Duration::from_nanos(nanoseconds)
            }
            Some(other) => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`--timeout` is a duration, not a {}", other.type_name()),
                ));
            }
        };
        Ok(Self {
            host,
            port,
            transport,
            timeout,
        })
    }

    /// The address to probe: the host as an address, or the first the resolver answers with.
    fn address(&self) -> Result<SocketAddr, ErrorValue> {
        let ip = match self.host.parse::<IpAddr>() {
            Ok(ip) => ip,
            Err(_) => addresses_of(&self.host)?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    ErrorValue::new(
                        ErrorCode::IoNotFound,
                        format!("the resolver answered nothing for `{}`", self.host),
                    )
                })?,
        };
        Ok(SocketAddr::new(ip, self.port))
    }

    /// Makes the attempt on the calling thread.
    fn run(&self) -> Result<Finding, ErrorValue> {
        let address = self.address()?;
        let started = Instant::now();
        let outcome = match self.transport {
            Transport::Tcp => TcpStream::connect_timeout(&address, self.timeout).map(|_| ()),
            Transport::Udp => probe_udp(address, self.timeout),
        };
        let elapsed = started.elapsed();
        Ok(match outcome {
            Ok(()) => Finding {
                reachable: Some(true),
                elapsed,
                error: None,
            },
            Err(error) => {
                let silent = matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                );
                Finding {
                    // Silence is not a refusal: nothing answered, and whether anything is there
                    // is unknown (spec §10.5).
                    reachable: if silent { None } else { Some(false) },
                    elapsed,
                    error: Some(if silent {
                        format!(
                            "nothing answered within {}",
                            ono_value::Duration::from_nanoseconds(
                                i128::try_from(self.timeout.as_nanos()).unwrap_or(i128::MAX)
                            )
                        )
                    } else {
                        error.to_string()
                    }),
                }
            }
        })
    }

    fn record(&self, schema: &Arc<Schema>, finding: &Finding) -> Result<RecordValue, ErrorValue> {
        build(
            schema,
            crate::PROBE_PROVIDER,
            &format!("connect({}:{})", self.transport.as_str(), self.port),
            vec![
                ("host", Value::string(&self.host)),
                ("port", Value::Port(self.port)),
                ("protocol", Value::string(self.transport.as_str())),
                (
                    "reachable",
                    finding.reachable.map_or(Value::Null, Value::Bool),
                ),
                (
                    "duration",
                    Value::Duration(ono_value::Duration::from_nanoseconds(
                        i128::try_from(finding.elapsed.as_nanos()).unwrap_or(i128::MAX),
                    )),
                ),
                (
                    "error",
                    finding.error.as_deref().map_or(Value::Null, Value::string),
                ),
            ],
        )
    }
}

/// A datagram to the port, and a bounded wait for whatever comes back.
///
/// UDP has no handshake, so the only positive answer is data from the peer, and the only
/// negative one is the ICMP port-unreachable the kernel turns into `ECONNREFUSED` on a
/// connected socket. Anything else is silence.
fn probe_udp(address: SocketAddr, timeout: Duration) -> std::io::Result<()> {
    let local: SocketAddr = if address.is_ipv4() {
        "0.0.0.0:0".parse().map_err(std::io::Error::other)?
    } else {
        "[::]:0".parse().map_err(std::io::Error::other)?
    };
    let socket = UdpSocket::bind(local)?;
    socket.connect(address)?;
    socket.set_read_timeout(Some(timeout))?;
    socket.send(&[])?;
    let mut buffer = [0u8; 1];
    socket.recv(&mut buffer).map(|_| ())
}

fn host_text(value: &Value) -> Result<String, ErrorValue> {
    match value {
        Value::String(text) if !text.is_empty() => Ok(text.to_string()),
        Value::Ip(address) => Ok(address.to_string()),
        other => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!(
                "`test port` takes a host name or address, not {}",
                if matches!(other, Value::String(_)) {
                    "an empty name".to_owned()
                } else {
                    format!("a {}", other.type_name())
                }
            ),
        )),
    }
}

fn port_number(value: &Value) -> Result<u16, ErrorValue> {
    match value {
        Value::Port(port) => Ok(*port),
        Value::Int(number) => u16::try_from(*number).map_err(|_| {
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("{number} is not a port: ports run from 0 to 65535"),
            )
        }),
        other => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!(
                "`test port` takes a port number, not a {}",
                other.type_name()
            ),
        )),
    }
}

/// The probe on a blocking thread; it is bounded by its own timeout plus the resolver's.
async fn run_blocking(probe: Probe, schema: Arc<Schema>) -> Result<RecordValue, ErrorValue> {
    let attempt = tokio::task::spawn_blocking(move || {
        let finding = probe.run()?;
        probe.record(&schema, &finding)
    });
    match attempt.await {
        Ok(outcome) => outcome,
        Err(joined) => Err(ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            format!("the probe thread stopped before answering: {joined}"),
        )),
    }
}

#[async_trait::async_trait]
impl Provider for PortProvider {
    fn id(&self) -> &str {
        crate::PROBE_PROVIDER
    }

    fn targets(&self) -> &[&str] {
        &["port"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        require(&probe_result_id()).map_or_else(|_| Vec::new(), |schema| vec![schema])
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("port.probe", Risk::Observe)]
    }

    fn availability(&self) -> Availability {
        Availability::Available
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let schema = require(&probe_result_id())?;
        let probe = Probe::from_query(query)?;
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                match run_blocking(probe, schema).await {
                    Ok(record) => {
                        let _ = sink.send(record.into_value()).await;
                    }
                    Err(error) => {
                        let _ = sink.fail(error).await;
                    }
                }
            },
        ))
    }

    async fn resolve(&self, _selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        // A probe result is a measurement, not an object: it declares no identity, so there
        // is nothing to resolve to (spec §27.3).
        Ok(Vec::new())
    }
}
