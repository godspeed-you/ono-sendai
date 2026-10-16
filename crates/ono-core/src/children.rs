//! Children a command starts outside the shell's executor — a provider's helper program, an
//! opaque plan action, a version probe — recorded for as long as they run, so a shell ending on a
//! signal can end them too (ADR-0959).
//!
//! The executor owns its process groups itself; these children are started by code that cannot
//! reach it and waits for them in place. Each one is recorded by its process id from just after
//! it was started until just after it was waited for, and the record is cleared then. A shell that
//! ends on a signal sends every recorded child the signals it sends its jobs.

use std::sync::{Arc, Mutex, PoisonError, Weak};

/// Every recorded child, weakly: a record goes when its guard does.
static RECORDED: Mutex<Vec<Weak<Mutex<i32>>>> = Mutex::new(Vec::new());

/// A child recorded for as long as this guard lives. Drop it once the child has been waited for.
#[derive(Debug)]
pub struct Recorded(Arc<Mutex<i32>>);

impl Recorded {
    /// Records the child with process id `pid`.
    #[must_use]
    pub fn new(pid: u32) -> Self {
        let cell = Arc::new(Mutex::new(i32::try_from(pid).unwrap_or(0)));
        let mut recorded = RECORDED.lock().unwrap_or_else(PoisonError::into_inner);
        recorded.retain(|weak| weak.strong_count() > 0);
        recorded.push(Arc::downgrade(&cell));
        Self(cell)
    }

    /// Records the child `pid` names, if it names one.
    #[must_use]
    pub fn of(pid: Option<u32>) -> Option<Self> {
        pid.map(Self::new)
    }
}

impl Drop for Recorded {
    fn drop(&mut self) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = 0;
    }
}

/// Calls `signal` with the process id of every recorded child, each under its record's lock, so
/// none is signalled after its guard has cleared it.
pub fn for_each(mut signal: impl FnMut(i32)) {
    let recorded: Vec<Arc<Mutex<i32>>> = RECORDED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter_map(Weak::upgrade)
        .collect();
    for cell in recorded {
        let pid = cell.lock().unwrap_or_else(PoisonError::into_inner);
        if *pid > 0 {
            signal(*pid);
        }
    }
}
