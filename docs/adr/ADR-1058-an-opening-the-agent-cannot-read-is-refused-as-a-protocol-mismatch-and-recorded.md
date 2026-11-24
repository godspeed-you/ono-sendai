# ADR-1058: An opening the agent cannot read is refused as a protocol mismatch, and recorded

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.4.1 §12.4, §13.2, §14.1, §54.1; spec §21.2; ADR-1056, ADR-1057
- Issues: #181, #182 (the handshake refusal paths both touch)
- Decided by: agent (autonomous)

## Context

Verifying every handshake refusal path for the semantically correct error at the client and an
audit record at the agent (the brief of the v0.6.6 remote-protocol workstream) found three paths
in `ono_protocol::serve` that did neither:

- an opening frame that is not a `Hello` (a frame of another kind);
- a `Hello` whose payload this build cannot read;
- an opening frame this build refuses at the header (unknown framing version or kind, undefined
  flags, a length over `Limits::max_frame_payload`, §12.4).

Each returned `Err(remote.protocol_mismatch)` and dropped the transport: the peer — authenticated
by mutual TLS on a direct link — met a closed socket and reported `remote.unreachable`, and the
agent's audit trail had no line for the decision. The second case is the one that matters for
ADR-1056's extension rule: a newer client whose `Hello` an older agent cannot parse must learn
that the two ends share no protocol, not that the host is unreachable. A peer that hung up before
saying anything was likewise not recorded.

## Decision

- **An opening the agent cannot read is answered with `Reject` `remote.protocol_mismatch`
  (E0602)**, carrying this side's reading of what it received (the `ProtocolError` sentence: the
  kind it got, the length it was told, the version it found — facts about the peer's own bytes,
  nothing about the agent), written and the peer let go within the bounded refusal of ADR-1057.
  The agent records `connection.protocol_mismatch` with the peer's fingerprint and address, and
  `serve` still returns the mismatch, as it documented.
- **An opening lost to the connection** — the peer hangs up before a frame, or the transport fails
  or ends mid-frame — is recorded as `connection.disconnected` with `remote.unreachable`; there is
  nobody to tell.
- E0602 carries no details over the wire: it is not in the refusal census, so ADR-1056's table
  gives it none.

## Consequences

- Tests: `crates/ono-protocol/tests/opening.rs` (four cases: another kind first, an unreadable
  hello, an oversized header, a peer that hangs up) and, for the client side of a lost
  connection, `crates/ono-protocol/tests/handshake.rs::should_report_an_unreachable_peer_when_its_answer_is_cut_off_mid_frame`.
- An unknown frame kind *after* the handshake still fails the link as a mismatch, which is the
  last line of ADR-1056's extension rule: a peer never ignores a frame it was not offered.

## Alternatives considered

- **Keep dropping silently.** Cheaper, and it turns every incompatible client into an
  "unreachable host" — the misdirection ADR-0439 already rejected for the ALPN break.
- **Answer only a peer that sent a `Hello`-kind frame.** A frame of another kind first is just as
  much "not this protocol", and telling it costs the same bounded write.
