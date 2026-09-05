# ADR-0572: A view is a tree the host draws, and the package drives it by events

- Status: accepted
- Date: 2026-09-03
- Spec refs: §31.2, §31.12, §31.27, §31.28, §31.62, §31.67, §31.74; §13.5, §17.4, §50; ADR-0015, ADR-0022, ADR-0050, ADR-0567, ADR-0568
- Decided by: user and agent

## Context

Issue #3 (c), the last domain: `views`. `protocol.v1.yaml` declares `views.open`, `views.submit`
and `views.close` from the package and `view.mount`, `view.event` and `view.unmount` from the
host; `contributions.v1.yaml` declares what a view is — an id, what it accepts, `interactive`
or `static`, its key bindings, a deterministic `fallback` for redirected output — and the
thirteen components a tree may be built of. None of it was served, and a manifest that
contributed a view was refused at load. Asked whether the non-interactive subset would do, the
user chose the full lens.

## Decision

**1. The package submits trees; the host owns every byte on the terminal.** A tree is JSON of
the thirteen components, nested through `Tabs` and `Split`. The host validates it — a component
outside the list, or a tree deeper than a screen could show, is `view.protocol_error`, and the
view is torn down with the terminal restored — sanitises every string (ADR-0015 T1, T2, T9),
lays it out for the terminal's size, and paints it on the alternate screen in raw mode. No
package text reaches the terminal unsanitised, and no host call carries an escape sequence.

**2. The package drives the view by events, and the host owns the exits.** After `views.open`
answers with the handle, the host sends `view.mount` with the size; every key press, resize,
focus change and cancellation goes to the package as `view.event`, and the package answers by
submitting a new tree — selection, scrolling and paging are the package's state, so the host
never guesses what a key means. Two keys are the host's whatever the package does: `Esc` and
`Ctrl-C` send `cancel`, and if the package has not closed the view within the invocation's call
deadline the host closes it and restores the terminal (§31.27: a buggy plugin cannot leave the
terminal in raw mode). A view is closed when its invocation ends, however it ends.

**3. Redirected output never mounts.** When standard output is not a terminal, `views.open`
answers `{handle, mounted: false}`, `views.submit` is accepted and discarded, and the package
emits its declared `fallback` instead — the determinism §31.28 and §50 require, decided in the
host rather than trusted to the package.

**4. The supervisor holds the lifecycle; the shell holds the terminal.** The supervisor keeps
the view table, checks `ui.view` on every call, validates trees, and forwards events; what
draws is a `ViewHost` the loader is handed, like the context source and the host services
before it. The shell's `ViewHost` runs the terminal loop on a thread of its own while the
package's invocation is in flight; the test host's records every tree and injects events, so
the conformance suite proves the lifecycle without a terminal (§31.73).

**5. The SDK reads events without a host call.** Events arrive as requests from the host while
the package's command is running; the SDK answers them and queues them, and `next_view_event`
hands them to the command in order. Nothing in the contract had to be added.

## Consequences

- A package contributes a view in its hello beside its commands, and the manifest's view
  declarations load; `inspect plugin` lists them. The example package contributes an
  interactive table of its items with `q` to close and `enter` to inspect.
- The thirteen components each have one text layout: a table through the shell's own table
  renderer, a tree through its tree renderer, key/value pairs, a log window, a sparkline of
  block characters, a gauge, tabs with one body, a split of two panes, a graph as its edges,
  a command palette and an object picker as filtered lists with a cursor, and a status line.
  The layout is the host's; a package asks for a component, never for a position.
- With this, every call `protocol.v1.yaml` declares is either served or answered with the
  honest refusal of ADR-0573, and issue #3 closes.

## Alternatives considered

- **Host-side selection state** (the host tracks the cursor, the package gets `selected`
  events). Rejected: it would make the host guess which component a key belongs to, and a
  package's own state is what §31.28 asks it to keep anyway.
- **A view as terminal ownership with a size contract.** Rejected: §31.2 makes injecting escape
  sequences a non-goal, and a view that owns the terminal is exactly that.
