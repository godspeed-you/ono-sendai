//! Helpers shared by the `ono-process` integration tests.
//!
//! Every test that waits on a real process runs its body under a hard deadline, so a regression
//! that deadlocks fails the suite instead of hanging it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]
#![allow(dead_code)]

use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// Serialises tests that observe a property of the whole process rather than of their own work.
///
/// `cargo test` runs the tests in one binary on parallel threads, so a test that counts the
/// process's open descriptors is measuring its siblings as much as itself. Taking this lock is
/// what makes such a test assert its own contract instead of the scheduler's timing.
pub fn exclusive() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let mutex = LOCK.get_or_init(|| Mutex::new(()));
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs `body` on a worker thread and fails the test if it does not finish within `timeout`.
pub fn within<T: Send + 'static>(
    timeout: Duration,
    body: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(body());
    });
    match rx.recv_timeout(timeout) {
        Ok(value) => value,
        Err(_) => panic!("the operation did not finish within {timeout:?}"),
    }
}

/// Polls `probe` until it yields a value or `timeout` elapses.
pub fn poll_until<T>(timeout: Duration, mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "the expected state was not reached within {timeout:?}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

/// The default deadline for a test that waits on a child process.
pub const DEADLINE: Duration = Duration::from_secs(30);

/// Renders captured bytes for an assertion message.
pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
