# ADR-0216: A mutation that declares no content input takes the piped objects

- Status: accepted
- Date: 2026-08-29
- Spec refs: §11.5, §12.1, §12.3, §14.3
- Decided by: agent (autonomous, `close-data`)

## Context

ADR-0082 §3 gave `write file` its content spelling: a command whose input is bytes or text —
`input: "bytes | string"` — consumes the pipeline as its `content` argument and takes its targets
from the selector instead. The implementation asked a weaker question than that:

```rust
if ctx.has_input() && !contract.input().is_stream() { … collect_content(…) }
```

`is_stream()` is false for `input: "null"` as well, so every mutation that declares no stream
input at all fell into the content path. `get interface lo | stop interface` answered

```text
Ono-Sendai-E0201 type.mismatch `stop interface` writes bytes or text, and a record is neither
```

— a refusal about a representation nobody asked for, for a command that writes no content at
all. The object-in spelling of spec §11.5 and §14.3, which works for `unmount filesystem`,
`stop process` and every other mutation, was unreachable for five commands.

## Decision

**The content path requires a contract that admits content.** A mutation reads the pipeline as
its `content` argument only when its declared input admits `bytes` or `string`. Every other
mutation takes the objects arriving on the pipeline as its targets, whatever it declares.

**Five contracts now declare the stream they accept**, so the surface says what the shell does:
`ono.interface.start`, `ono.interface.stop`, `ono.interface.remove` accept
`null | stream<ono.interface/1>`, and `ono.mount.start`, `ono.mount.stop` accept
`null | stream<ono.mount/1>`. A verb that *creates* what it names — `add interface`,
`add route`, `mount filesystem` — keeps `input: "null"`: there is no object yet to pipe in.

## Consequences

- `get interface lo | stop interface`, `get interface lo | start interface`,
  `get mount /mnt | stop mount` behave like every other mutation: one `ActionResult` per piped
  object, carrying that object as its target.
- A mutation that genuinely takes content is unchanged: `… | write file /tmp/x` still collects
  the stream, and piping a record into it still gets §12.3's honest refusal.
- The five contracts are a widening — a command that accepted nothing on stdin now accepts a
  stream — so no consumer breaks.

## Alternatives considered

- **Fix only the contracts.** Rejected: it leaves the same wrong refusal waiting for the next
  mutation someone declares with `input: "null"`, and the code would still be deciding "content"
  from the absence of a stream rather than from the presence of content.
- **Refuse a piped stream where the contract declares `input: "null"`.** Rejected: that is the
  honest reading of the type, but it would withdraw the object-in spelling from eleven further
  commands that answer it correctly today, to no user's benefit.
