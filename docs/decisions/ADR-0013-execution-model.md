# ADR-0013: The execution model — planning, mixed pipelines and terminal ownership

- Status: accepted
- Date: 2026-08-26
- Spec refs: §11.1, §11.2, §12.2, §12.3, §12.5, §18.5, §24.5, §29, §29.3, §48
- Decided by: agent (autonomous)

## Context

Spec §48 walks an all-native pipeline from source to execution graph, and §29.1/§29.2 show mixed
pipelines, but neither says how a native stage and an external process are actually connected,
which process owns the terminal, or where the async runtime stops. Those three questions decide
whether `vim` works, whether `yes | head -1` terminates, and whether an infinite provider can
exhaust memory. They have to be answered once, before any of the pieces exist.

## Decision

### Compile, then run

Source becomes an execution graph in four steps, as spec §48 sets out:

```text
parse      -> AST with spans                              (ono-parser)
resolve    -> stable command ids, provider capabilities,  (ADR-0011)
              output schemas, external paths
check      -> field existence, unit compatibility,        (spec §11.3, ADR-0014)
              stream boundedness
plan       -> a graph of stages and the channels between them
```

Everything that can fail without touching the system fails during check, before a single process
is enumerated or spawned. `get process | where cpy > 20` reports `type.unknown_field` with a
suggestion and runs nothing, exactly as spec §11.3 requires.

### Stage kinds

```text
NativeProducer     a provider: yields Value
NativeTransform    where, select, sort, …: Value -> Value
NativeConsumer     a mutation: Value -> ActionResult
Serializer         `to <format>`: Value -> Bytes
Deserializer       `from <format>`: Bytes -> Value
External           a process: Bytes -> Bytes
Sink               the terminal, a file, or a caller
```

### Channels, and what may connect to what

Between two native stages: a bounded `tokio::sync::mpsc` channel. Bounded is the whole point —
spec §11.2 requires that a slow consumer stop an infinite producer, and a bounded channel is the
mechanism rather than a policy anyone has to remember.

Between two adjacent **External** stages: a real `pipe(2)`, with no copy through the shell. A
contiguous run of external stages is handed to `ono-process` as one OS-level pipeline. This is
not an optimisation: it is what makes `yes | head -1` deliver `SIGPIPE` to `yes` and terminate,
which a shell that shuttled the bytes itself would break.

**Native to External**: permitted only when the upstream stage yields `Bytes` or `Text`. A
structured stream reaching a process is `type.mismatch` (E0201) whose help names `to json`,
`to csv` and `format table` — spec §12.3 forbids the hidden formatting that would otherwise
become API behaviour, and spec §12.5 requires a serializer before a byte sink.

**External to Native**: the process's stdout enters the value system as bytes, decoded to text
where the configured encoding permits and never losing an undecodable byte (spec §12.2).
`from json` and its siblings turn that into values explicitly.

stderr of an external stage is never captured into the value stream. It goes where the user sent
it, defaulting to the shell's own stderr (spec §12.5).

### Where the runtime stops

Native stages run as Tokio tasks. External processes do not: they are spawned by `ono-process`
with blocking `std::process` and direct terminal calls, because terminal ownership, `tcsetpgrp`
and signal delivery are defined in terms of the controlling terminal and the foreground process
group, which spec §24.5 itself flags as outside the "everything is a task" model. The runtime is
an implementation detail of the native side and never appears in a child's world.

### Terminal ownership, and why a foreground external command gets no PTY

A foreground external command **inherits the shell's own terminal**. The shell puts it in a new
process group, hands that group the terminal with `tcsetpgrp`, and takes the terminal back when
it finishes.

Ono does **not** allocate a pseudo-terminal for an ordinary foreground command. Spec §29.3 is
explicit that "`vim` should behave like `vim`, not like content inside a Ono-Sendai widget", and
interposing a pty is precisely how a shell turns a program into content inside a widget: it adds
a translation layer that mangles window size, mouse reporting, bracketed paste, the alternate
screen and every escape the program relies on. The rich renderer gets out of the way by not
being there at all.

A pty is allocated only where a terminal must be *supplied* rather than *passed on*:

- the shell's own stdout is not a terminal but the program needs one;
- a remote link is carrying an interactive session (phase H);
- a KUANG/11 view hosts a program inside a pane (phase I).

### Pipelines and status

A pipeline's stages start together. Its status is the last stage's, and the whole per-stage
status vector is retained on the history entry rather than collapsed (ADR-0008, spec §16.5).

A pipeline that ends in a `NativeConsumer` yields `ActionResult` values, one per target, and its
status is derived from them — never from a count that lost which target failed.

### Cancellation

Ctrl-C sends `SIGINT` to the foreground process group, which reaches external children the way
they expect. The same event trips a cancellation token that every native stage's channel is
selected against, so a native pipeline stops at its next await rather than at the end of its
current producer (spec §18.5). A cancelled pipeline exits 130.

### Transiently missing objects

Spec §48 Step 6 leaves provider policy open and requires it be deterministic and tested. The
policy: an object that disappears between enumeration and detail read is **skipped**, and a
`provider.unavailable` error value is emitted on the pipeline's error channel carrying the
object's identity. It is neither silently dropped, which would make a count wrong, nor fatal,
which would make `get process` fail whenever anything exited.

## Consequences

Easy: `vim`, `less`, `top` and `ssh` behave exactly as under any other shell, because the shell
is not between them and the terminal; `yes | head -1` terminates; an infinite provider cannot
outrun a slow consumer; a typo costs nothing because nothing ran.

Hard: mixed native/external pipelines have two different connection mechanisms, and the planner
has to know which one applies where. That is inherent — one carries values, the other carries
bytes — and making them uniform would mean either serialising everything, which loses the point
of an object shell, or shuttling every byte through the shell, which loses `SIGPIPE`.

Must be revisited in phase H, where a stage may execute on another machine and the channel
becomes a multiplexed protocol stream.

Encoded by: the pipeline tests in `crates/ono-cli`, the external-pipeline tests in
`crates/ono-process`, and the acceptance cases `023-external-pipelines`, `024-pty-applications`,
`035-interop-boundary`.

## Alternatives considered

- **Allocating a pty for every foreground command** — rejected: it makes the shell an
  intermediary in every escape sequence a full-screen program sends, which is the failure spec
  §29.3 names. It also doubles the syscall cost of the most common thing a shell does.
- **Auto-serialising a structured stream into an external process** — rejected by spec §12.3:
  the chosen format would silently become the contract, and changing it later would break every
  script that depended on it.
- **Running external processes as Tokio tasks** — rejected: `tokio::process` gives no control
  over the process group at `fork` time and no path to `tcsetpgrp`, so job control would have to
  be faked.
- **`pipefail` semantics for pipeline status** — rejected in ADR-0008; the per-stage vector is
  explicit and always present instead.
