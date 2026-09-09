//! Spec §27.2: every stable command is bound to an implementation.
//!
//! The registry is the product surface and it is written before the code (spec §27, §36), which
//! makes a stable command with nothing behind it the cheapest possible drift: the contract still
//! advertises it, `help` still describes it, and only running it says otherwise. §27.2 asks CI to
//! check for exactly that. [`ono_command::unbound_stable_commands`] has been the list to fail on
//! since phase D; this is the check that finally runs it against the real registry.
//!
//! The yardstick is [`ono_command::builtin_commands`] — the table the *library* assembles, with
//! no providers and no shell around it. Sixty-five stable commands are deliberately not in it,
//! because their implementation lives somewhere the library cannot reach: a provider, the
//! evaluator, or `ono-cli` itself. `BOUND_ELSEWHERE` below names every one of them together with what
//! does bind it, and each group is exercised end to end by the suite that owns it —
//! `crates/ono-cli/tests/files.rs`, `storage.rs`,
//! `containers_packages.rs`, `processes.rs` for the provider-bound verbs,
//! `context.rs` for `enter`/`leave`, `meta_config.rs` for configuration, `plugins.rs`
//! and `plugin_commands.rs` for KUANG/11, `remote_commands.rs` for the link table, the
//! `spatial_*` suites for the fourteen spatial verbs, and `change_*` for the thirteen v0.6
//! change and recovery verbs.

use std::collections::BTreeSet;

use ono_command::{CommandRegistry, Phase, Stability};

pub use crate::scan::Problem;

/// A stable command the library table does not bind, and what binds it instead.
///
/// Every entry is a claim, and it is checked in both directions: a command that is missing from
/// the table and from this list fails the gate, and so does an entry that has become bound.
const BOUND_ELSEWHERE: &[(&str, &str)] = &[
    // --- a provider binds it -------------------------------------------------------------------
    // A mutating verb binds where a provider for the target advertises the capability the
    // contract names (ADR-0068 §3), and a content verb the same way (ADR-0083). A table built
    // without providers therefore binds neither, and registering a stub instead would give the
    // user a command that always fails rather than one that is honestly not there (spec §50).
    (
        "ono.file.read",
        "a provider advertising `file.read` (ADR-0083)",
    ),
    (
        "ono.file.write",
        "a provider advertising `file.write` (ADR-0068 §3)",
    ),
    (
        "ono.file.copy",
        "a provider advertising `file.copy` (ADR-0068 §3)",
    ),
    (
        "ono.file.move",
        "a provider advertising `file.move` (ADR-0068 §3)",
    ),
    (
        "ono.file.remove",
        "a provider advertising `file.remove` (ADR-0068 §3)",
    ),
    (
        "ono.filesystem.mount",
        "a provider advertising `mount.manage` (ADR-0068 §3)",
    ),
    (
        "ono.filesystem.unmount",
        "a provider advertising `mount.manage` (ADR-0068 §3)",
    ),
    (
        "ono.package.add",
        "a provider advertising `package.manage` (ADR-0068 §3)",
    ),
    (
        "ono.package.remove",
        "a provider advertising `package.manage` (ADR-0068 §3)",
    ),
    (
        "ono.package.set",
        "a provider advertising `package.manage` (ADR-0068 §3)",
    ),
    (
        "ono.package-source.refresh",
        "a provider advertising `package-source.refresh` (ADR-0068 §3, ADR-0565)",
    ),
    (
        "ono.process.set",
        "a provider advertising `process.set` (ADR-0068 §3, ADR-0092)",
    ),
    (
        "ono.signal.send",
        "a provider advertising `process.signal` (ADR-0092 §2)",
    ),
    // --- the session owns the context stack ----------------------------------------------------
    // `enter` and `leave` push and pop the frames of spec §14.1, which live in the session and
    // not in any provider (ADR-0075, ADR-0076).
    ("ono.container.enter", "the session's context stack (§14.1)"),
    ("ono.dir.enter", "the session's context stack (§14.1)"),
    ("ono.file.enter", "the session's context stack (§14.1)"),
    ("ono.group.enter", "the session's context stack (§14.1)"),
    ("ono.interface.enter", "the session's context stack (§14.1)"),
    ("ono.link.enter", "the session's context stack (§14.1)"),
    ("ono.mount.enter", "the session's context stack (§14.1)"),
    ("ono.process.enter", "the session's context stack (§14.1)"),
    ("ono.service.enter", "the session's context stack (§14.1)"),
    ("ono.socket.enter", "the session's context stack (§14.1)"),
    ("ono.user.enter", "the session's context stack (§14.1)"),
    ("ono.context.leave", "the session's context stack (§14.1)"),
    // --- the evaluator owns configuration and scope --------------------------------------------
    (
        "ono.config.get",
        "the evaluator's configuration layers (ADR-0010)",
    ),
    (
        "ono.config.set",
        "the evaluator's configuration layers (ADR-0010)",
    ),
    (
        "ono.limits.inspect",
        "the evaluator's settings catalogue, which is what the shell enforces (ADR-0456)",
    ),
    ("ono.env.set", "the evaluator's own scope (ADR-0020 §9)"),
    // --- `ono-cli` answers it as a shell builtin -----------------------------------------------
    // KUANG/11's lifecycle and the remote link table are the shell's: the supervisor and the
    // link store are session state, so `crates/ono-cli/src/plugins.rs` and `remote.rs` claim
    // these stages before the table is asked.
    (
        "ono.plugin.install",
        "`ono-cli`'s KUANG/11 host (spec §31.8)",
    ),
    (
        "ono.plugin.remove",
        "`ono-cli`'s KUANG/11 host (spec §31.8)",
    ),
    ("ono.plugin.load", "`ono-cli`'s KUANG/11 host (spec §31.10)"),
    (
        "ono.plugin.unload",
        "`ono-cli`'s KUANG/11 host (spec §31.10)",
    ),
    ("ono.plugin.set", "`ono-cli`'s KUANG/11 host (spec §31.8)"),
    (
        "ono.plugin.verify",
        "`ono-cli`'s KUANG/11 host (spec §31.16)",
    ),
    (
        "ono.capability.grant",
        "`ono-cli`'s capability broker (spec §31.16)",
    ),
    (
        "ono.capability.revoke",
        "`ono-cli`'s capability broker (spec §31.16)",
    ),
    (
        "ono.permission.set",
        "`ono-cli`'s permission layer over the broker (K11P §16, ADR-0604)",
    ),
    ("ono.assistant.ask", "`ono-cli`'s KUANG/11 host (spec §31)"),
    ("ono.host.link", "`ono-cli`'s link table (ADR-0104)"),
    ("ono.link.detach", "`ono-cli`'s link table (ADR-0104)"),
    // --- `ono-cli` registers the spatial verbs -------------------------------------------------
    // v0.4 §45.6 keeps spatial dispatch in the shell, because a place belongs to a host and a
    // boot that no library crate knows (ADR-0141).
    (
        "ono.place.find",
        "`ono-cli`'s spatial dispatch (v0.4 §45.6)",
    ),
    (
        "ono.place.look",
        "`ono-cli`'s spatial dispatch (v0.4 §45.6)",
    ),
    (
        "ono.place.near",
        "`ono-cli`'s spatial dispatch (v0.4 §45.6)",
    ),
    (
        "ono.place.enter",
        "`ono-cli`'s spatial dispatch (v0.4 §45.6)",
    ),
    (
        "ono.place.follow",
        "`ono-cli`'s spatial dispatch (v0.4 §45.6)",
    ),
    ("ono.place.map", "`ono-cli`'s spatial dispatch (v0.4 §45.6)"),
    (
        "ono.place.map-links",
        "`ono-cli`'s spatial dispatch (v0.4 §45.6)",
    ),
    (
        "ono.place.home",
        "`ono-cli`'s spatial dispatch (v0.4 §45.6)",
    ),
    (
        "ono.place.back",
        "`ono-cli`'s spatial dispatch (v0.4 §45.6)",
    ),
    ("ono.place.up", "`ono-cli`'s spatial dispatch (v0.4 §45.6)"),
    (
        "ono.place.jump",
        "`ono-cli`'s spatial dispatch (v0.4 §45.6)",
    ),
    (
        "ono.place.trail",
        "`ono-cli`'s spatial dispatch (v0.4 §45.6)",
    ),
    ("ono.place.pin", "`ono-cli`'s spatial dispatch (v0.4 §45.6)"),
    (
        "ono.place.unpin",
        "`ono-cli`'s spatial dispatch (v0.4 §45.6)",
    ),
    // --- `ono-cli` registers the temporal verbs ------------------------------------------------
    // v0.5 §39 keeps `ono-cli` as the crate that "owns active TemporalContext in Session", and
    // every one of these reads that coordinate and the session's ledger. A library table has
    // neither, so binding one there would give the user a command that always answers about the
    // present (ADR-0690).
    (
        "ono.temporal.at",
        "`ono-cli`'s temporal dispatch (v0.5 §4.2, §39)",
    ),
    (
        "ono.temporal.now",
        "`ono-cli`'s temporal dispatch (v0.5 §4.3, §39)",
    ),
    (
        "ono.temporal.present",
        "`ono-cli`'s temporal dispatch (v0.5 §4.8, §39)",
    ),
    (
        "ono.temporal.timeline",
        "`ono-cli`'s temporal dispatch (v0.5 §11, §39)",
    ),
    (
        "ono.temporal.changes",
        "`ono-cli`'s temporal dispatch (v0.5 §13, §39)",
    ),
    (
        "ono.temporal.why",
        "`ono-cli`'s temporal dispatch (v0.5 §16, §39)",
    ),
    (
        "ono.event.find",
        "`ono-cli`'s temporal dispatch (v0.5 §20.3, §39)",
    ),
    (
        "ono.event.inspect",
        "`ono-cli`'s temporal dispatch (v0.5 §11.6, §39)",
    ),
    (
        "ono.recorder.get",
        "`ono-cli`'s temporal dispatch (v0.5 §10.3, §39)",
    ),
    (
        "ono.recorder.start",
        "`ono-cli`'s temporal dispatch (v0.5 §10.3, §39)",
    ),
    (
        "ono.recorder.stop",
        "`ono-cli`'s temporal dispatch (v0.5 §10.3, §39)",
    ),
    (
        "ono.temporal-history.remove",
        "`ono-cli`'s temporal dispatch (v0.5 §30.8, §39)",
    ),
    // --- `ono-cli` binds the change dispatch, v0.6 §50 -----------------------------------------
    // Every one of the thirteen needs session state the library cannot reach: the plan store of
    // §36.1, the recovery provider registry, the spatial index the impact graph is derived over,
    // and the v0.5 ledger the lifecycle events go to. §62.10 forbids the plan semantics living in
    // `ono-cli` — they live in `ono-change-*`, and what `ono-cli` binds is the wiring.
    (
        "ono.change.plan",
        "`ono-cli`'s change dispatch (v0.6 §5.1–§5.3, §50)",
    ),
    (
        "ono.change-plan.get",
        "`ono-cli`'s change dispatch (v0.6 §36.4, §50)",
    ),
    (
        "ono.change-plan.inspect",
        "`ono-cli`'s change dispatch (v0.6 §5, Appendix B.10, §50)",
    ),
    (
        "ono.change-plan.rebase",
        "`ono-cli`'s change dispatch (v0.6 §7.5, §50)",
    ),
    (
        "ono.change-plan.resume",
        "`ono-cli`'s change dispatch (v0.6 §41.3, §50)",
    ),
    (
        "ono.change.impact",
        "`ono-cli`'s change dispatch (v0.6 §5.4, §50)",
    ),
    (
        "ono.change.protect",
        "`ono-cli`'s change dispatch (v0.6 §5.5, §50)",
    ),
    (
        "ono.change.apply",
        "`ono-cli`'s change dispatch (v0.6 §5.6, §50)",
    ),
    (
        "ono.change.verify",
        "`ono-cli`'s change dispatch (v0.6 §5.7, §50)",
    ),
    (
        "ono.change.recover",
        "`ono-cli`'s change dispatch (v0.6 §5.8, §50)",
    ),
    (
        "ono.recovery.get",
        "`ono-cli`'s change dispatch (v0.6 §37.5, §50)",
    ),
    (
        "ono.recovery.inspect",
        "`ono-cli`'s change dispatch (v0.6 §11.1, §50)",
    ),
    (
        "ono.recovery.remove",
        "`ono-cli`'s change dispatch (v0.6 §37.3, §50)",
    ),
];

/// The stable commands `registry` declares as delivered that `is_bound` does not answer for.
///
/// `is_bound` is the command table, asked by id. It is a closure rather than a `CommandTable` so
/// that the check can be driven with a table that has lost a binding, which is the only way to
/// know it would notice.
#[must_use]
pub fn check_bindings(registry: &CommandRegistry, is_bound: impl Fn(&str) -> bool) -> Vec<Problem> {
    let elsewhere: BTreeSet<&str> = BOUND_ELSEWHERE.iter().map(|(id, _)| *id).collect();
    let mut problems = Vec::new();

    for contract in registry.commands() {
        if contract.stability() != Stability::Stable
            || !matches!(contract.phase(), Phase::Delivered(_))
        {
            continue;
        }
        let id = contract.id();
        match (is_bound(id), elsewhere.contains(id)) {
            (false, false) => problems.push(Problem {
                location: id.to_owned(),
                detail: format!(
                    "spec §27.2: `{id}` is a stable command of a delivered phase and nothing \
                     implements it. Bind it, or name what does bind it in `BOUND_ELSEWHERE` in \
                     xtask/src/bindings.rs, beside the suite that runs it"
                ),
            }),
            (true, true) => problems.push(Problem {
                location: id.to_owned(),
                detail: format!(
                    "`{id}` is bound by the library table, and `BOUND_ELSEWHERE` in \
                     xtask/src/bindings.rs still says something else binds it. Remove the entry: \
                     an excuse nobody rechecks is how spec §27.2 stops being checked"
                ),
            }),
            _ => {}
        }
    }

    problems
}
