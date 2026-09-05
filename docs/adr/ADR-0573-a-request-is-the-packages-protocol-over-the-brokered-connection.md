# ADR-0573: A request is the package's protocol over the brokered connection

- Status: accepted
- Date: 2026-09-03
- Spec refs: §31.12, §31.15, §31.21, §31.37; ADR-0566, ADR-0568, ADR-0570
- Decided by: user and agent

## Context

`protocol.v1.yaml` declares four network calls. ADR-0570 served `network.connect` and
`network.close` and left `network.request` and `network.listen` unserved, each with a reason: a
request needs an HTTP client behind the operator's trust store, a listener needs accepted
connections delivered as handles inside a stream. Asked which way to go, the user decided that
HTTP is a package's business rather than the host's, and that the view domain is to be served in
full. This ADR records the network half.

## Decision

**1. `network.request` is not the host's to serve.** A request is a protocol — HTTP today,
whatever else tomorrow — spoken over a connection the host brokers. The trust decision §31.21
puts in the operator's hands is the `hosts` and `ports` scope of `network.connect`, and it is
the same decision for a request as for any other byte stream. So the host carries no HTTP client
for a protocol it does not speak: a package that wants to speak HTTP does so over
`network.connect`, or delegates to a KUANG/11 package that does. The call stays declared, and
the host answers it with `provider.unavailable` naming the brokered path, so a package that
calls it learns where the door is rather than that it is locked. `credential` handles, which the
contract lets a request carry, arrive with the secret store that would issue them.

**2. `network.listen` is served, and every accepted connection is a handle.** The listener is
checked against the `ports` scope and audited; it binds the loopback address, because a package
listening on every interface is a decision the operator has not been asked; the package reads
the listener as a stream whose values are `{connection: handle, peer}`, and each handle is a
connection it reads and writes exactly like one it opened. Closing the listener's handle stops
accepting; the connections already handed over live on until they are closed.

## Consequences

- Fifteen of §31.12's sixteen domains have every call the host will serve; `views` is the last,
  and it is the next tranche.
- A package author who needs HTTP writes it over the brokered connection. That is more work
  than a host call would have been, and it is honest about what the host verified: the
  destination, not the protocol.
- Deferred with the secret store: TLS for `network.connect` (the contract's `tls` protocol,
  where the host performs the handshake and applies the operator's trust store) and the
  `credential` handle. Both answer `provider.unavailable` today.

## Alternatives considered

- **An HTTP client in the host** behind the trust store. Rejected by the user: HTTP is a
  package concern, and the host would ship a client the gate cannot exercise against the world.
- **Binding listeners on every interface.** Rejected: §31.21's brokering exists so that reach
  is the operator's decision, and a port scope says nothing about interfaces.
