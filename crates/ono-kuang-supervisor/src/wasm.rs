//! The `wasm-component` tier of spec §31.10 (issue #3 (b), ADR-0569): a package's component
//! runs inside the WebAssembly component runtime Ono embeds, and speaks the same framed protocol
//! over its WASI standard streams that a native process speaks over its pipes.
//!
//! What the tier gives by construction, because the guest is not a process: no descriptors, no
//! filesystem, no network, no environment — the WASI context is built with none of them — and a
//! memory ceiling the runtime enforces on every growth. What it does not give is a CPU ceiling:
//! a component that spins is preempted at every epoch so the host stays responsive, and is not
//! stopped; the confinement table says so as `not_provided`.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::io::DuplexStream;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, ResourceLimiter, Store};
use wasmtime_wasi::cli::{AsyncStdinStream, AsyncStdoutStream};
use wasmtime_wasi::p2::bindings::Command;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

/// How often the runtime is asked to yield, so one busy component cannot hold the thread.
const EPOCH: Duration = Duration::from_millis(10);

/// The bytes a guest stream may hold before the host has read it.
const PIPE: usize = 64 * 1024;

/// What the runtime saw of the guest's memory: the last size it grew to, and the largest.
#[derive(Debug, Default)]
pub struct Gauge {
    current: AtomicU64,
    peak: AtomicU64,
}

impl Gauge {
    fn record(&self, bytes: u64) {
        self.current.store(bytes, Ordering::Relaxed);
        self.peak.fetch_max(bytes, Ordering::Relaxed);
    }

    /// The guest's linear memory now, in bytes.
    #[must_use]
    pub fn current(&self) -> u64 {
        self.current.load(Ordering::Relaxed)
    }

    /// The most linear memory the guest has had, in bytes.
    #[must_use]
    pub fn peak(&self) -> u64 {
        self.peak.load(Ordering::Relaxed)
    }
}

/// The store's state: the WASI context, the resource table, and the ceiling.
struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    memory_max: u64,
    gauge: Arc<Gauge>,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl ResourceLimiter for State {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let desired_bytes = u64::try_from(desired).unwrap_or(u64::MAX);
        let allowed = desired_bytes <= self.memory_max && maximum.is_none_or(|max| desired <= max);
        if allowed {
            self.gauge.record(desired_bytes);
        }
        Ok(allowed)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= 1 << 20 && maximum.is_none_or(|max| desired <= max))
    }
}

/// The one engine every component shares: compiled code is cached per engine, and its
/// configuration is the tier's.
fn engine() -> Result<&'static Engine, String> {
    static ENGINE: OnceLock<Result<Engine, String>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            let mut config = Config::new();
            config.wasm_component_model(true).epoch_interruption(true);
            Engine::new(&config).map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// How a component ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exit {
    /// `run` returned: the component's own status.
    Returned {
        /// Whether it reported success.
        success: bool,
    },
    /// The runtime stopped it: a trap, a refused growth the guest could not survive, or a
    /// failure to instantiate.
    Trapped(String),
    /// The host ended it.
    Killed,
}

/// A running component: the task that drives it, the ticker that keeps it fair, and the gauge.
#[derive(Debug)]
pub struct WasmInstance {
    task: Option<tokio::task::JoinHandle<Result<Result<(), ()>, String>>>,
    ticker: tokio::task::JoinHandle<()>,
    gauge: Arc<Gauge>,
    exit: Option<Exit>,
}

impl WasmInstance {
    /// Loads the component at `entry` and starts it, with its standard input and output as the
    /// protocol streams: the host writes into the returned writer and reads from the returned
    /// reader. Nothing else is given to it.
    ///
    /// # Errors
    ///
    /// The engine's, the file's or the linker's own reason when the component cannot be loaded.
    pub fn spawn(
        entry: &Path,
        memory_max: u64,
    ) -> Result<(Self, DuplexStream, DuplexStream), String> {
        let engine = engine()?;
        let component = Component::from_file(engine, entry).map_err(|error| {
            format!(
                "`{}` is not a component this runtime can load: {error}",
                entry.display()
            )
        })?;
        let mut linker: Linker<State> = Linker::new(engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker)
            .map_err(|error| format!("the WASI host functions could not be linked: {error}"))?;

        // Host → guest: the host writes `host_in`, the guest reads its stdin from `guest_in`.
        let (host_in, guest_in) = tokio::io::duplex(PIPE);
        // Guest → host: the guest writes its stdout into `guest_out`, the host reads `host_out`.
        let (guest_out, host_out) = tokio::io::duplex(PIPE);
        let wasi = WasiCtxBuilder::new()
            .stdin(AsyncStdinStream::new(guest_in))
            .stdout(AsyncStdoutStream::new(PIPE, guest_out))
            .build();
        let gauge = Arc::new(Gauge::default());
        let mut store = Store::new(
            engine,
            State {
                wasi,
                table: ResourceTable::new(),
                memory_max,
                gauge: Arc::clone(&gauge),
            },
        );
        store.limiter(|state| state);
        store.epoch_deadline_async_yield_and_update(1);

        let ticker = {
            let engine = engine.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(EPOCH).await;
                    engine.increment_epoch();
                }
            })
        };
        let task = tokio::spawn(async move {
            let command = Command::instantiate_async(&mut store, &component, &linker)
                .await
                .map_err(|error| format!("the component could not be instantiated: {error}"))?;
            command
                .wasi_cli_run()
                .call_run(&mut store)
                .await
                .map_err(|error| error.to_string())
        });
        Ok((
            Self {
                task: Some(task),
                ticker,
                gauge,
                exit: None,
            },
            host_in,
            host_out,
        ))
    }

    /// What the runtime saw of the guest's memory.
    #[must_use]
    pub fn gauge(&self) -> &Arc<Gauge> {
        &self.gauge
    }

    /// Stops the component now. The task is aborted, which drops the store and everything in it.
    pub fn kill(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            self.exit = Some(Exit::Killed);
        }
        self.ticker.abort();
    }

    /// Waits for the component to end and says how.
    pub async fn wait(&mut self) -> Exit {
        if let Some(exit) = &self.exit {
            return exit.clone();
        }
        let exit = match self.task.take() {
            None => Exit::Killed,
            Some(task) => match task.await {
                Ok(Ok(Ok(()))) => Exit::Returned { success: true },
                Ok(Ok(Err(()))) => Exit::Returned { success: false },
                Ok(Err(trap)) => Exit::Trapped(trap),
                Err(_) => Exit::Killed,
            },
        };
        self.ticker.abort();
        self.exit = Some(exit.clone());
        exit
    }
}

impl Drop for WasmInstance {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.ticker.abort();
    }
}
