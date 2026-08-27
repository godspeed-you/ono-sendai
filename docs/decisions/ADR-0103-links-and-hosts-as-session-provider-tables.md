# ADR-0103: Links and hosts are tables of the session provider

- Status: accepted
- Date: 2026-08-27
- Spec refs: §9.1 (the remote table), §14.4, §21.1–§21.3, §33.5, §35.3, §37 Phase H; ADR-0036,
  ADR-0037, ADR-0090
- Decided by: agent (autonomous)

## Context

`get link` rendered the session's link table by hand in `context.rs` — text a person could read
and nothing a pipeline could use: `get link | to json` printed nothing structured, `watch link`
and `trace link` had no records to start from, and `get host` answered E0101 because no
provider claimed the `host` target. Spec §9.1 lists both as producers, §33.5 requires every
stream to serialise, and `remote_missing.rs` asks for both as data.

ADR-0090 built the seam this needs: the session publishes plain rows before each pipeline
runs and the `ono.shell` provider answers from them, and it left `link` and `host` for the
remote family to add (ADR-0090 §3). What remained open was the shape of a host record — §28
defines none — and where hosts come from at all.

## Decision

### 1. `link` is one more published table

`SessionTables` gains `links: Vec<LinkRow>`, refreshed by `Session::publish_links` from the
same `pipeline_context()` call that publishes jobs. `ono.shell` serves `link` with `link.list`
and `ono.link/1`; `get link`, `get link <name>`, `get link | to json` and `watch link` are the
ordinary producer paths. The hand-rendered `get link` in `context.rs` is gone, and with it the
`GetLink` claim; `link host` and `enter link` stay the shell's, as `enter` and `leave` are.

`ono.link/1` grows, additively: `host` (where the link points — the name itself for
`link host`, whatever `--host` said for a definition), `mode` (`agent` | `agentless`, spec
§21.3's visible fallback), `protocol` (the negotiated protocol version, null until negotiated),
`providers` (the remote provider ids, which keep their ids across the link per ADR-0036), and
the state `defined` beside `connected` and `closed` for a definition never established. A
definition's `targets` is the empty list: nothing was negotiated, and that is what an empty
list says.

`SessionLink` is therefore a definition with an optional `LinkConnection` (the `RemoteLink` and
its mounted registry) rather than a connection with a name; `link_registry` and
`pipeline_context` answer only for an established one.

### 2. Hosts come from three sources, and one record per name

`ono.host/1` is written by this ADR (ADR-0012): `name`, `address`, `port`, `user`, `source`,
`link`, `transport`; identity `[name]`. The sources, consulted in this order:

| `source` | What | Access |
|---|---|---|
| `ono` | the shell's own host file, `<config dir>/hosts.json` (ADR-0010's directory: `ONO_CONFIG_DIR`, `XDG_CONFIG_HOME/ono`, `~/.config/ono`) | read and rewritten whole by `add/set/remove host` (ADR-0104) |
| `ssh-config` | the `Host` blocks of `~/.ssh/config` — literal names only, with `HostName`, `Port`, `User`; patterns, `Match` bodies and `Include` are not hosts and are skipped | read only: it is OpenSSH's file, and rewriting it would lose everything the shell does not parse |
| `link` | the hosts the session's links point at | the link table |

The OpenSSH configuration is a source because the ssh transport of ADR-0037 runs `ssh <host>`,
which reads exactly that file: a name that works for `link host` is a name that file defines.
A host several sources list is one record, from the first source, so `get host` never shows
`devbox` twice; whichever source it came from, a held link adds `link` and `transport` to the
record. `get host --source <name>` consults one source alone. A source that cannot be read is
a failure on the stream beside the other sources' records (spec §16.5), and no configured
source at all is an empty stream (spec §35.3), never an invented host.

The sources are located from the session's environment when the registry is built
(`providers::registry_with_tables`), so a test with a scratch `HOME`/`XDG_CONFIG_HOME` reads its
own files and never the developer machine's.

### 3. Phase H is a delivered phase

`ono-command`'s `DELIVERED` list gains `H`. A phase-H contract whose verb is `get` binds a
`ProviderProducer`; the mutating and context verbs of the family bind nothing here — no
provider advertises `link.create` or `link.manage`, and ADR-0104 makes those the shell's own —
so nothing is registered that would fail halfway (spec §50).

## Consequences

- `get link | to json` is a `stream<ono.link/1>`; `get host` lists `~/.ssh/config` hosts with
  `source: ssh-config`, the linked host with its transport, and nothing when nothing is
  configured. Tests: `remote_missing.rs` (`should_serialise_a_held_link_as_a_typed_record`,
  `should_serialise_an_empty_link_table_when_nothing_is_linked`,
  `should_list_a_linked_host_among_the_known_hosts`,
  `should_list_a_host_from_the_ssh_client_configuration_with_its_source`,
  `should_resolve_one_configured_host_by_name`,
  `should_answer_an_empty_host_list_when_nothing_is_configured`); `hosts.rs` unit test for the
  OpenSSH parser.
- `get link` at the terminal is now the table renderer's — `NAME HOST TRANSPORT MODE STATE
  TARGETS` — so acceptance case 049 matches the columns by regular expression instead of the
  hand-rendered `testbox  local  connected`.
- `ono.shell` resolves a `name` selector to hosts, never to links: link mutations are the
  shell's (ADR-0104), and a name that is both a link and a host must not make one `set host`
  act twice.
- Inside a link frame `get link` and `get host` are answered by the remote's `ono.shell`, as
  every provider call is (spec §14.4, ADR-0036): the remote's links and hosts, not this
  session's.

## Alternatives considered

- **A `host` provider crate reading `/etc/hosts` and DNS.** `/etc/hosts` maps addresses, not
  hosts a person would link to, and a resolver answers a different question (`resolve dns`,
  ADR-0087). Rejected for now; a further source slots into the same list.
- **Rewriting `~/.ssh/config` from `add host`.** Rejected: the shell parses three keywords of a
  file with dozens, and a rewrite would silently drop the rest.
- **One record per (name, source).** Rejected: `get host devbox` would answer two rows for one
  machine, and the identity of a host is its name.
- **A YAML or `config.ono`-style host file.** JSON is already a dependency, is unambiguous to
  parse, and the file is the shell's to write; nothing is gained by a second format.
