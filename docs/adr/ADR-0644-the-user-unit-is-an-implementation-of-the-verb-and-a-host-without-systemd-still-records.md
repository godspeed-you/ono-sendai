# ADR-0644: The user unit is an implementation of the verb, and a host without systemd still records

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §10.3, §10.5, §10.9, §22.8, §30.2
- Decided by: agent (autonomous)

## Context

§10.9 asks the reference Linux implementation to support a user service named
`ono-recorder.service`, and then constrains what that may mean: "the command layer MAY start/stop
this through the existing service mechanisms or a dedicated recorder controller, but user-facing
semantics remain `start recorder` and `stop recorder`."

§10.5 constrains it further, and the two together decide the shape. The recorder "MUST NOT run a
privileged system daemon merely to increase visibility", and §22.8 adds the reason: "v0.5 must
remain useful on a normal Linux account. High-fidelity privileged sources are extensions, not a
hidden prerequisite."

## Decision

### 1. The unit exists, and it is never a second interface

`service::unit_file` produces the unit and `service::ServiceControl` drives it, and neither is
reachable from a user. `start recorder` reaches `ServiceControl` where a systemd user manager is
present and `Recorder::start` directly where it is not; `get recorder` answers from the recorder's
own state either way. There is no command that names the unit.

### 2. Every invocation is `--user`, and no code path can reach the system manager

`ServiceControl` builds `["--user", verb, "ono-recorder.service"]` and there is no constructor,
builder or parameter that changes the first element. §10.5's third prohibition is then structural
rather than remembered, and `crates/ono-recorder/tests/privilege.rs` asserts it over the calls
rather than over the code.

The runner is a trait so the assertion is an outcome on a host with no systemd — the acceptance
container has none — and so the real `systemctl` is one implementation rather than the only one.

### 3. The unit carries §10.5 where the service manager enforces it

`NoNewPrivileges=yes` is the setuid prohibition in the one place it survives a user running
`systemctl` by hand. `RestrictSUIDSGID=yes` is beside it. There is no `User=`: a user unit runs as
the user by construction, and naming one would be the beginning of the daemon §10.5 forbids.
`MemoryMax=256M` gives §32.4's 100 MiB budget a ceiling the kernel keeps rather than a target the
code hopes for.

`WantedBy=default.target` rather than `multi-user.target`, because those are the user manager's
targets and the system manager's respectively, and the wrong one would be a unit that never starts.

### 4. A host with no systemd records anyway

`service::systemd_available` looks for the user manager's own socket under `XDG_RUNTIME_DIR` and
answers false where there is none. False is not an error: the recorder is a user-level collector,
and §10.9's "SHOULD" is about the *service*, not about the recording. The acceptance container has
no systemd, no journald and no D-Bus, and a temporal acceptance case starts the recorder there
with no image change.

### 5. `ExecStart` names a flag the CLI must provide

The unit runs `<executable> --recorder-service`, which is `service::DEFAULT_ARGUMENT` and is
overridable through `UnitOptions::with_arguments`. The flag has to exist in `ono-cli` for the unit
to work; until it does, the unit is written and not installed, and `start recorder` takes the
in-process path. This ADR records the dependency rather than hiding it.

## Consequences

The recorder crate spawns exactly one program, `systemctl`, and the privilege test asserts that by
scanning its own source for any other `Command::new`. A future need to spawn something else is a
change that has to argue with a test.

`unit_path` resolves `~/.config/systemd/user/ono-recorder.service` through the same XDG rule the
ledger path uses, so installing the unit needs neither a real environment nor a real home in a
test.

Tests: `crates/ono-recorder/tests/lifecycle.rs` (the unit's content, the `--user` invocations, and
the recorder starting on a host that reports no systemd), `crates/ono-recorder/tests/privilege.rs`
(no `--system`, and nothing else spawned).

## Alternatives considered

- **A system unit with a `User=` line.** Rejected: it is the privileged daemon §10.5 forbids,
  wearing a user's name.
- **D-Bus activation.** Rejected: the acceptance container has no D-Bus daemon, so the mechanism
  that proves the product would be the one mechanism the harness cannot exercise.
- **No unit at all, only the in-process recorder.** Rejected: §10.9 asks for it by name, and a
  recorder that stops when the shell exits retains nothing across sessions, which is §10.1's whole
  purpose.
