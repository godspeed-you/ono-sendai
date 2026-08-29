# ADR-0015: The threat model, made testable

- Status: superseded by ADR-0245
- Date: 2026-08-26
- Spec refs: §17, §31.16, §35.6, §43, §49
- Decided by: agent (autonomous)

## Context

Spec §49 lists fifteen threats and nine mitigation directions, in prose. `docs/ACCEPTANCE.md`
§4.4 turns that into a release requirement: "The threat model of spec §49 has a test for each
stated risk." A prose list cannot be checked off, and a threat with no named owner and no named
test is a threat nobody will get to. This ADR fixes, for each threat, which component owns the
mitigation and which test proves it, so the release checklist has something to point at.

## Decision

Each row is a release-blocking requirement. A phase that lands the owning component lands its
row's test in the same increment.

| # | Threat (spec §49) | Mitigation | Owner | Proven by |
|---|---|---|---|---|
| T1 | malicious filenames containing control sequences | every value is sanitised at the render boundary, unconditionally; the raw value is retained separately (spec §49: "retain raw data separately from display") | `ono-render` | `crates/ono-render/tests/presentation.rs`; acceptance case creating a file whose name holds an escape sequence and listing it |
| T2 | terminal escape injection from external stdout | external bytes reaching a **terminal** are passed through only while the external command owns the foreground terminal (ADR-0013); bytes Ono itself renders are sanitised | `ono-process`, `ono-render` | acceptance: an external program printing escapes into a native pipeline cannot move the cursor |
| T3 | plugin code execution | KUANG/11 isolation and the capability broker; install, enable, load and run are separate states (spec §31.8, §31.10) | `ono-kuang-supervisor` | plugin conformance suite: denial paths (spec §31.74) |
| T4 | poisoned completion sources | completion never executes a candidate source; candidates are sanitised before display and never auto-accepted | `ono-editor`, completion | completion tests with hostile candidates |
| T5 | remote agent impersonation | explicit trust store, pinned host keys, mutual authentication before any provider call | `ono-protocol` | protocol tests: an unknown key is refused, not prompted past |
| T6 | host key changes | `remote.host_key_changed` (E0603), classified `safety` rather than transport (ADR-0006), and never auto-accepted | `ono-protocol` | protocol test asserting refusal and the error code |
| T7 | schema/protocol bombs causing memory exhaustion | bounded frames, bounded depth, bounded total size on every decoder; bounded channels everywhere (ADR-0013) | `ono-protocol`, `ono-kuang-protocol`, `ono-value` | fuzz targets plus explicit deep/large-input tests |
| T8 | history leakage of secrets | a `Secret` semantic type with redacted default rendering; a secret-aware history policy that redacts before writing (spec §17.5) | `ono-history`, `ono-value` | history test: a secret value never reaches the history file |
| T9 | unsafe rendering of OSC hyperlinks | OSC is never emitted from data; hyperlinks only from a theme-controlled construct that cannot take a value's text as its target | `ono-render` | `presentation.rs`: an OSC sequence in a value cannot survive painting |
| T10 | command confusion between native and external namespaces | the fixed resolution order and forced namespaces of ADR-0011, with `explain` reporting what the code actually did | `ono-cli` | resolution tests; `explain` acceptance case |
| T11 | PATH shadowing | `explain` prints the absolute path of an external hit; a destructive command shows its resolved target before acting (spec §17.1) | `ono-cli` | acceptance: a shadowing binary earlier in `PATH` is visible in `explain` |
| T12 | TOCTOU between preview and destructive action | identity is confirmed immediately before mutation, not at preview time; a target whose identity changed is reported `failed` in its `ActionResult` rather than acted on | `ono-command`, providers | test: an object replaced between preview and act is refused |
| T13 | PID reuse between selection and signal | a process's identity is `(pid, started)` — the `identity` list of `ono.process/1` — and is re-read before signalling; a mismatch refuses | `ono-provider-linux` | test: a recycled pid is not signalled |
| T14 | symlink races | directory traversal uses `openat`-relative operations with `O_NOFOLLOW` where the operation is not meant to follow links; no path is re-resolved between check and use | `ono-provider-linux` | test: a symlink swapped mid-traversal does not escape the tree |
| T15 | privilege escalation boundaries | elevation is explicit and visible (spec §17.2); no native command silently elevates; the prompt makes an elevated context impossible to miss | `ono-cli`, `ono-render` | acceptance: privilege visible in the prompt; no command elevates without being asked |

### Standing rules that follow from the table

1. **Sanitisation is at the render boundary, not at the provider.** A provider must report exactly
   what the system said, because that is what makes `inspect` trustworthy (spec §49: retain raw
   data separately from display). The renderer is where hostile bytes stop.
2. **No security-relevant behaviour is conditional on a configuration setting.** T1, T2 and T9 in
   particular are unconditional; a setting that can turn them off is a setting an attacker can
   arrange to have set.
3. **Every decoder is fuzzed** (spec §35.6): the parser, each serializer, the remote protocol, the
   plugin protocol, and the procfs and netlink decoders. A decoder without a fuzz target is not
   finished.
4. **A refusal is never a prompt.** T5 and T6 fail with a structured error; they do not offer a
   "continue anyway" that a script will eventually answer for the user.

## Consequences

Easy: `docs/ACCEPTANCE.md` §4.4 becomes checkable — fifteen rows, fifteen tests; a reviewer can
ask "which row is this?" of any security change.

Hard: several rows belong to phases that do not exist yet, so the table is a standing debt
tracked in `docs/STATE.md` until each is closed. That is the intended shape: the debt is
enumerated rather than forgotten.

Encoded by: the tests named in the table, and the cross-cutting entry in `docs/STATE.md`.

## Alternatives considered

- **Handling the threats as they arise, per phase** — rejected: T1, T2, T9 and T12 constrain
  interfaces that phases B and C freeze, and retrofitting sanitisation or identity confirmation
  after providers exist means changing every provider.
- **A configurable sanitisation policy** — rejected: see standing rule 2.
