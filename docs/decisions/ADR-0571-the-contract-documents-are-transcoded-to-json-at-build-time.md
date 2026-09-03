# ADR-0571: The contract documents are transcoded to JSON at build time

- Status: accepted
- Date: 2026-09-03
- Spec refs: §34 (cold start budget), §36.5, v0.3 §1.66; ADR-0055, ADR-0313, ADR-0490
- Decided by: agent (autonomous)

## Context

§34 budgets a cold shell start at 50 ms aspirational and 100 ms acceptable, and `cargo xtask perf`
(ADR-0490) measures it as `shell.cold_start`: `ono -c "echo ready"`. On the reference machine
that figure was 26,8 ms first / 30,1 ms p95, while `bash -c 'ls /usr/bin | wc -l'` takes 4 ms on
the same hardware. `ono --version` takes 0,9 ms, so the binary is not the cost; the session is.

Measured in-process, release build, median of three runs, the first pipeline of a one-shot
session spends:

| phase | ms |
| --- | ---: |
| `CommandRegistry::embedded()` — 203 KB of command YAML plus verbs, targets, capabilities | 6,4 |
| `builtin_schemas()` — 216 KB of schema YAML, ninety documents | 6,9 |
| `register_async` — systemd and logind over D-Bus | 6,7 |
| `ono_adapter::Registry::bundled()` — 45 KB of adapter-pack YAML | 2,0 |
| tokio multi-thread runtime, build and drop | 1,2 |
| session, configuration, theme, synchronous provider constructors | < 1 |

Fifteen of twenty-four milliseconds are YAML parsing, of text that is fixed when the binary is
linked. The documents are embedded with `include_str!` precisely so that a cold start does not
read files (ADR-0055, the registry's own module comment); embedding them and then parsing 465 KB
of YAML on every start kept the file reads out and left the parse in.

## Decision

**The three crates that embed `docs/spec/` documents — `ono-command`, `ono-value`,
`ono-adapter` — transcode them from YAML to JSON in a `build.rs`, embed the JSON, and read that at
run time.**

1. **The YAML stays the source.** `docs/spec/commands/*.yaml`, `docs/spec/schemas/*.yaml`,
   `docs/spec/verbs.yaml`, `targets.yaml`, `capabilities.yaml` and
   `docs/spec/adapters/first-party/*.yaml` are unchanged in role: people read and edit them,
   `spec-check` validates them, packages copy their shape. Nothing reads the generated JSON but
   the crate that generated it.
2. **The transcoding is exact.** `build.rs` parses each document into `serde_yaml_ng::Value` and
   writes it with `serde_json` — the same value model, one spelling to another. At run time the
   JSON is read back into the same `Raw*` types (`ono-command`), the same `serde_yaml_ng::Value`
   walker (`ono-value`) and the same `AdapterPack` (`ono-adapter`) as before; no consumer changed.
   A unit test in each crate reads the YAML on disk, transcodes it the same way, and asserts that
   what the binary carries is equal to it, document for document, and that no document on disk
   is missing from the binary.
3. **`cargo:rerun-if-changed` names every document and its directory**, so an edited or added
   contract rebuilds the crate, exactly as `include_str!` did.
4. **A document that does not transcode is a build error**, said once on stderr. There is no run
   time fallback to YAML: the shell never sees the YAML, so it cannot be half-migrated.
5. **`AdapterPack::parse` still takes YAML**, because third-party packs arrive as YAML (ADR-0313's
   depth check stays in front of it). The bundled packs go through a private `embedded(json)`
   that skips the depth check — the transcoding already parsed them as YAML — and share the
   stamping of pack id and version with `parse`.

### What JSON is stricter about

YAML reads a plain scalar into a `String` field whatever it looks like; JSON does not. Two
first-party packs listed the `ip` and `curl` flags `-4` and `-6` unquoted, which YAML had been
reading as the strings `"-4"` and `"-6"` because the field asked for strings. As JSON they were
integers and the packs did not read. They are quoted now, in a `spec` commit of their own ahead of
this one, and the fidelity test would catch the next one: a pack that does not read is left out of
`first_party()` by design, and `should_read_every_first_party_pack_it_embeds` makes being left out
loud.

## Consequences

- `shell.cold_start` (`cargo xtask perf --profile S`, release build, 20 iterations,
  `ryzen-3900x-ubuntu-2604`): see the commit body for the before/after record.
- Three `build.rs` files of the same shape, with `serde_yaml_ng` and `serde_json` as
  build-dependencies. They are already dependencies of the crates, so the supply chain is
  unchanged (`xtask supply-chain` reads `build-dependencies` too).
- `ono-value` keeps `serde_yaml_ng` at run time: `from_yaml`, `yaml_depth` and the `convert`
  command need it. The change is only that the schema contracts no longer go through it.
- The build is a little longer by the transcoding, which is well under a second for all three
  crates together and happens only when a document changes.

## Alternatives considered

- **Parse the three groups on three threads, overlapped with the D-Bus connection.** Halves the
  wall clock and keeps all of the work. Rejected: the work is on text that cannot change after
  the link step, so the right amount of it at run time is none.
- **A binary format (`postcard`, `bincode`) of the parsed `CommandContract`, `Schema` and
  `AdapterPack`.** Faster still, but it needs `Serialize` on every contract type and a build-time
  crate that owns those types, which means splitting each crate in two. JSON reaches the same
  order of magnitude with no type touched.
- **A cache in `$XDG_CACHE_HOME`, written after the first parse.** State on disk that must be
  invalidated by binary version, with a cold first run that the benchmark measures. Rejected.
- **Parsing lazily, per family or per schema, on first use.** `check_pipeline_with` needs every
  schema and `resolve` every verb on the first pipeline, so the first pipeline of a one-shot
  session — the one §34 measures — would pay all of it anyway.
