# ADR-0466: A store that will not parse authorizes nobody, and says which kind of nothing it is

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.3, §9.1, §9.2, §9.3, §56.2, §56.3, §59.5, §65.2, §65.4; ADR-0015 T5/T6,
  ADR-0355, ADR-0437
- Decided by: agent (autonomous)

## Context

ADR-0437 made the listening side demand a client certificate and said in as many words what it
left open: "a listening agent today authenticates every client and authorizes all of them". §9.1
is the gap in one sentence — "a valid client certificate proves only that the connecting process
holds a private key. It does not prove that the agent operator wants to expose system data or
actions to that key."

The store that closes it is fixed by §9.2 down to its path and its failure mode:
`~/.config/ono/authorized_clients`, human-readable, line-oriented, strictly parsed, and "a
malformed authorization store MUST NOT be treated as empty and MUST NOT cause the agent to fall
back to permissive access". §65.4 names the fail-open variant as a failure mode with a name.

Three questions had to be answered: **where the type lives**, **what one line looks like**, and
**what a reader does with a line it cannot understand**.

## Decision

**`AuthorizedClients` lives in `ono-protocol`, beside `TrustStore`, and the path policy lives in
`ono-cli`.** §56.2 gives the protocol crate "trust-decision primitives" and forbids it "filesystem
path policy"; §56.3 gives the CLI "locating identity/trust/authorization files". That is exactly
the split ADR-0355 already made for `trusted_hosts`, so the second store is the same shape as the
first and a reader who knows one knows the other. `crate::trust::AUTHORIZED_CLIENTS_FILE` and
`HostSources::authorized_clients` are where the reference path of §9.2 is written down, once.

**One line is a fingerprint and then `name=value` fields.**

```text
sha256:1f0c… observe=true actions=process.signal,service.manage label=deploy
```

`observe` is **required**, because leaving it out would make the weakest grant depend on a default
nobody wrote down, and §9.4's default belongs to the command that adds a client rather than to the
parser that reads one. `actions` and `label` are optional; an absent `actions` and an empty
`actions=` are the same empty allowlist, because a person editing the file by hand will reach for
the second.

**A malformed non-comment line refuses the whole file**, with
`remote.authorization_store_invalid` (E1204) naming the file and the line number. Not the line:
the file. A reader that skipped what it could not understand would authorize whatever the
remaining lines happened to say, and the line most likely to be mistyped is the one that restricts
something. §9.3's "unknown fields MUST be rejected" is the same rule one level down, and it is
enforced without a version field, because the file has no schema-evolution rule yet and §9.3
permits unknown fields only where one exists.

**A missing store and a corrupt store are different conditions with the same effect.** Both
authorize nobody. `AuthorizedClients::is_present` tells them apart, and the two refusals read
differently: a client that is not listed gets `remote.unauthorized` (E1202) whose message says
whether there is a list at all, and a store that will not load gets E1204 naming the line. An
operator sent to the wrong one of those two places wastes an afternoon.

**A corrupt store stops the agent before it binds.** §2.3 — "if Ono claims that a safety control
is applied before an operation, failure to apply that control MUST prevent the operation from
starting" — and §59.5 — "deterministic startup/configuration failure". `serve_authenticated` loads
the store before `TlsListener::bind`, so there is no window in which the agent is reachable while
it decides. It loads it again per accepted connection (ADR-0470), and a store that stops parsing
while the agent runs refuses every new connection rather than admitting one.

## Consequences

Easy: every path that could authorize anybody goes through one function that can only answer with
a listed client or a refusal, so §65.4 has no doorway. The reference path is one constant and the
format is one parser, so `get client-key` and the agent cannot disagree about what a file says.

Hard: a v0.4.0 listening agent served everyone who could reach the port, and a v0.4.1 one serves
nobody until an operator runs `add client-key`. That is the compatibility break §4.2 asks for
("MUST fail safely rather than silently downgrade"), and the agent says so on stderr the moment it
starts with no store. Every existing test and acceptance case that made a direct link had to
authorize its own client key first, which is the correct amount of friction and was six lines.

Also hard: the strict parser means an operator who fat-fingers one line loses the whole policy
until they fix it. That is the trade §9.2 chose deliberately, and the diagnostic names the line.

Encoded by: `crates/ono-cli/tests/authorized_clients.rs::should_parse_the_documented_entry_model_including_an_empty_action_set`,
`::should_reject_an_unknown_field_in_an_authorization_entry`,
`::should_fail_to_load_the_store_when_one_non_comment_line_is_malformed`,
`::should_never_treat_a_malformed_store_as_an_empty_one`,
`::should_distinguish_a_missing_store_from_a_corrupt_one`, case `187`.

## Alternatives considered

**A JSON or YAML store.** Free parsing, free unknown-field rejection with `deny_unknown_fields`.
Rejected: §9.2 requires "human-readable, line-oriented", and the neighbouring trust store is
already three whitespace-separated fields per line. A second format for the second half of the
same subject would make the pair harder to hold in the head than either alone.

**Skip the malformed line and load the rest, with a warning.** What every `sshd_config` reader
does. Rejected in one sentence by §9.2, and rightly: the line an operator most often mistypes is
the one that says `observe=false`.

**Read the store as empty when it will not parse.** Reads as the safe direction, and is the
doorway to §65.4: the next thing a fail-open reader does is fall back to a default, and an empty
store is indistinguishable from a store nobody has written.
