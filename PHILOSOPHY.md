# Ono-Sendai Philosophy

`README.md` says what Ono-Sendai is. The [Wiki](https://github.com/godspeed-you/ono-sendai/wiki)
says how to use it. `docs/` says what it formally guarantees. This document says **why it
behaves the way it does** — the principles that decided the interface, and the things they
forbid.

It is written to be used. When a new command, a new provider or a new rendering has to be
designed, the argument for it should be findable here; and if a proposed feature contradicts a
principle below, that is the finding, not a detail to be smoothed over.

Each principle is stated the same way: what it is, why it matters, what the interface therefore
does, and what Ono-Sendai therefore refuses.

---

## The machine is already structured

**The principle.** A process is a process. A socket is a socket. A service is a service. The
kernel knows this before anything is printed — pid, uid, cgroup, state, the inode behind the
socket, the unit behind the pid — and it knows it precisely.

**Why it matters.** Classical Unix tooling takes that structure, flattens it to characters at
the process boundary, and then spends an entire ecosystem — `awk`, `cut`, `sed`, `jq`, and
regexes that break the day someone adds a column — rebuilding what the machine held a moment
earlier. The reconstruction is lossy, unversioned, and specific to one tool's output format on
one version of one distribution.

**What Ono-Sendai does.** Native commands ask the system directly — procfs, netlink, D-Bus, the
systemd APIs — and emit typed objects against published schemas in `docs/spec/schemas/`. The
table you see at the terminal is a *rendering* of a value, produced last, for you.

**What it refuses.** It does not parse a tool's human-readable output and call the result
structure. Where a foreign program has a machine-readable mode, an adapter negotiates that mode
explicitly and says so; where it has none, the bytes stay bytes.

## Structure survives the pipe

**The principle.** What flows between two stages of a pipeline is the object, not its printed
form.

**Why it matters.** The moment a pipeline stage receives text, every stage after it is doing
forensics. Field names, types, units and null-ness are all gone, and the next author guesses
them back. PowerShell proved the fix a decade before this project existed; the fix is right, and
the argument against it was always the ceremony, never the idea.

**What Ono-Sendai does.** `get process | sort memory desc | take 3` moves `stream<ono.process/1>`
through three stages. `memory` is a quantity with a unit, not the string `3.21 GiB`, so sorting
it is arithmetic and not luck. Rendering happens once, at the end, and only when something is
there to render to.

**What it refuses.** No stage secretly serialises to text and re-parses. A value's identity,
type and provenance are inspectable at every point — `inspect` and `type` are part of the
language, not a debugging aid.

## Unix remains underneath

**The principle.** Running an arbitrary executable is trivial. Always. This is not a
compatibility mode, a bridge or a fallback — it is the floor.

**Why it matters.** A shell that asks you to abandon `git`, `grep`, `ssh`, `make`, `less` and
thirty years of muscle memory is not a shell you can set as your login shell. Adoption is not a
marketing problem here; it is a design constraint. A systems interface you cannot live in is a
demo.

**What Ono-Sendai does.** `git log --oneline | grep fix` behaves exactly as it does in bash,
byte for byte, including quoting, job control, signals and exit status. When a *typed* consumer
follows a supported tool, an adapter rewrites the invocation to that tool's own machine-readable
form and decodes it into the same schemas the native providers use — visibly, with `explain`
able to show every step of the rewrite.

**What it refuses.** It never silently changes what a foreign program does. `raw` bypasses the
adapter layer unconditionally; `adapt` demands structure and fails loudly rather than guessing.
Text tools — `grep`, `sed`, `awk`, editors, pagers — stay raw by design, because pretending they
are structured would be the lie this whole project exists to avoid.

## One language, many system domains

**The principle.** Processes, files, sockets, services, users, mounts, containers and remote
hosts are addressed with one grammar and one controlled verb registry — `get`, `set`, `where`,
`sort`, `enter`, `trace`, `watch`, `link` — not with thirty separate command languages.

**Why it matters.** The cost of classical Unix is not any single tool; it is that each tool is
its own dialect, with its own flags, its own idea of a column and its own failure conventions.
The knowledge does not compound. Learning `ps` teaches you nothing about `ss`.

**What Ono-Sendai does.** A verb means the same thing everywhere it is accepted, and the
registries in `docs/spec/` are the contract that keeps it that way — the shell's own `help`, the
completion metadata and the generated reference pages all come from the same source, so they
cannot drift apart.

**What it refuses.** No per-domain special grammar; no verb that means one thing for processes
and another for sockets. A domain that cannot be expressed in the common language is a signal
that the language is missing something, not a licence to add a dialect.

## The system is a place

**The principle.** A machine has a geography. Objects sit somewhere, have neighbours, and are
reachable by walking rather than by knowing their name in advance.

**Why it matters.** Most system investigation begins without the name of the thing. You know a
symptom and a coordinate: *something* is listening on 8080; *something* is eating the disk;
*something* holds that file open. A shell whose only addressing mode is "type the identifier of
the thing you are looking for" makes you solve the problem before you may use the tool.

**What Ono-Sendai does.** `find place --where local.port == 8080` finds the listener before
anyone knows it is nginx. `look` says where you are and what the exits are; `near` lists what is
next to you; `enter`, `follow` and `jump` move; `back`, `up`, `home` and `trail` deal with
where you have been and where a thing belongs. The prompt is a location URI because you are, in
a real sense, somewhere.

**What it refuses.** Hierarchy and graph are never blurred. `up` walks where a thing *belongs*;
`back` walks where *you* have been; `follow` walks a relationship an observer *asserted*. Three
different questions, three different verbs — collapsing them into one convenient "go there"
would be the kind of helpfulness that produces wrong conclusions.

## Relationships are first-class

**The principle.** The edge between two objects is a value, with the same standing as the
objects it joins.

**Why it matters.** Almost every real question is relational. Which process holds this socket.
Which unit owns this pid. What dies if this mount goes away. Classical tooling answers these by
correlating two text tables by eye.

**What Ono-Sendai does.** `trace` asks the providers what they can assert about an object — its
parent, its children, the sockets it listens on, the files it holds — and returns a graph value
that flows through the pipeline like any other value: filterable, pipeable, inspectable.

**What it refuses.** A graph is never rendered as a picture you cannot address. If it can be
drawn, it can be piped.

## Relationships must be observed, not invented

**The principle.** Every edge names who asserted it, how they know, and how sure they are.

**Why it matters.** An invented edge is worse than a missing one. A shell that infers "these two
things are probably related" and displays the guess with the same weight as a kernel fact will
eventually be believed at exactly the wrong moment — during an incident, by a tired person, at
04:00.

**What Ono-Sendai does.** Edges carry provenance and confidence. An edge the kernel asserts and
an edge a heuristic suggests do not look alike, and the difference survives into the pipeline
where a later stage can filter on it.

**What it refuses.** No topology assembled from plausibility. No "related items" panel. No
correlation presented as causation because it made the diagram tidier.

## Identity must mean more than an integer

**The principle.** A pid is not an identity. A process is a lifetime.

**Why it matters.** Pids are reused. A tool that remembers "process 4127" and acts on it later
may act on a completely different program, and the failure mode is silent and destructive — you
signalled the wrong thing and nothing told you.

**What Ono-Sendai does.** Objects carry a lifetime descriptor, not just a kernel integer. A
place you visited that then exits becomes a **tombstone**: visibly dead, still reachable through
`back` and `trail`, and safe from whoever inherits its number. When two candidates answer the
same reference, the shell stops and names them both instead of picking.

**What it refuses.** It does not resolve an ambiguous identity for you, and it does not quietly
re-point a stale reference at a live object that happens to have the same number.

## Uncertainty must remain visible

**The principle.** Unknown is a value. It is `null`, and it is never a zero, an empty string, a
dash or an average.

**Why it matters.** Fabricated data is indistinguishable from measured data once it is in a
table, and every downstream calculation inherits the fabrication. "0 connections" and "we were
not allowed to look" lead to opposite decisions.

**What Ono-Sendai does.** A field a provider could not read is `null` and renders as such. Every
neighbourhood carries one of six defined states, so a door you may not open renders as a locked
door and an empty room renders as an empty room. Provenance says which host, which provider and
how fresh.

**What it refuses.** No filling gaps with plausible defaults. No silently dropping the rows a
scan could not see. No aggregate that hides the fact that half its inputs were unavailable.

## Danger must be visible before damage

**The principle.** Destructive scope, elevated privilege and remote targets announce themselves
*while there is still time to stop* — not in the confirmation prompt, and certainly not in the
postmortem.

**Why it matters.** The dangerous moment in a shell is not pressing Enter; it is the minute
before, when you believe you are on the laptop and you are on production. The state that makes a
command catastrophic is context you already lost.

**What Ono-Sendai does.** The prompt expresses the real context: which host, which privilege,
which scope. `explain <pipeline>` shows what a command would do, in full, without doing it.
Destructive verbs are named as such in the registry rather than discovered by their effects.

**What it refuses.** It does not habituate you to a modal "are you sure?" that everyone learns
to dismiss. It does not hide a remote context behind a prompt that looks local.

## Discoverability is part of the language

**The principle.** `help`, completion, `explain`, `inspect` and `type` answer from the same
registries the shell dispatches on. Documentation is not a parallel artifact that describes the
shell; it is generated from what the shell actually is.

**Why it matters.** Every shell's real manual is the one in the reader's head, assembled from
half-remembered flags. Documentation that can drift from behaviour will drift, and then it
teaches errors confidently.

**What Ono-Sendai does.** Completion is semantic: it knows a verb's targets and a target's
fields. The generated reference under `docs/reference/` and the shell's own `help` have one
source, and `spec-check` fails the build on drift between contract and implementation.

**What it refuses.** No stable command without published metadata. No documented example that is
not executed by the gate — including the examples in `README.md`.

## No fake intelligence

**The principle.** The shell does not guess intent, and it does not present a guess as a fact.

**Why it matters.** The most damaging thing a systems tool can do is be confidently wrong about
what you meant, in a domain where the actions are irreversible.

**What Ono-Sendai does.** It answers what was asked, from data it actually has, with provenance
attached. Where an assistant or an analysis extension produces an interpretation, it is labelled
as an interpretation and isolated by the KUANG/11 capability model — it cannot quietly become
part of the object graph.

**What it refuses.** It never derives destructive intent from vague natural language. It does not
autocorrect a command into a different command. It does not summarise a system's state into a
verdict it cannot defend.

## Reveal the machine, don't decorate it

**The principle.**

> **The machine is already strange enough. Reveal it.**

Ono-Sendai is named after the cyberspace decks in Gibson's Sprawl, and that inheritance is a
constraint, not a licence. A deck is worth having because it shows you the system as it actually
is. The atmosphere is supposed to be a *side effect of the truth*.

**Why it matters.** Every decorative element competes with real information for the reader's
attention, and, worse, teaches them to discount the display. Once a terminal has cried wolf with
a fake scan, the real one is noise too.

**What Ono-Sendai does.** Allowed: terse system vocabulary where the word is *accurate*; live
tables whose movement comes from real events; graph views of real relationships; latency
indicators on real links; a prompt that expresses real location; dark, restrained themes.

**What it refuses.** `ACCESS GRANTED` on a successful `ls`. Fake scanning progress. Matrix rain.
Random glitches. Boot animations. Hexadecimal noise. Artificial keystroke delay. Sound effects.
Failure messages written like a video game. Spinners that spin while nothing is happening.

A live object stream looks alive because processes are dying in it. A dependency trace feels like
walking into a machine because the edges came from the kernel. The prompt creates a sense of
place because you actually are somewhere. Every effect in this shell is a side effect of telling
the truth about the system — which is also why none of it can be turned into a screensaver.

---

## What these principles mean for the interface

Read together, they produce a fairly small set of interface rules:

- **Data and rendering are separate concerns.** Output is deterministic when it is redirected;
  the pretty table is what happens when a terminal is present, not what the pipeline contains.
- **Errors are values.** Structured, typed, inspectable, with a code from a published taxonomy —
  and a human rendering on top of the value, never instead of it.
- **The active context is always visible**, and local and remote share one mental model. A remote
  machine is another place in the same world, not a second product mode with its own language.
- **Extensions are guests, not co-authors.** KUANG/11 packages contribute real objects and real
  relationships to the same typed pipeline as native commands, under declared capabilities,
  brokered host calls, process confinement and an audit trail — because an extension that can
  silently invent an edge breaks the honesty guarantee everything else rests on. And the guest
  list is honest too: the native tier confines a plugin's process, it does not isolate it from the
  filesystem or the network, and it says so rather than borrowing the word "sandbox".
- **Configuration cannot execute.** `~/.config/ono/config.ono` sets values, functions and
  aliases; it cannot run commands at startup.

## What Ono-Sendai deliberately refuses to do

Stated plainly, so that "we could add it" has an answer:

- It does not parse unstable human-readable output and call the result structure.
- It does not fabricate a value it could not read, in any direction — no zeros, no defaults, no
  interpolation.
- It does not assert a relationship no provider observed.
- It does not resolve an ambiguous identity on your behalf.
- It does not act on a guess about what you meant.
- It does not perform activity it is not performing.
- It does not require you to abandon Unix to use it.
- It does not become a graphical file manager, an IDE, or a monitoring product. Those are
  non-goals in the specification (`docs/ono_sendai_shell_spec_v0.2.md` §38) and they stay
  non-goals.

---

The formal, checkable version of all this is elsewhere: the behaviour Ono-Sendai guarantees is in
the specification and the machine-readable contracts under `docs/spec/`, and every place an
implementation decision resolved an ambiguity is an ADR in `docs/decisions/`. This document is
the reasoning those artifacts encode.
