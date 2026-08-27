# ADR-0050: `view` — the interactive browser, and what Phase J does not build

- Status: accepted
- Date: 2026-08-26
- Spec refs: §6.4, §13.5, §13.6, §17.4, §20.3, §31.53–§31.55, §37 J, §40.1
- Decided by: agent (autonomous)

(Numbered 0050: 0036–0039 were reserved by the phase H agent and 0040–0049 by the phase I
agent working in parallel; monotonicity holds, the gap is deliberate.)

## Context

Phase J's list — navigable graphs, multi-pane inspect/watch, timeline exploration, object
pickers, remote-link overview — is prefixed by its own discipline: *"deliver only where
semantics justify them."* §13.5 writes selection as MAY; §31.55's `proc-space` sketch shows the
shape the spec wants: a view is opened explicitly, receives objects, never owns the data, and
falls back to plain rendering on redirection.

## Decision

### One verb, `view`, per the §40.1 review

The spec already spells it — `get process | view proc-space` (§31.55), and §7.1's verb table
carries `view` as "specialized renderer/TUI, view-scoped, read-only UI/object access". No
existing verb covers an interactive consumer: `format` renders and returns, `inspect` describes
one value. `view` is a consumer that owns the terminal until dismissed, mutates nothing, and is
the single surface KUANG/11 view plugins later register into (§31.53) — one vocabulary for
built-in and contributed views.

### What `view` delivers, and how that covers Phase J

`ono.data.view` takes the stream and a view name. `view table` is the **object picker**: a
cursor over the rows, moved with the arrows, and — this is the point — **leaving the view keeps
the selected row as `@`** (§6.4), so `get process | view table` then `@ | inspect` acts on what
was picked. ADR-0033's positional addressing gains its cursor exactly as promised: the cursor
*sets* what bare `@` means. Enter toggles the **inspect pane** — the selected object's fields
beside the collection — which is the multi-pane inspect of §37 J in its minimal honest form.
`view tree` renders graph values navigably, which is the **navigable graph** over `trace`
output. The **remote-link overview** is `get link | view table` the moment links exist —
a view, not a feature.

Selection can never change pipeline data (§13.5 MUST): the view consumes an already-produced
stream and emits nothing.

### The fallback is the law of §17.4, not a convenience

Where stdout is not a terminal, `view` renders the same values plainly and deterministically —
§31.55's own fallback rule — so a script that inherited a `view` keeps working and nothing
interactive hides in a pipeline (§17.4).

### What Phase J deliberately does not build, and why

- **Timeline/history exploration** (§20.3, MAY): Ctrl-R search and `history` cover the
  semantics; a timeline adds presentation over the same records. "Only where semantics justify"
  cuts it. It becomes justified when links multiply contexts — revisit with Phase H's wiring.
- **Multi-pane *watch* views**: the live table already updates in place; a split-pane watch
  adds arrangement, not semantics.
- **Full-screen cyberspace theatrics**: §37 J's closing sentence says the feeling "emerges from
  actual system data rather than from a theme" — the deliverable is addressability of what is
  on screen, which `@`-selection is.

## Consequences

- `@` bare stops being an error after a view set it; its help still names the two sources.
- The view loop lives in ono-cli beside the live view, reusing the editor's raw-mode terminal
  and the renderer's styled lines; a KUANG/11 view later receives the same stream over the
  plugin protocol instead (§31.54's capability discipline unchanged).
- Tests drive a real PTY: pick a row, see the pane, quit, act on `@`.

## Alternatives considered

- **Make tables interactive after every command** (§13.5's "rendered collection MAY expose a
  cursor"). Rejected for now: a modal state after every command changes what Enter means at the
  prompt, and §4.4 forbids UI that gets in the way of typing. An explicit `| view` keeps the
  contract obvious. Revisitable without breaking anything.
- **A `--interactive` flag on `format`.** Rejected: format's contract is "returns text";
  a consumer that owns the terminal is a different thing, and §31.55 already names it `view`.
