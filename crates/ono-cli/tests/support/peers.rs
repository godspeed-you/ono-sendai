//! Real local peers: `ono --agent --listen` processes, each with its own scratch home and so its
//! own identity, trust store and authorization store (v0.4 §43.7; v0.4.1 §7.1, §9).
//!
//! Every peer listens on its own loopback address (`127.0.0.2`, `127.0.0.3`, …). A host key is
//! pinned under the host, not under `host:port` (ADR-0353), so two peers on one address would be
//! one host presenting two keys — exactly what a pin refuses. Distinct loopback addresses are
//! distinct hosts to the trust store, and the kernel routes all of `127.0.0.0/8` to `lo`, so no
//! network and no privilege is needed.

#![allow(
    dead_code,
    clippy::expect_used,
    clippy::panic,
    reason = "a shared test fixture states preconditions the same way a #[test] body does"
)]

use std::io::{BufRead as _, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use ono_testkit::{Scratch, scratch};

use super::{binary, client_fingerprint, ono_at_home};

/// How long a peer may take to say where it listens.
const STARTUP: Duration = Duration::from_secs(30);

/// One listening agent with a home of its own.
pub struct Peer {
    /// The peer's home: its identity, its trust store, its authorization store.
    pub home: Scratch,
    /// `127.0.0.N:port`, as `link host … --transport tcp` takes it.
    pub address: String,
    /// The loopback address alone — the host a client pins the peer under.
    pub host: String,
    /// The fingerprint of the key the peer proves it holds; also its client identity when it
    /// links onward.
    pub fingerprint: String,
    process: Option<ono_testkit::OwnedChild>,
    log: Arc<Mutex<String>>,
    _lines: mpsc::Receiver<String>,
}

impl Peer {
    /// Starts an agent listening on `host` (a loopback address) with a fresh home, passing
    /// `extra` after `--listen`.
    pub fn start(host: &str, extra: &[&str]) -> Self {
        Self::start_in(scratch(), host, extra)
    }

    /// Starts an agent in `home`, which may already hold pins and grants.
    pub fn start_in(home: Scratch, host: &str, extra: &[&str]) -> Self {
        let listen = format!("{host}:0");
        let mut args = vec!["--agent", "--listen", listen.as_str()];
        args.extend_from_slice(extra);
        let mut process: ono_testkit::OwnedChild = Command::new(binary())
            .args(&args)
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the agent starts")
            .into();
        let stderr = process.stderr.take().expect("stderr was piped");
        let log = Arc::new(Mutex::new(String::new()));
        let collecting = Arc::clone(&log);
        let (sender, lines) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                collecting
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push_str(&line);
                // The suite reads the start-up lines; after that nobody listens, and that is
                // fine — the log keeps collecting.
                let _ = sender.send(line.trim().to_owned());
            }
        });
        let (mut address, mut fingerprint) = (None, None);
        while address.is_none() || fingerprint.is_none() {
            let line = lines.recv_timeout(STARTUP).unwrap_or_else(|_| {
                panic!(
                    "the agent on {host} ended before it said where it listens: {}",
                    log.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                )
            });
            if let Some(bound) = line.split("listening on ").nth(1) {
                address = Some(bound.trim().to_owned());
            } else if let Some(printed) = line.split("host key ").nth(1) {
                fingerprint = Some(printed.trim().to_owned());
            }
        }
        Self {
            home,
            address: address.unwrap_or_default(),
            host: host.to_owned(),
            fingerprint: fingerprint.unwrap_or_default(),
            process: Some(process),
            log,
            _lines: lines,
        }
    }

    /// Lets the shell (or agent) whose home is `client` observe this peer (§9.4).
    pub fn authorize(&self, client: &Scratch) {
        let fingerprint = client_fingerprint(client);
        ono_at_home(
            &self.home,
            &format!("add client-key {fingerprint} --label suite"),
        )
        .assert_success();
    }

    /// Authorizes `client` and then takes its observe access away again (§9.7): the client is
    /// authenticated and authorized to connect, and may not read.
    pub fn authorize_without_observe(&self, client: &Scratch) {
        self.authorize(client);
        let fingerprint = client_fingerprint(client);
        ono_at_home(
            &self.home,
            &format!("set client-key {fingerprint} --observe false"),
        )
        .assert_success();
    }

    /// Everything the peer wrote to stderr so far: its start-up summary and audit trail.
    pub fn log(&self) -> String {
        self.log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Waits until the peer's audit trail contains `needle` at least `times` times.
    pub fn wait_for(&self, needle: &str, times: usize) -> String {
        for _ in 0..1500 {
            let seen = self.log();
            if seen.matches(needle).count() >= times {
                return seen;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "the peer never recorded `{needle}` {times} time(s): {}",
            self.log()
        );
    }

    /// Ends the peer now, as a host that goes away does.
    pub fn kill(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
            let _ = process.wait();
        }
    }

    /// The process id, while the peer runs.
    pub fn pid(&self) -> Option<u32> {
        self.process.as_ref().map(|process| process.id())
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Pins `peer`'s key in `home`'s trust store, under the peer's loopback host (spec §21.5).
pub fn pin(home: &Scratch, peer: &Peer) {
    ono_at_home(
        home,
        &format!(
            "add host-key {} --fingerprint {}",
            peer.host, peer.fingerprint
        ),
    )
    .assert_success();
}

/// A shell in `home` running `script`, whose stdout is read line by line as it is written.
pub struct Streaming {
    process: ono_testkit::OwnedChild,
    lines: mpsc::Receiver<String>,
}

impl Streaming {
    /// Starts `ono -c script` in `home`.
    pub fn start(home: &Scratch, script: &str) -> Self {
        let mut process: ono_testkit::OwnedChild = Command::new(binary())
            .args(["-c", script])
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path())
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the shell starts")
            .into();
        let stdout = process.stdout.take().expect("stdout was piped");
        let (sender, lines) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                if sender.send(line.trim().to_owned()).is_err() {
                    return;
                }
            }
        });
        Self { process, lines }
    }

    /// The next line that satisfies `wanted`, within `budget`; every line read on the way is
    /// returned too, for the failure message.
    pub fn until(&self, budget: Duration, wanted: impl Fn(&str) -> bool) -> (String, Vec<String>) {
        let deadline = std::time::Instant::now() + budget;
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) if wanted(&line) => return (line, seen),
                Ok(line) => seen.push(line),
                Err(_) => panic!("no line matched within {budget:?}; read {seen:#?}"),
            }
        }
    }
}

impl Drop for Streaming {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
