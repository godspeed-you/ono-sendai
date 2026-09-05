# ADR-0024: `watch`, events, and what a live view may do to the screen

- Status: accepted
- Date: 2026-08-26
- Spec refs: §4.4, §4.6, §11.2, §13.5, §18, §31.14, §31.15, §34; ADR-0013, ADR-0014
- Decided by: agent (autonomous)

## Context

Spec §18 asks for `watch`, an event/snapshot model, in-place rendering, native background jobs
and stable object identity, and §18.3 gives the behaviour that makes it worth having: "A process
record keyed by PID can update CPU/memory fields without printing a new table each interval."

Everything in that sentence is a decision. What counts as the same row; what happens when the
terminal is not a terminal; what happens when the provider produces faster than the screen can
show; and what `watch process | to json` means when nobody is watching.

## Decision

### Sameness is `ObjectId`, and nothing else

A row updates in place when the new value's `ObjectId` matches the old one's. `ObjectId` is the
schema plus the identity fields the schema declares (`ono-provider-api`), so a process is
`(pid, started)` and a recycled pid is a **different row**, not the same one changing. A schema
with no declared identity yields no `ObjectId`, so its values are appended rather than updated —
a projection's rows are values, not objects, and updating one in place would claim two unrelated
results were the same result.

### A subscription always begins with a snapshot

Spec §31.14's three primitives are `snapshot`, `subscribe` and `watch`. A subscription emits the
current state as `EventKind::Snapshot` events before any change, so no consumer has to reconstruct
the starting point. `watch` is the runtime-managed combination: subscribe where the provider can,
poll where it cannot — and **which one it did is reported**, because spec §18.2 requires polling
to be explicit rather than a cost that is invisible until someone profiles it.

### In-place rendering is a *presentation*, and only a terminal gets it

Spec §4.6 already fixes this and §18.3 restates it: a terminal gets rows updating in place; a
pipe and a file get the ordinary event and snapshot values, one after another, deterministically.
The values are identical either way — only the number of characters differs. `watch process |
to json` is therefore a well-defined stream of events, not a broken table.

Spec §4.4 forbids animation that blocks input and forbids artificial delay. A redraw happens when
a value changed, and at most at the configured interval; a tick that changed nothing redraws
nothing.

### Overflow is a policy, and the honest policies are named

Spec §31.15 lists `block-upstream`, `drop-oldest`, `drop-newest`, `coalesce` and `fail-stream`.
A live view's default is **coalesce by object identity**: two updates to one object between two
frames are one update, because the screen can only show the newer one anyway, and coalescing is
lossless with respect to what is displayed.

A `watch` feeding a *pipe* defaults to **block-upstream**, because a consumer that is reading
every event needs every event, and dropping some silently would make the stream a lie. Where a
provider cannot be blocked, the policy is `drop-oldest` **and the drop is reported on the error
channel** — a gap a consumer knows about is recoverable; one it does not is not.

### Cancellation is the only way a watch ends

A live query runs until it is cancelled: Ctrl-C, `detach`, the job being killed, or the
consumer going away. The cancellation propagates through the pipeline (ADR-0013) and stops the
provider's subscription, so a detached watch that nobody reads does not keep a netlink socket open
forever.

### A backgrounded watch is a job, not a hidden thread

`watch service nginx &` becomes a job in the same table as an external command (spec §18.4), it
appears in `get job` and in the prompt's job segment, and `fg` brings its rendering back to the
foreground. A live view the user cannot see, list or stop would be the worst kind of background
work.

## Consequences

Easy: one identity rule serves the renderer, the event model and the safety check that stops a
signal reaching a recycled pid; a live view degrades to a plain event stream with no special case;
a watch cannot outlive the thing that wanted it.

Hard: coalescing means an interactive `watch` does not show every intermediate value. That is
what a screen can do, it is stated rather than discovered, and the piped form shows everything.

Must be revisited in phase I: a plugin's subscription is subject to the same policies, and
spec §31.15 gives the host final authority over a plugin's preferred overflow behaviour.

Encoded by: the watch tests in `crates/ono-cli`, the event tests in `crates/ono-provider-api`, and
the acceptance cases for `watch` in a terminal and through a pipe.
