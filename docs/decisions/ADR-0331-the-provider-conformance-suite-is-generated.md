# ADR-0331: The provider conformance suite is generated from the declarations

- Status: accepted
- Date: 2026-08-29
- Spec refs: §35.3 (provider conformance), §36.3 (generated tests), §36.5 (contract drift),
  §10.2 (the type vocabulary), §10.6 (units), §47 (the registries); spec v0.4 §42.1;
  ADR-0012, ADR-0111, ADR-0126, ADR-0248
- Decided by: agent (autonomous)

## Context

Spec §35.3 says it in one sentence: "Every provider capability gets a generated conformance suite
from registry metadata." Nothing in the tree generated one. What existed was a *drift* check —
`crates/ono-cli/tests/providers.rs` compared `docs/spec/providers/*.yaml` with the built registry
— plus hand-written per-provider schema restatements in `ono-provider-linux/tests/schemas.rs` and
`ono-provider-netlink/tests/schema_contract.rs`. Those covered four providers of eighteen and two
schemas of thirty; the rest of the registry advertised targets, schemas and identity strategies
that no test ever asked a question about.

ADR-0248 corrected two sentences in `docs/ACCEPTANCE.md` that claimed the generation existed, and
argued against building it: "a conformance suite generated from a YAML file asserts what the YAML
says, so it can only ever restate the claim." That argument is right about a generator that emits
assertions *about the YAML*. It is wrong about the suite §35.3 describes, whose five example
clauses — "identity is stable within process lifetime", "pid is required and positive", "name is
non-null", "unknown memory is null, not zero", "permission failure is represented, not fabricated"
— are all questions put to a *running provider*. The declaration says what to ask; the provider
has to answer.

## Decision

**`cargo xtask conformance` generates `crates/ono-cli/tests/provider_conformance.rs` from
`docs/spec/providers/*.yaml`, `docs/spec/schemas/*.v1.yaml`, `docs/spec/capabilities.yaml` and
`docs/spec/commands/*.yaml`.** The file is committed; `spec-check` regenerates it and fails when
the committed copy differs, exactly as it does for `docs/reference/` (spec §36.2).

**The generated file carries the declarations; the assertions live beside it**, hand-written, in
`crates/ono-cli/tests/conformance_harness/mod.rs`. A generated file that also carried the
assertions would be 1600 lines nobody reads, and the assertions are the half a human has to be
able to argue with. Four questions are asked of every provider entry:

1. **Surface** — the registry advertises exactly the declared targets, capabilities and schemas,
   each capability at the risk `capabilities.yaml` fixes and elevated exactly when that file says
   `required`. (`Capability::needs_elevation` is a boolean and `elevation` has three values;
   `conditional` means the capability works unprivileged for some targets, so only `required` is
   privilege the shell must hold up front.)
2. **Schema contract** — every schema the entry declares is carried, field for field, in
   declaration order, with the declared type name, requiredness, nullability, unit, identity and
   default view.
3. **Target** — a bare snapshot of every target the entry serves behaves as declared, and every
   record it yields claims a declared schema and satisfies it.
4. **Account** — every declared capability is reached by something: a read capability by the
   snapshot case of the target it reads, any other by a command in `docs/spec/commands/` that
   asks the registry for it. Generation *fails* when a capability reaches neither, so the
   accounting cannot have a hole in it.

**The declarations gain a `conformance:` block**, one word per target, because "how does a bare
snapshot of this target behave" is not derivable from anything else and a suite that guessed would
be a suite that skipped:

- `enumerable` — the snapshot ends and yields records, possibly none;
- `selector_required` — the snapshot must *refuse* with a structured error; a provider that
  cannot answer without an argument says so rather than answering emptily (§35.3);
- `unbounded` — the snapshot is a live stream that need not end; what it emits still has to
  satisfy its contract.

A target with no exercise, an exercise for a target the entry does not serve, and an exercise word
the harness does not implement each stop generation rather than producing a suite with a hole.

**The identity exercise is spec v0.4 §42.1 as far as one snapshot can carry it**: every field the
schema's identity is made of must be present in every record — an object that omits it cannot be
found again — and two objects whose identities are *fully known* may not share one. An identity
with a null component is exempt from the second half, because there the provider is saying it does
not know, which §35.3 requires it to say rather than to invent. Two live cases sit in that
exemption today and are reported rather than asserted away; see Consequences.

**The type vocabulary of spec §10.2 is read twice**, once by `ono-value` to build the schema a
provider carries and once by the generator to write down the name it expects. That is deliberate:
two independent readings held together by the generated suite is the drift check §36.5 asks for,
and a single shared parser would make the comparison vacuous.

**`docs/ACCEPTANCE.md` §4.1 D says "generated from" again, and this time it is true.** ADR-0248's
mechanical rule stays and now also accepts a path `cargo xtask conformance` writes.

### Supersedes

This ADR supersedes ADR-0248's *decision to build no generator*. Everything else ADR-0248 decided
stands: the §4.7.4 wording, and `xtask::reference::check_generation_claims` enforcing that a box
claiming a generation names a generated path.

## Consequences

The hand-written tests the generation subsumes are gone, in the same commit:
`crates/ono-cli/tests/providers.rs` entirely; the four `assert_contract` cases and the helper in
`ono-provider-linux/tests/schemas.rs`; `should_declare_the_identity_the_contract_names`,
`should_declare_every_field_the_contract_names` and `should_advertise_exactly_the_schemas_it_emits`
in `ono-provider-netlink/tests/schema_contract.rs`. What stays is what a declaration cannot
express: what a field *means* (`should_declare_the_cpu_field_as_a_percentage`), and that records
decoded from a fixed `/proc` tree or a fixed netlink byte stream satisfy their contract.

Eighteen provider entries, thirty schemas and thirty-five targets are now exercised where four
providers and two schemas were before. Running it found four contract violations, fixed in their
own commits beside this one:

- `ono.interface/1` carried `unit: bytes` on `mtu`, `rx_bytes` and `tx_bytes` in the code and
  declared no unit in the contract. A unit is part of the meaning (§10.6), so the contract gained
  it.
- `ono.capability-grant/1` declared `plugin`, `class` and `granted_at` required, and the
  definition rows `get capability` lists before the grants (ADR-0111) carry none of the three.
  They are nullable now, with the reason on each field.

Two live findings remain open, measured rather than relaxed:

- `ono.socket/1` identifies a socket by `inode` alone, and the kernel reports no inode for some
  sockets (TIME\_WAIT among them): three of sixteen connections in a live snapshot carried a
  wholly null identity. What would close it is an identity that falls back to the tuple —
  `(protocol, local, remote)` — which is a schema-identity change and therefore its own increment.
- `ono.filesystem/1` identifies a filesystem by `(uuid, source)`, and pseudo filesystems have
  neither: two `source: none` entries shared one identity. What would close it is carrying the
  device number, which `ono.filesystem/1` does not have a field for today.

Generation is cheap (it reads eleven YAML files) and the suite runs in under a second, because
every provider it asks answers locally.

## Alternatives considered

- **Leaving the drift check as the whole story** (ADR-0248). Rejected above: it says the registry
  and the code agree about *names*, and never asks a provider a question.
- **Generating the assertions as well as the data.** Rejected: the file would be unreviewable, and
  a change to what conformance *means* would be a change to a generator's string templates rather
  than to a test someone can read.
- **Sharing `ono-value`'s type parser with the generator.** Rejected: it would make the
  schema-contract case compare a value with itself.
- **A `build.rs` in `ono-cli` generating the suite at compile time.** Rejected: the suite would
  not be visible in the repository, and `spec-check` could not report drift with a path to fix.
