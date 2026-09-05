# ADR-0570: A program runs under the host's confinement, and a connection is a stream

- Status: accepted
- Date: 2026-09-03
- Spec refs: §31.12, §31.15, §31.20, §31.21, §31.37; v0.4.1 §16.1; ADR-0015, ADR-0283, ADR-0567, ADR-0568
- Decided by: agent (autonomous)

## Context

After ADR-0568, `process.exec`, `network.connect`, `network.request`, `network.listen`,
`network.close` and the `views` domain were still absent. Two of them can be served with what
the shell has; the rest cannot be served honestly yet, and this ADR says which is which.

## Decision

**1. `process.exec` runs the program under the confinement a native plugin runs under.** The
supervisor resolves the program before the check and checks the resolved path against the
`programs` glob scope and its file name against the `executables` list (ADR-0015 T11); the
shell then spawns it through the same confined spawn a native package gets — the rlimits, the
descriptor hygiene, the session, no inherited environment — in a directory of its own, with only
the environment the package gave it. Its standard output and error come back as a stream of
`{stream, line}` values and end with `{exited: code}`, so a package reads a program the way it
reads any host stream. A byte-exact stream and a stdin handle are later work.

**2. A connection is a stream in both directions.** `network.connect` checks the host against
the `hosts` scope and the port against the `ports` scope, audits the destination either way,
and opens a TCP connection the shell holds; the package reads it with `streams.next` — chunks
as `{bytes}` — and writes it with `streams.emit`, whose values are bytes or text; `network.close`
drops it. The package never receives a descriptor (§31.21). `tcp` is the transport this build
carries; `tls` and `udp` answer `provider.unavailable`.

**3. What is declared and not served says so.** `network.request` needs an HTTP client behind
the operator's trust store and `network.listen` needs accepted connections delivered as handles
inside a stream; `views.open`, `views.submit` and `views.close` need the view runtime that
`view.mount` on the plugin side would drive, which the shell does not have. Each answers
`provider.unavailable` with the brokered path that exists, rather than a stub that pretends.

## Consequences

- Fourteen of §31.12's sixteen domains have at least one served call; `views` and the two
  unserved network calls are the remainder, and the issue stays open for them.
- The example package gained `exec` and `connect`; the conformance suite proves a program inside
  the scope with its output and exit status, one outside refused before the host, a connection
  inside the scopes echoing through both stream calls, and a port outside refused and audited.
  Through the binary, `/bin/echo` runs under the real confined spawn and a loopback listener
  answers through the broker.

## Alternatives considered

- **Handing the package the child's descriptors.** Rejected: §31.21 forbids a socket descriptor
  crossing, and a program's pipes are no different.
- **A `network.send` call beside `streams.emit`.** Rejected: the contract has no such call, and a
  connection that is read as a stream is written as one.
