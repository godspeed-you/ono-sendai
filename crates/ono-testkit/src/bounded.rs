//! Running the real binary under a deadline that is kept, and reaping what it leaves.
//!
//! [`Shell`](crate::Shell) is the right shape for a suite whose subject is expected to answer: it
//! panics when a run overruns, which is a clear failure. A test whose *subject is the hang* needs
//! the opposite. The overrun is the observation, so the run has to come back with what it managed
//! to say, and the child has to be gone before the assertion is written.
//!
//! That second half is not a nicety. Issue #22 found two leftover `ono` processes holding a
//! pipeline open for seven hours at about 2.2 GiB each, and a sweep on 2026-09-02 killed 331
//! leaked `journalctl --follow` stubs, the oldest five days old. A proof about a hang that leaks
//! the hung process has made the machine worse in order to describe it.

use std::time::{Duration, Instant};

use crate::Scratch;

/// What a run produced within a budget it was not allowed to exceed.
///
/// [`Shell`](crate::Shell) panics when a run overruns and leaves the child behind, which is the
/// right shape for a suite whose subject is expected to answer. A test whose *subject is the
/// hang* needs the opposite: the overrun is the observation, and the child must be gone before
/// the assertion is written, so an ignored proof cannot leak a shell onto the machine that ran
/// it (issue #22 found two such strays holding a pipeline open for seven hours).
#[derive(Debug)]
pub struct Bounded {
    /// The script that was run, so a failure message can quote it.
    pub script: String,
    /// The budget it was given.
    pub budget: Duration,
    /// Whether it finished on its own rather than being killed at the deadline.
    pub finished: bool,
    /// Its exit code, when it exited with one.
    pub code: Option<i32>,
    /// Everything it wrote to standard output before it finished or was killed.
    pub stdout: String,
    /// Everything it wrote to standard error before it finished or was killed.
    pub stderr: String,
}

impl Bounded {
    /// Whether the run said nothing at all on either stream — v0.4.1 §33.3's "neither output nor
    /// progress".
    pub fn silent(&self) -> bool {
        self.stdout.trim().is_empty() && self.stderr.trim().is_empty()
    }

    /// The run, its budget and both streams, as a failure message reads them.
    pub fn report(&self) -> String {
        format!(
            "`ono -c {:?}` {} within {:?}\n--- stdout ({} bytes) ---\n{}\n--- stderr ({} bytes) ---\n{}",
            self.script,
            if self.finished {
                format!("exited {:?}", self.code)
            } else {
                "was still running and was killed".to_owned()
            },
            self.budget,
            self.stdout.len(),
            self.stdout,
            self.stderr.len(),
            self.stderr
        )
    }
}

/// Runs `script` through the real binary with `dir` as its whole configuration home, and kills it
/// at `budget` rather than waiting for it.
///
/// Both streams are drained on their own threads, so what comes back is what a user would have
/// seen by the deadline even when the shell never reaches the end of its pipeline.
pub fn run_bounded(dir: &Scratch, script: &str, budget: Duration) -> Bounded {
    let mut child = std::process::Command::new(crate::ono_binary())
        .args(["-c", script])
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path().join("xdg"))
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .env("ONO_CONFIG_DIR", dir.path().join("ono"))
        .env("NO_COLOR", "1")
        .env_remove("ONO_CONFIG")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the ono binary must be built before an integration test runs it");

    let out = drain(child.stdout.take().expect("stdout was piped"));
    let err = drain(child.stderr.take().expect("stderr was piped"));

    let deadline = Instant::now() + budget;
    let mut finished = false;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => {
                finished = true;
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }
    if !finished {
        let _ = child.kill();
    }
    // Reaped on every path, including the one that killed it, so an overrunning proof leaves no
    // zombie behind the tests that run after it.
    let status = child.wait().ok();

    Bounded {
        script: script.to_owned(),
        budget,
        finished,
        code: status.and_then(|exited| exited.code()),
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    }
}

/// Reads a pipe to its end on a worker, so neither stream can deadlock the other.
fn drain(mut pipe: impl std::io::Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = pipe.read_to_end(&mut buffer);
        String::from_utf8_lossy(&buffer).into_owned()
    })
}
