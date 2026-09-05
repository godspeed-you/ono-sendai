# ADR-0055: The declarative adapter contract

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.7, §1.9, §1.10, §1.11, §1.14–§1.16, §1.22, §1.27, §1.44–§1.47, §1.66; v0.2 §31.5, §36, §47; ADR-0012, ADR-0022, ADR-0027
- Decided by: agent (autonomous)

## Context

Spec v0.3 §1.44 sketches an adapter manifest, §1.45 the declarative SDK it needs, §1.46 the
version probe and §1.47 the fixtures — and says the actual manifest schema "MUST be versioned
and machine-validated". §1.66 wants first-party contracts in the source tree under
`spec/adapters/` (read `docs/contracts/adapters/`, AGENTS.md §2) so that reference pages,
invocation matrices, probes, capability declarations and fixture harnesses derive from one
file. This ADR fixes the format before the first adapter is written to it.

## Decision

1. **One file per package**, `docs/contracts/adapters/first-party/<package>.yaml`, format
   `ono-adapter-pack/1`, described field-for-field by `docs/contracts/adapters/schema.yaml` in the
   style of `docs/contracts/kuang/manifest.v1.yaml`. A package carries `package` (id, name,
   version, publisher, tier), `roles: [adapter]`, `capabilities` (the `process.exec` grant of
   v0.3 §1.22: `executables` and `argv_policy: declared-invocations-only`) and `adapters`.

2. **An adapter names one executable family and its invocations.** `executable.names`,
   `executable.versions` (a semver range, or `any` for version-independent machine protocols),
   `executable.version_probe` (argv, a capture pattern, `cache: executable-identity`),
   `output_demand` (the demands it answers: `structured`, `interactive`), `fallback: raw`,
   `tier` (A/B/C of v0.3 §1.9), `limits` (prose the reference page prints) and `invocations`.

3. **An invocation is a matcher plus a plan.** `match.words` lists the positional spellings
   that select it (`[[address], [addr], [a]]`; `[[]]` for none), `match.flags.allow` the user
   flags that may pass through, `match.positionals` whether further words are allowed. Anything
   else the user typed is `UnsupportedInvocation` (v0.3 §1.14: never silently removed,
   reinterpreted or approximated). `plan.argv` is the exact machine-oriented invocation, user
   flags appended when `plan.append_user_flags` is true, `plan.env` the environment
   stabilisation (`LC_ALL: C`), and `plan.stdin: null | inherit`.

4. **Decoders are declarative first, code second.** `decoder.kind` is one of:
   - `json` — a JSON document; `records` names the key holding the record list (absent: the
     document itself), `nested` a key whose children are recursed into with the parent's
     identity carried as `parent`;
   - `lines` — an explicit field protocol; `field_separator`, `record_separator` (`\n` or
     `\0`) and `columns`, in the order the plan's argv requests them;
   - `builtin` — a decoder implemented in Rust under a stable `id` (`ss-text-v6`), declared
     with `stability: version-constrained`; the only kind allowed for Tier C.

5. **`fields` maps decoded names onto the schema.** Every entry is `from` (the decoded field,
   `$parent.<field>` for nested trees), an optional `unit` (`bytes`, `kib`, `seconds`,
   `percent`), an optional `map` of literal translations (`{"0": false, "1": true}`), and
   `exactness` (`exact` by default; `normalized` when a unit or literal map applied;
   `inferred` only when written down). Coercion into the target type is driven by the schema
   field's declared type, so the contract cannot promise a type the schema does not have.
   Decoded fields the map does not name go into `extensions["<package>.<adapter>"]`
   (v0.3 §1.11); nothing is dropped and nothing is fabricated — a missing field is `null`.

6. **Schemas are canonical.** `schema` must name a registered `ono.*/1` schema; an adapter that
   needs a new one adds it to `docs/contracts/schemas/` in the same increment, and the ADR-0027 list
   of implied schemas (block-device, namespace, journal-event, …) is met that way.

7. **Fixtures are part of the contract.** `fixtures` names a directory under
   `docs/contracts/adapters/fixtures/<package>/<adapter>/`; each fixture is `<name>.out` (the bytes
   the tool produced) beside `<name>.yaml` (the invocation, the tool version, the distro, and
   the values or the error the decoder must produce). Every invocation needs at least one
   fixture, and the families of v0.3 §1.47 (empty, error, unknown fields, malformed) are
   checked by name where they apply.

8. **`spec-check` validates all of it**: every first-party file parses under the schema, ids
   are `org.ono.compat.<package>` and adapter ids kebab-case, every executable an adapter names
   is in the package's `process.exec` set, every `schema` is registered, every `fields.from` is
   spelled and every target field exists in the schema, every `builtin` decoder id exists in the
   binary, every fixture directory exists and is non-empty, and every fixture decodes to what
   its `.yaml` says (ADAPT-010 adds the last rule when the decoders exist).

## Consequences

- Adding a Tier A or B tool is a YAML file, a fixture set and an acceptance case — no Rust —
  which is what lets the compatibility program be delegated and reviewed as data.
- The reference pages and the compatibility matrix are generated from the same file
  (v0.3 §1.66), so support claims cannot drift from behaviour.
- `ono-adapter` embeds the first-party files with `include_str!` exactly as `ono-command`
  embeds the command contracts; a KUANG/11 package ships the same format in its manifest's
  `adapters:` section (ADAPT-008).

## Alternatives considered

- Rust-coded adapters only — rejected: v0.3 §1.45 wants simple adapters without a runtime
  component, and code cannot be turned into a compatibility matrix.
- One contract file per adapter — rejected: the capability grant is per package (§1.22), and
  a package that declares `ip` and `ss` together is what the user reviews.
