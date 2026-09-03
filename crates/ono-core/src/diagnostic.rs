//! Saying something on standard error without making the listener a dependency.
//!
//! Commentary is what a program says about its work; it is never the work. A shell that answered
//! a question has answered it whether or not anybody was still reading the console, and a
//! listening agent is serving whether or not its log is going anywhere.

/// Writes one line of commentary to standard error, and does not care whether it arrived.
///
/// The arguments are [`eprintln!`]'s, and the one difference matters to anything that outlives
/// whoever started it: a write that fails costs the line and nothing else. `eprintln!` panics on
/// a failed write, so a process whose standard error is a pipe nobody drains any more —
/// `ono --agent 2>&1 | head`, a log a supervisor closed, a console an operator shut — is killed
/// by its own commentary (`Broken pipe`, `No space left on device`).
///
/// ```
/// ono_core::diagnostic!("{}: {}", ono_core::SHORT_NAME, "listening on 127.0.0.1:7999");
/// ```
#[macro_export]
macro_rules! diagnostic {
    ($($arg:tt)*) => {{
        use ::std::io::Write as _;
        let mut stream = ::std::io::stderr().lock();
        let _ = ::std::writeln!(stream, $($arg)*);
    }};
}
