//! Children a command starts outside the shell's executor — a provider's helper program, an
//! opaque plan action, a version probe — recorded for as long as they run, so a shell ending on a
//! signal can end them too (ADR-0959).
//!
//! The executor owns its process groups itself; these children are started by code that cannot
//! reach it and waits for them in place. Each one is recorded from just after it was started until
//! just after it was waited for — by its process group when it was started in a group of its own,
//! so the programs it starts in turn are reached too, and by its process id otherwise. A shell that
//! ends on a signal sends every recorded child the signals it sends its jobs, and once it has begun
//! to end, a child recorded after that is ended as soon as it is recorded.

use std::sync::{Arc, Mutex, PoisonError, Weak};

/// What a record names: a process group (negative, as `kill(2)` takes one) or a process.
type Target = i32;

/// Every recorded child, weakly, and the hook that ends a child recorded once the shell is ending.
struct Records {
    children: Vec<Weak<Mutex<Target>>>,
    closing: Option<fn(Target)>,
}

static RECORDED: Mutex<Records> = Mutex::new(Records {
    children: Vec::new(),
    closing: None,
});

/// A child recorded for as long as this guard lives. Drop it — or call [`Recorded::reaped`] — in
/// the step right after the child has been waited for.
#[derive(Debug)]
pub struct Recorded(Arc<Mutex<Target>>);

impl Recorded {
    /// Records the child with process id `pid`, signalled alone.
    #[must_use]
    pub fn new(pid: u32) -> Self {
        Self::record(i32::try_from(pid).unwrap_or(0))
    }

    /// Records the child with process id `pid`, which leads a process group of its own: the
    /// group is signalled, so its descendants are reached too.
    #[must_use]
    pub fn group(pid: u32) -> Self {
        Self::record(-i32::try_from(pid).unwrap_or(0))
    }

    /// Records the child `pid` names, if it names one, signalled alone.
    #[must_use]
    pub fn of(pid: Option<u32>) -> Option<Self> {
        pid.map(Self::new)
    }

    /// Records the group the child `pid` leads, if it names one.
    #[must_use]
    pub fn group_of(pid: Option<u32>) -> Option<Self> {
        pid.map(Self::group)
    }

    fn record(target: Target) -> Self {
        let cell = Arc::new(Mutex::new(target));
        let mut records = RECORDED.lock().unwrap_or_else(PoisonError::into_inner);
        records.children.retain(|weak| weak.strong_count() > 0);
        records.children.push(Arc::downgrade(&cell));
        // A shell already ending ends what is started now, under the same lock its last sweep
        // took, so no child slips between that sweep and the shell's end (review Z7).
        if let Some(end) = records.closing
            && target != 0
        {
            end(target);
        }
        Self(cell)
    }

    /// Clears the record: the child has been waited for, and its id may be another's soon.
    pub fn reaped(&self) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = 0;
    }
}

impl Drop for Recorded {
    fn drop(&mut self) {
        self.reaped();
    }
}

/// Calls `signal` with what every recorded child is reached by — a negative process group or a
/// process id — each under its record's lock, so none is signalled after its record was cleared.
pub fn for_each(mut signal: impl FnMut(i32)) {
    let recorded: Vec<Arc<Mutex<Target>>> = RECORDED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .children
        .iter()
        .filter_map(Weak::upgrade)
        .collect();
    for cell in recorded {
        let target = cell.lock().unwrap_or_else(PoisonError::into_inner);
        if *target != 0 {
            signal(*target);
        }
    }
}

/// From now on, a child recorded is passed to `end` at once: the shell is ending (review Z7).
pub fn close(end: fn(i32)) {
    RECORDED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .closing = Some(end);
}
