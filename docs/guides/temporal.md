# Time in Ono-Sendai

v0.5 adds a second coordinate to the session. v0.4 gave it a place; this gives it an instant.

```text
Context {
    space: local/service/nginx
    time:  2026-08-31T12:17:00+02:00
}
```

Everything below follows from one rule, and the rule is worth reading twice before the commands:

> **Ono reconstructs only what its evidence supports.** Where a source observed something, Ono can
> say so and say who observed it. Where none did, the answer is *unknown* — not zero, not empty,
> and not a plausible guess.

That is why several things here refuse. A shell that answered every temporal question would be
guessing at most of them, and a guess about what a machine was doing an hour ago is worse than a
refusal, because it is actionable.

## Evidence-backed time

Ono knows about the past exactly as far as something recorded it. Four things can have:

- **the session** — what this shell itself did, and what it watched while it was running;
- **the recorder** — a bounded, opt-in, user-level collector, off until you turn it on;
- **a provider that owns history** — journald is the one that genuinely does on an ordinary Linux
  host; a container engine's event stream is another;
- **a KUANG/11 package** — an external observability store, a filesystem snapshot manager, an
  audit log.

Each observation carries three separate instants, and Ono never collapses them: when the source
says it happened, when Ono saw it, and when it reached the ledger. A remote host's event keeps
its own source time; the local ingest time goes beside it, not over it.

Each observation also carries a **strength**, and nothing in the system can raise one:

| strength | what it means |
|---|---|
| `authoritative` | the source owns this fact for this scope — systemd about a unit's state |
| `asserted` | the source reports it and is not the sole authority |
| `derived` | Ono computed it from stronger evidence by a documented rule |
| `correlated` | a meaningful association, and no more than that |
| `observational` | a sample or a snapshot, with no exhaustiveness guarantee |

## Coverage, and why a question can be refused

**Coverage** is what a source was *able* to observe over an interval, and it is per capability
rather than one flag for everything. A snapshot taken at 12:00 gives you `point_sample` coverage
of process existence at 12:00. It does not give you coverage from 11:50 to 12:10, so it cannot
tell you whether a process existed at 11:55.

This is the difference that makes absence meaningful:

- *unknown* — nothing was watching, so nothing can be said;
- *absent* — something was watching, it would have seen this, and it did not.

Ono says the second only where coverage supports it. §7.4 of the specification is the rule and
`can_prove_absence` is where it lives in the code.

A **gap** is a first-class object, not a hole in the output. If the recorder was down between
12:20 and 12:24, a timeline crossing that interval draws the gap:

```text
12:20:00        ---- coverage gap: recorder offline 4m12s ----
12:24:12
```

It is drawn even though events exist on both sides. A gap that is not shown is the failure mode
this whole design is built against.

## `at` and `now`

```text
local/service/nginx:// > at -10m

historical context
  requested    -10m
  resolved     2026-08-31 12:07:14 +02:00
  coverage     partial

local/service/nginx:// @12:07:14 [PAST?] >
```

`at` takes an instant, a local date and time, a time today, a negative duration, or an event
reference (`at event @e42`). It changes only *when* you are; the place is untouched.

`[PAST]` means the reconstruction for this place is supported. `[PAST?]` means it is materially
partial — some of what you are about to read is unknown, and it will say which. The marker is a
word, so it survives a monochrome terminal and a pipe.

`at` resolves *before* it moves you. An unresolvable selector leaves the coordinate exactly where
it was, and says why:

```text
local:// > at -3d

temporal.not_recorded
no source can reconstruct this scope at the requested time

available:
  session history      42m
  recorder history     disabled
  journald             2d for service events
```

A time in the future is refused — historical context is somewhere Ono has been. `now` returns to
the present, keeping the place if it still exists and reporting the move if it does not.

Every read-only command takes `--at <selector>` for a single question, and it is the same engine:
`get service nginx --at -1h` and `at -1h` followed by `get service nginx` give the same answer
with the same coverage.

## The past is read-only

While the coordinate is historical, every Ono mutation refuses:

```text
local/service/nginx:// @12:07:14 [PAST] > restart service nginx

temporal.read_only
`restart service` cannot execute while observing historical state

return to the present:
  now
```

This covers native mutations, KUANG/11 mutation tools and remote mutations, and it is enforced at
the one place every command passes through rather than in each of them.

An arbitrary external program is refused too, with `temporal.present_only` — not because Ono
knows what it would do, but because it would run against the machine as it is now, which is not
the machine you are looking at. The escape hatch is explicit:

```text
local/service/nginx:// @12:07:14 [PAST] > present git status
```

`present` runs the command in the real current environment, says so in the result metadata, and
leaves your coordinate where it was. Pure Ono transforms over reconstructed values keep working
normally — `get process | where cpu > 20 | select pid name` reads the past and filters it.

## Timeline

`timeline` is the chronological projection, and it returns typed values rather than text:

```text
12:17:51.203  config/nginx.conf       changed
12:18:02.011  nginx.service           reload requested        [ono]
12:18:03.104  process/1827            disappeared
12:18:03.401  nginx.service           active                  [systemd]
```

```text
timeline --since 1h | where kind == "object.changed" | where subject.type == "service"
```

Without a selector it is scoped to where you are standing and the events relevant to it; `--all`
widens it. Each row carries a stable reference — `@e42` — that works in `inspect event @e42`,
`at event @e42` and `why event @e42`.

`find event` searches without you knowing when something happened:

```text
find event 'kind == "action.failed"'
find event 'subject.name contains "nginx" and source_time > now() - 1h'
```

## Changes

```text
local:// > changes --since 10m

ADDED
  process/7128          backup
REMOVED
  process/6902          backup
CHANGED
  service/backup
    state               running -> failed
```

Where one side has no evidence, it says so rather than inventing a number:

```text
filesystem/data.used
  from      unknown
  to        94.1%
  coverage  partial before 12:00
```

## Historical navigation

`look`, `near`, `map`, `find place`, `enter`, `follow`, `jump` and `up` all answer at the active
coordinate. A map at 12:17 is the topology that was reconstructable at 12:17 — an exit that only
exists today does not appear in it, and a relation added afterwards is not drawn.

If the place you are standing in did not exist then:

```text
place not known at requested time
```

with the coverage saying whether that means *known absent* or *nothing was watching*.

Filesystem structure is the honest limit. Ordinary filesystems keep no history, so Ono shows past
directory structure only where a checkpoint, a snapshot provider, sufficient audit evidence or a
KUANG/11 package supports it. It will not show you today's directory and call it yesterday's.

## Rewind

In a live map, `Space` pauses the view's cursor — the providers, the recorder and the machine keep
running; only the view stops. `[` and `]` step through *significant* events rather than every
sample. `Enter` enters the focused object at the cursor's time. `N` returns to now and summarises
what changed while you were away.

Stepping into a gap shows the gap. The map does not keep drawing the last state with a clock that
advances.

## Causation, correlation, and `why`

This is the part most worth understanding, because it is where a systems tool is most tempted to
lie.

Ono distinguishes four relationships:

- **`caused_by`** — a registered rule found evidence of a direct causal link. An Ono action that
  became a systemd job; a job result naming the unit transition it produced; a kernel-sourced
  parent/child creation; a provider's own transaction identifier.
- **`triggered_by`** — A started a mechanism that led to B through steps the source names.
- **`correlated_with`** — a registered correlation rule found a meaningful association, and no
  more.
- **`preceded_by`** — A happened before B under the supported ordering model. Nothing else.

**Temporal proximity never produces a causal edge.** A configuration file changed nine seconds
before a service failed is a correlation, and it stays one however plausible it looks.

`why` needs no model and no network:

```text
> why event @e95

process/2741 appeared at 14:03:12.410

known cause
  nginx.service started a replacement worker

chain
  action @a91: restart nginx.service
      | triggered
  systemd job 4821
      | caused
  nginx.service activating
      | caused
  process/2741 appeared

correlated
  /etc/nginx/nginx.conf changed 6.3s before restart
  correlation only - no evidence that this change caused the restart

evidence
  Ono ActionResult      authoritative for operator action
  systemd job identity  authoritative for unit transaction
  process membership    derived from systemd/procfs
```

And when it does not know:

```text
nginx.service
failed at 14:03:17.004

cause
  unknown

correlated
  14:03:06  /etc/nginx/nginx.conf changed
              11s before failure
              correlation only

coverage gap
  process exit status unavailable
```

**That is a successful answer, not an error.** It exits zero. Unknown cause with the evidence,
the correlations and the gaps laid out is the honest result, and a confident wrong cause would be
worse than useless.

Every causal edge names the rule that produced it, the source it came from and the evidence
behind it. The rules are data — `docs/contracts/temporal/causality.yaml` lists all ten with the
evidence each requires — so you can audit what Ono is willing to claim before it claims it.

An AI assistant may read all of this and produce a hypothesis. A hypothesis is a typed
`Inference` with its model, its inputs and its confidence, and it cannot become a causal edge.

## Ono's own actions

The shell has one source of causal information a monitoring system does not: it knows what you
asked for. Every Ono mutation gets an `ActionId` before it runs, and the lifecycle —
requested, authorized, executed, completed or failed — reaches the ledger. Where an external
authority returns its own transaction id, the mapping is recorded:

```text
ActionId ono:a91f  ->  systemd job /org/freedesktop/systemd1/job/4821
```

That mapping is what makes `why` able to explain a service transition at all.

Ono claims that an external command it ran created the process it forked, because it owns that
fork. It claims nothing about what that process then did, unless an adapter or provider reports
it.

## The recorder

Persistent recording is **off by default**. Turn it on for a session:

```text
local:// > start recorder

recorder running   healthy
  recording            enabled
  store                ~/.local/share/ono/temporal/ledger.sqlite3
  retention            24h / 512.00 MiB
```

To keep recording across sessions, set it in your configuration:

```text
set config temporal.recording.enabled = true
```

`get recorder` reports state, retention, how much is held, the sources it collects from, how many
events were dropped, and — where history stopped — why.

### What it keeps, and what it will not

It collects canonical provider events, the snapshots checkpoints need, relation changes, Ono
action events, coverage markers and landmark transitions.

It does **not** keep, by default: command output, file contents, environment dumps, secrets,
command-line arguments that carry secret values, packet payloads, or unlimited metric samples.

A command carrying something secret-shaped is redacted before it is written — including where the
secret is a separate word (`--token abc`) and where it is a password inside a connection string.
Process command lines are not persisted at all unless you turn on
`temporal.record.process_argv`, which is off and documented as a privacy decision.

The recorder runs with your privileges. It is not setuid, does not ask for sudo, does not run a
privileged daemon, and reads nothing you could not have read through an ordinary Ono provider. It
sees no more because it persists.

### Retention

Bounded by age and by size, whichever removes data first — 24 hours or 512 MiB by default.
Expired history answers `temporal.out_of_retention` and names the earliest instant still held,
which is a different fact from "nothing was recorded" and gets a different code.

### Privacy of the store

The directory is `0700` and the database `0600`, created that way before SQLite opens the file.
`remove temporal-history` clears it, under the same confirmation policy as any destructive
operation.

## When history is unavailable

A corrupted store does not take the shell with it. The affected history is refused rather than
served, the interval it covered becomes a visible gap, and everything that is not a temporal
query keeps working. `get recorder` says what went wrong.

If a migration cannot complete safely, the shell runs with temporal persistence off and an
explicit diagnostic.

## Remote hosts

A linked host may offer current snapshots only, live events, real history, or no temporal support
at all. Negotiation reports which, and a host with no history says so rather than presenting its
present as its past.

Clocks are the interesting part. Two hosts' events are not ordered because their wall clocks
differ by milliseconds — Ono keeps a partial order and orders two events only where the evidence
does: a shared sequence, a shared transaction id, an explicit request and response, a known
causal link. Uncertainty is shown rather than smoothed:

```text
14:03:12.100 +/- 40ms  web01 connection opened
14:03:12.118 +/- 55ms  db01 connection accepted
```

They may be displayed in that order. Nothing claims the first caused the second.

## Plugins

Temporal access is capability-controlled, and current access does not imply historical access:

| capability | what it permits |
|---|---|
| `temporal.read.current` | the present, which every object-reading package already has |
| `temporal.read.history` | retained history, scoped by a window the operator grants |
| `temporal.read.evidence` | the evidence behind a claim |
| `temporal.contribute.events` | contributing events, validated for schema, scope, size and rate |
| `temporal.contribute.causality` | contributing causal rules |
| `temporal.recorder.manage` | starting and stopping the recorder |

A package cannot assert that an object exists outside what it can resolve through providers it
has been granted. Its causal claims are capped at `asserted` — the host will not let a package
declare itself authoritative — and its correlations stay correlations in core rendering.

## What the sources can and cannot do

`docs/contracts/temporal/sources.yaml` is the honest matrix, and it is checked against the
providers on every gate run. The short version for a normal Linux host:

| source | live events | history |
|---|---|---|
| systemd D-Bus | yes, with job identity | no |
| journald | no | yes, within its own retention |
| netlink (link, address, route) | yes | no |
| container engine | yes | yes, within the daemon's log |
| procfs | no — snapshots, diffed | no |
| mounts, filesystems | no — checkpoints | no |

Nothing claims exhaustive event delivery. systemd coalesces property changes, netlink drops under
pressure, the journal rate-limits, and a container engine's log ends when the daemon restarts.
Where a claim would be convenient and untrue, the matrix says no.

No part of this needs root or eBPF. A higher-fidelity privileged source is a KUANG/11 extension,
never a hidden prerequisite.

## Troubleshooting

**`temporal.not_recorded`** — nothing covered that time and scope. The message lists every source
and how far back each reaches. `start recorder` begins retaining history; it does not create
history that was never observed.

**`temporal.out_of_retention`** — it was there and retention removed it. The message names the
earliest instant still held.

**`[PAST?]` where you expected `[PAST]`** — coverage is materially partial. `inspect` on the
result shows which capability is short and which source was asked.

**A gap you did not expect** — the recorder was down, a source dropped events, or the interval is
beyond retention. The gap says which.

**`why` answers unknown** — that is an answer. Read the correlations and the coverage gaps
beneath it; they say what evidence would have been needed.
