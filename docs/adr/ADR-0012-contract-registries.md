# ADR-0012: The machine-readable contract registries

- Status: accepted
- Date: 2026-08-26
- Spec refs: §7.1, §8, §9.1, §10.2, §10.5, §11.5, §16.1, §22.1, §27, §28, §30, §31.16, §36,
  §37, §40.4, §43, §46, §47, §52, §53
- Decided by: agent (autonomous)

## Context

Spec §47 lists the machine-readable files the repository "can be bootstrapped with", §27 makes
them the source of truth for public command contracts, and §36 derives docs, completion, help,
provider conformance tests and drift checks from them. But the specification gives the
registries as *examples*, not as a schema: §27 shows one command entry and §27.3 one schema
entry, and neither says what the full field set is, what a stability level means, how a phase
maps onto an entry, or what to do with the many `<verb> <target>` pairs the §52 matrix marks
with a question mark.

Writing the registries therefore requires a set of decisions that no section of the
specification fixes. They are recorded here in one ADR because they are one decision — the
shape of the public contract — and because a later agent reading any single registry file needs
one place that explains its conventions.

## Decision

### 1. `docs/contracts/` layout follows §47, one file per registry

`verbs.yaml`, `targets.yaml`, `errors.yaml`, `capabilities.yaml`, `language.yaml`,
`commands/<family>.yaml`, `schemas/<name>.v1.yaml`. `grammar.ebnf` is already committed with
ADR-0009. `providers/*.yaml` (§35.3 conformance) and `kuang/*.yaml` (§31.78) are **not** written
here: the first is generated from provider implementations that do not exist before Phase C, and
AGENTS.md §14 schedules the second for Phase I.

Every file carries `version: 1` and a header comment naming the spec sections it derives from.

### 2. Identity schemes

| Kind | Form | Source |
|---|---|---|
| Command | `ono.<target>.<verb>` | spec §27's `ono.process.get` |
| Command with no object target | `ono.data.<verb>`, `ono.meta.<verb>` | the families of §9.1 |
| Verb | `ono.verb.<verb>` | — |
| Target | `ono.target.<name>` | — |
| Schema | `ono.<name>/<major>`, file `schemas/<name>.v1.yaml` | spec §27.3's `ono.process/1` |

Multi-word names are kebab-case (`ono.action-result/1`, `ono.env-var/1`,
`ono.config-setting/1`), matching the file names §47 already writes (`action-result.v1.yaml`).
Only the Ono project may claim `ono.*` (spec §31.5); contributed commands and schemas are
publisher-namespaced.

### 3. Stability is an API promise; `phase` is a schedule

The two are independent, and conflating them would make the registry lie in one direction or the
other.

- `stable` — the specification names the command in §9.1 or in another normative section. Its
  id, semantics and output schema are a compatibility promise (§40.4). A stable command may
  still be unscheduled: `get package` is stable and `phase: planned`.
- `experimental` — the command is named only by the §52 matrix, by an example, or by §7.1's verb
  table, or §53 explicitly declines to freeze its syntax (`join`, `diff`). The surface may
  change.
- `planned` — a `?` cell of the §52 matrix, or a target §8 names with no command anywhere. The
  entry additionally carries `validation_required: true`.

`phase` is the spec §37 phase that delivers the command, or the string `planned` when no phase
does. `spec-check` must require an implementation for `stable` commands whose phase has been
reached, and must not require one otherwise (§27.2).

### 4. The `?` cells of the §52 matrix become `planned`, not silence and not commands

Spec §52 says: "Question marks mean the semantic usefulness must be validated rather than
mechanically implemented for symmetry." Dropping those cells would lose the design surface;
emitting them as ordinary commands would be exactly the mechanical symmetry the sentence warns
against. They are therefore present as `stability: planned` with `validation_required: true`,
which records the cell without promising the command. Where a `planned` entry duplicates an
existing one — `start mount` against `mount filesystem`, `send signal` against
`kill process --signal`, `start interface` against `set interface --up` — it carries a `note`
saying so and stating that one of the two spellings must be withdrawn before either is stable.

### 5. `inspect` and `watch` are not expanded per target

The §52 matrix has an `inspect` cell for thirteen targets. One generic `ono.meta.inspect`
(spec §9.1 Meta: "detailed field/value/provenance view") serves all of them; only
`inspect process` and `inspect plugin`, which §9.1 defines with their own detailed outputs, get
their own entries. `watch`, by contrast, *is* per target, because §9.1 gives each a distinct
event output (`Stream<ProcessEvent>`, `Stream<ServiceEvent>`) and §15.2 requires help to state a
command's output type.

The `dir` row of §52 mirrors the `file` row; because `get dir` yields File records (§9.1), the
`dir` cells that behave identically are served by the `file` commands, and only `get dir`,
`enter dir`, `remove dir` and `set dir` exist separately.

### 6. Phase assignment where §37's bullet list is silent

Spec §37 Phase C says "deliver high-quality" and lists nine target names. Three §9.1 commands
address Linux core system objects that the list omits — `resolve dns`, `test port`, `get device`
— plus `get session`. They are assigned Phase C: they are core provider work of exactly the kind
the phase delivers, and §37's list reads as illustrative rather than exhaustive.

Conversely, the network **write** paths (`add`/`remove`/`set route`, `set interface`) are
`phase: planned` even though their targets are Phase C. Phase C's success criterion is about
inspection — "common inspection tasks no longer require parsing text" — and reconfiguring a
host's networking is a separate product decision with a separate risk profile.

`group` and `reduce` are assigned Phase B although §37's Phase B list names only the other
eight transforms: §9.1 puts all ten in one family and they are the same stream machinery.
`join` and `diff` stay unscheduled, because §53 declines to freeze them.

### 7. Type vocabulary: §10.2 plus three additions

Schema field types use the value model of spec §10.2. Three names are added:

- **`value`** — any value of the model. Used only where a field genuinely spans the union:
  `ActionResult.target` (§11.5's `ValueRef`), `ConfigSetting.value`, `Error.target`.
- **`<schema-id>` as a bare type** — an *embedded* record of that schema, in contrast to
  `ref<schema-id>`, which is an identity handle. `ActionResult.error` embeds an `ono.error/1`;
  `Process.user` refers to an `ono.user/1`. §27.3 shows `ref<…>` but has no spelling for
  embedding, and §22.1's Graph needs one.
- **`enum` carries a `values:` list** on the field that uses it. §28 writes `state Enum` without
  enumerating; a closed set that is not written down cannot be validated or completed.

A `ref<T>` carries T's identity fields plus a display name and resolves to the full record. This
is what lets §23.6's requirement hold — "represent unresolved IDs without discarding numeric
identity": a `ref<ono.user/1>` for an unresolvable uid still carries the uid.

### 8. Nullability is deliberate, per spec §10.5

Every field is exactly one of `required: true` or `nullable: true`; a field that is neither is a
defect. A field a provider might be unable to answer is `nullable: true`, and null means
"unknown or unavailable", never zero and never an empty string. Consequences worth naming:
`Process.cpu` is nullable because a single procfs read cannot compute it; `Neighbor.mac` is
nullable because an incomplete ARP entry has none; `Route.destination` is nullable and null
*is* the default route — a real answer, documented as such in the field's `doc`.

### 9. Identity for the schemas §28 leaves without one

§28.5 (Interface), §28.6 (Mount) and the derived schemas give no identity line.

- `Interface` → `[index]`. The name is renameable while the interface lives; the netlink index
  is not.
- `Mount` → `[target]`, with the caveat noted in the file that mounts can stack on one point.
- `Route` → `[table, family, destination, gateway, interface]`; `Neighbor` → `[address, interface]`;
  `Filesystem` → `[uuid, source]`; `Group` → `[gid]`; `EnvVar` → `[name]`; `Job` → `[id]`;
  `ConfigSetting` → `[key]`.
- Structural sub-records and non-addressable values declare `identity: []`: `Endpoint`, `Error`,
  `Graph`, `GraphNode`, `GraphEdge`.

### 10. Fields the spec names but does not type

- `File.mode` (`PermissionMode?` in §28.2) is a `string` of four octal digits, e.g. `"0644"`.
  Not an `int`, because ADR-0009's grammar has no octal literal to compare it against. A
  `PermissionMode` semantic scalar is deferred.
- `Socket.local` / `Socket.remote` (`Endpoint?` in §28.4) are `ono.endpoint/1`, a new structural
  schema. §41.2 (`where local.address not in [127.0.0.1, ::1]`) and §6.2 (`group remote.host`)
  both reach into it, so its fields are fixed by the spec's own examples.
- `Group.members` is `list<string>` of login names, not user references: the account database
  stores names, and a listed name need not resolve to an account.

### 11. Two capability vocabularies, one file

`capabilities.yaml` has two top-level lists, because §27's `provider_capability` and §31.16's
capability families are different things and merging them would be a security error waiting to
happen:

- `provider_capabilities` — what a provider must be able to do for a command to work. Referenced
  by every command entry. §27.1 derives the "provider capability matrix" from this pairing.
- `kuang_capabilities` — the complete 29 families of §31.16, verbatim. These are a security
  boundary: granted, scoped, leased, audited, revocable.

Both carry `risk` (`read` / `observe` / `mutate` / `destructive`) and `elevation`
(`none` / `conditional` / `required`). `observe` is separated from `read` because a subscription
holds resources and keeps running, which §11.2 and §18 treat differently from a query.
`conditional` is a first-class answer rather than a hedge: on Linux most read capabilities are
unprivileged for your own objects and privileged for others, and a shell that flattened that
would mislead on every second command.

Capability *classes* (`required` / `optional` / `runtime_requested`, §31.17) are a property of a
package's declaration, not of a capability, so they are not fields here.

### 12. Commands the spec writes outside §9.1

Kept, at `experimental`, because the specification writes them even though §9.1's tables do not:
`connect host` (§6.1), `trace file --users` and `trace socket --port` (§22.3), `get log --service`
(§33.2, §41.4), `| tail 30` (§33.2, §41.4), `format table` (§12.3), `get config` / `set config`
(§30), `leave` (§7.1, §37 Phase E).

`tail` exists twice, deliberately: `ono.file.tail` and `ono.journal.tail` take a target word and
follow a source (§7.1: "follow append-oriented content", targets file and journal), while
`ono.data.tail` is the transform spelling `| tail 30` that §33.2 uses. The two are
disambiguated by the presence of a target word, exactly as `get` is.

`get log` and `get journal` overlap; `get log` is the one the spec's examples use, so
`get journal` is `planned` with a note that one must be withdrawn.

### 13. Deferred schemas

A command may reference a schema that is not yet written. Rather than weakening the type to
`record`, the reference is written in full and the deferred set is enumerated here, so it is one
grep away and nothing silently loses its type. Forty-two schemas are deferred:

| Phase | Schemas |
|---|---|
| B | `measure` |
| C | `connection`, `device`, `dns-record`, `probe-result`, `session` |
| D | `command`, `execution-plan`, `help-page`, `inspection`, `process-detail`, `type-info` |
| E | `context` |
| F | `process-event`, `service-event`, `socket-event`, `interface-event`, `route-event`, `file-event`, `mount-event`, `user-event`, `group-event`, `container-event`, `host-event`, `link-event` |
| H | `host`, `link` |
| I | `plugin`, `plugin-package`, `plugin-inspection`, `plugin-runtime`, `verification-result`, `capability-grant`, `assistant`, `assistant-turn`, `model-provider`, `finding`, `plugin-audit-event` |
| planned | `container`, `image`, `package`, `log-record` |

Each is written in the increment that delivers its phase, before the provider that emits it.

### 14. `language.yaml` restates, it does not redefine

`grammar.ebnf` is the grammar (ADR-0009). `language.yaml` carries the lexical and resolution
surface that completion and highlighting need as data (§46.3): keywords, operator precedence
read off the production chain, unit suffixes with their factors, the argument-mode tables, the
namespaces and resolution order of ADR-0011, and the statement forms. Where the two could
disagree, `grammar.ebnf` wins, and the file says so.

`elif` and `until` appear in ADR-0009's expression-mode head list and in `grammar.ebnf`'s
header, but the grammar spells the constructs `else if` and `while`. They are listed as
**reserved**, not as keywords, so that a future spelling cannot silently change the meaning of
an existing script.

## Consequences

Easy: help, completion, reference docs, the command-id constants and the provider capability
matrix are all derivable now (§27.1, §46); `spec-check` has something concrete to check drift
against; a new command is added by writing a registry entry first (§7 step 1 of the TDD loop),
which is what §27 asks for.

Hard: 168 command entries mean 168 places to keep honest. The mitigation is that `spec-check`
validates the cross-references mechanically — every verb against `verbs.yaml`, every target
against `targets.yaml`, every `provider_capability` against `capabilities.yaml`, every schema
reference against `schemas/`, and every `argument_mode` against ADR-0009's table.

Must be revisited: when Phase D implements `spec-check` properly, the `validation_required`
entries are the backlog of §52 cells to decide; when a KUANG/11 package contributes a command
(§31.22), `argument_mode` must come from the registry rather than only from the parser's
built-in table, as ADR-0009 already anticipates.

Encoded by: the registry files themselves, and the `cargo xtask spec-check` cross-reference
checks that Phase D adds.

## Alternatives considered

- **Omitting the `?` cells of §52 entirely** — rejected: the matrix is a design surface, and
  losing it would mean rediscovering the same questions later without the record that they were
  already asked.
- **Emitting the `?` cells as ordinary commands** — rejected: §52 forbids exactly that in the
  sentence following the table.
- **One `commands.yaml` instead of a directory** — rejected: §47 gives the directory, and a
  single file of 168 entries is unreviewable in a diff.
- **Merging provider capabilities and KUANG/11 capabilities into one list** — rejected: one is a
  dispatch concern, the other a security boundary. A future reader must never be able to grant a
  package `process.list` by mistaking it for `process.read`.
- **Typing unresolved schema references as `record`** — rejected: it would make the registry
  parse cleanly while quietly discarding the contract, which is the failure mode §36.5 exists to
  catch.
- **Generating `errors.yaml` from `ono_core::ErrorCode`** — deferred, not rejected. ADR-0006
  anticipates it. Until the generator exists, the file is hand-written and verified against
  `crates/ono-core/src/error.rs` code-for-code, name-for-name and kind-for-kind.
