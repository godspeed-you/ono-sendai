# ADR-0104: Link definitions, `connect host` and `test host` are the shell's own commands

- Status: accepted
- Date: 2026-08-27
- Spec refs: §6.1, §9.1, §11.5, §14.4, §16.5, §21.1–§21.2, §43 (E0601); ADR-0036, ADR-0037,
  ADR-0068, ADR-0090, ADR-0093, ADR-0103
- Decided by: agent (autonomous)

## Context

`docs/spec/commands/remote.yaml` declares `add`, `set`, `rename`, `remove` and `detach link`,
`connect host` and `test host`. Every other mutation in this shell is a `ProviderMutation`: the
registry resolves the target through the provider that serves it and calls the provider's
`act` (ADR-0068). A link cannot be that. The link table is session state — the definition, the
`RemoteLink` once established, the frames standing on it — and a provider is precisely what a
link frame *swaps* (spec §14.4): inside `enter link prod-db`, the registry that answers is
prod-db's. A provider owning the link table would answer `remove link prod-db` from the wrong
side of the link the moment the link was entered. ADR-0090 rejected a provider holding a
session back-reference for the same reason.

`link host` and `enter link` already run in the shell (`context.rs`), as `cd` does, and
ADR-0093 established the seam for a shell-answered command whose values seed the pipeline
(`resolve command`, `get config`, `set config`).

## Decision

### 1. `crates/ono-cli/src/remote.rs` answers the link family through the ADR-0093 seam

`remote::claims` recognises a stage by head word and target — `(add|set|rename|remove|detach,
link)`, `(connect|test, host)` — exactly as `meta::claims` recognises `set config`;
`remote::answer` binds the arguments through the contract (so `--timeout 2s` is a typed
duration and an unknown option is the contract's refusal), acts on the session, and returns
the values that seed the rest of the pipeline: one `ono.action-result/1` per link mutation,
whose `operation` is the command id (`ono.link.remove`), one `ono.probe-result/1` for a probe.
Help, completion, typing and `explain` stay the registry's.

Only the word form is answered: `get link | remove link` (the piped form the contract's
`input` allows) is not, because the seam answers the first stage of a pipeline. `ono.shell`
does not advertise `link.manage` or `link.create`, so nothing binds a `ProviderMutation` that
would run on the wrong side of a link.

### 2. What each command does

| Command | Effect | `changed` |
|---|---|---|
| `add link N --host H --transport T` | records a `SessionLink` with no connection (`state: defined`); `H` defaults to `N`, `T` to `ssh`; an unknown transport or an existing name is refused | true |
| `set link N --host/--transport` | changes the definition; refused (`provider.unsupported`) while the link is established, because the table would then describe a connection that does not exist; no property is a usage error (ADR-0084) | whether a field moved |
| `rename link N M` | renames; refused while a frame stands on `N`, because frames name their link by name (spec §14.4) | true |
| `remove link N` | pops every frame standing on `N`, forgets the link, drops the connection — dropping hangs up (ADR-0036 §8) | true |
| `detach link N` | pops every frame standing on `N`, keeps the link; a one-shot link (below) goes with its frame | whether a frame went |

Refusals are the pipeline's structured failure, never a row that looks like a change (spec
§16.5): a link nothing answers to is `resolve.target_not_found`, a name in use is
`io.already_exists`.

### 3. `connect host` is `link host` plus the frame, minus the persistence

Spec §6.1 writes `connect host prod-db` and the prompt is `prod-db://~ >` afterwards; the
contract says "without persisting a link". So `connect host N [--transport T]` establishes the
connection exactly as `link host` does, records it with `persistent: false`, and pushes the
link frame itself. `leave` and `detach link` pop the frame and, for a non-persistent link,
forget it and hang up: after `connect host testbox; leave`, `get link` is empty. `link host`
prints its summary line and pushes nothing; `connect host` answers the `ono.link/1` record its
contract declares — the same row `get link` would show — and stands inside.

### 4. `test host` probes over the held link, or over the transport, and reports E0601

`ono.probe-result/1` is written by the network family for `test port` (ADR-0087) and shared
here; `test host` fills `port: null`, `protocol: ono` (the link protocol of spec §21.2 over the
transport), and the fields the handshake negotiates — `transport`, `protocol_version`, `agent`
(the far side's agent string, `ono/<version>`), `providers` (the remote provider ids).

- For a host the session holds an established link to, the probe reports the handshake's
  facts; `reachable: true` and the duration is the check's own.
- Otherwise it connects the way `link host` would — over a link definition's transport if one
  names the host, else over ssh — bounded by `--timeout` (default 10 s), hangs up, and reports
  what it negotiated.
- Over ssh, the shell hands its own `~/.ssh/config` to ssh as `-F` when the file exists
  (`SshTarget::with_config`, an amendment to ADR-0037's one spelling): the file `get host`
  lists hosts from is then the file ssh resolves them with, whatever the account's home
  directory is — which is also what keeps the probe offline in a test with a scratch `HOME`.
- A host that does not answer is `remote.unreachable` (E0601) carrying the transport's reason,
  and the run fails: the errors registry says "`test host <name>` probes reachability and
  reports where the attempt failed", and a probe that failed is the one case the schema's
  `error` field is not the better answer for — a script gating on `test host` needs the exit
  status, not a row it has to inspect.

### 5. Host records are the provider's: `add`, `set`, `remove host` act on the shell's host file

Unlike a link, a recorded host is not session state — it is a line in the shell's own host
file (ADR-0103 §2) — so these three go the ordinary way: `ProviderMutation` over `ono.shell`,
which advertises `host.list` (the capability the contracts name) and answers `add`, `set` and
`remove` in `act` by rewriting the file whole. The OpenSSH configuration is never written: a
host it lists cannot be changed from here, and `set`/`remove` of one is `io.not_found` with the
help saying why. `--dry-run` answers `skipped` with what would have happened.

`ProviderMutation` gains one rule for it: a selector that resolves to nothing is, for the verb
`add`, the target itself — a creation names what does not exist yet, and whether it may be
created is the provider's to say — where every other verb keeps ADR-0068 §2's "it is not
there" outcome. (The network family generalises the same rule to every verb in ADR-0088 §2;
the two agree on `add`.)

## Consequences

- `remote_missing.rs`: the link-definition tests (`should_record_a_link_definition_without_establishing_it`,
  `should_show_which_host_a_link_definition_points_at`, `should_change_a_link_definition_when_set`,
  `should_report_the_change_when_a_link_definition_is_set`, `should_rename_a_link_definition`,
  `should_forget_a_link_definition_when_removed`,
  `should_report_the_teardown_when_an_established_link_is_removed`,
  `should_refuse_to_enter_a_link_that_was_removed`, `should_pop_the_link_frame_when_detaching`,
  `should_keep_the_link_when_detaching`,
  `should_answer_again_from_a_detached_link_when_it_is_entered_again`), the `connect host`
  tests, the `test host` tests and the host-record tests
  (`should_record_a_host_in_the_shells_own_source`, `should_modify_a_recorded_host`,
  `should_remove_a_recorded_host`) are green; acceptance case
  `044-remote-links-as-objects` exercises the family end to end in the container.
- A link mutation alone at the prompt renders its ActionResult row, as every mutation does;
  `set config` alone stays silent (ADR-0094) — the two families differ deliberately: a
  settings line is declarative, a link teardown is an event worth a line.
- The piped forms remain open; a later increment can answer them by collecting the head
  stages as `… | enter socket` does (ADR-0075).

## Alternatives considered

- **`ono.shell` advertises `link.manage` and acts on the tables.** Rejected: the provider
  would need to pop frames and drop connections it cannot reach (ADR-0090), and inside a link
  frame the remote's `ono.shell` would be the one asked.
- **A `connect host` that persists a link like `link host`.** Rejected: the contract's one
  sentence is the whole difference between the two commands.
- **`test host` as a `ProviderProducer` like `test port` (ADR-0087).** Rejected: a `Query`
  carries no verb, so `ono.shell` could not tell `test host` from `get host`, and the held
  link's negotiation lives in the session.
- **Reporting an unreachable host as `reachable: false` with exit 0.** Rejected for `test host`
  (kept for `test port`, whose refusal answers the question): E0601's registered help text
  names `test host` as the command that reports where the attempt failed.
