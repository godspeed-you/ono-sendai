# ADR-0916: An artifact is keyed on the exact wasmtime release, and on nothing of Ono's

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.2 §31.36; ADR-0870 §8, ADR-0914, ADR-0915
- Issues: #126
- Decided by: agent (autonomous)

## Context

ADR-0870 §8 relied on wasmtime's default `ModuleVersionStrategy`, in which an artifact carries
the engine's version and the engine checks it on every load. By default that version is only
the **major** version (`"47"`, `ModuleVersionStrategy::as_str`). An artifact written by 47.0.3's
Cranelift is therefore accepted by a 47.0.4 engine. If 47.0.4 exists to fix a miscompile, the
shell keeps running the miscompiled code after the upgrade, and nothing tells the operator to
recompile. Issue #126 says that a shell upgrade invalidates artifacts. With the default, one
that stays within a major version does not.

The flags, tunables and wasm features that wasmtime records and checks do not help here. They
describe the configuration, not the compiler's code.

## Decision

**1. The version stamped into and checked on every artifact is the exact wasmtime release:**
`Config::module_version(ModuleVersionStrategy::Custom("wasmtime-47.0.4"))`. It is set in
`ono-kuang-supervisor::wasm::config`, the one configuration that `ono`'s engine and
`kuang-compile` share. An artifact from any other release, patch releases included, is refused
as `incompatible` with wasmtime's own words (`Module was compiled with incompatible version
'…'`), and the refusal names `kuang-compile`.

**2. The release is written once, as `wasm::WASMTIME_VERSION`, and a test holds it to
`Cargo.lock`.** wasmtime exports no constant for its full version. A build script that parsed
the lock file would work, but a failing test gives the same assurance more simply. A bump of
wasmtime that forgets the constant fails
`compiled::engine_version::should_name_the_exact_wasmtime_release_the_lock_file_resolves`.
`engine_identity()`, which the refusal and `kuang-compile --help` print, reports the same string
(`wasmtime 47.0.4 on x86_64`).

**3. Ono's own version is not part of the key.** The code in an artifact is a function of
wasmtime's compiler and of the configuration. wasmtime already records the configuration and
checks it on load: target and ISA flags, Cranelift settings, tunables, wasm features. An Ono
release that changes neither leaves every artifact correct. Keying on Ono's version would make
every Ono release, including a documentation fix, cost every operator one `kuang-compile` per
component, and it would add nothing to what the engine checks. ADR-0870 accepted one command per
component per *engine* version, and this keeps that cost at that rate. It also makes that rate
true: every wasmtime release is a new engine version, and nothing else is. #126's "a shell
upgrade invalidates it" holds for every shell upgrade that changes the engine. That was the
point of the sentence ("the artifact is per engine version and per host architecture, so…").

**4. An artifact is not additionally bound to its component (no sidecar, no trailer).**
The review asked for this as optional. The artifact's name is the SHA-256 of the component's
bytes. The store and every directory above it can be changed only by this user or root
(ADR-0915), and the only writer there is `kuang-compile`, which names what it writes. A record
of the component inside or beside the artifact would guard only against that same trusted
writer renaming files by hand, and that writer could just as well edit the component. In
exchange, it would add a file format of Ono's own around wasmtime's, which every artifact, the
acceptance cases that edit artifacts and the image build would have to carry.

## Consequences

- Every wasmtime bump, patch releases included, turns every artifact into an `incompatible`
  refusal that names `kuang-compile`. That is one command per component, and `install plugin`
  runs it for anything installed afterwards.
- Artifacts written before this change carry `"47"` and are refused once, as `incompatible`.
  Ono is not yet in production use, so this costs one recompilation and needs no migration.
- The stamp's shape changed from a bare number to `wasmtime-<release>`. Two things that locate
  it by that shape to simulate an engine upgrade are adapted:
  `ono-kuang-sdk/tests/compiled.rs::engine_stamp` and acceptance case 351.
- Tests:
  - `compiled::patch_release::should_refuse_an_artifact_stamped_with_the_major_version_alone`.
    It failed before this change: the artifact was mapped.
  - `compiled::engine_version::should_name_the_exact_wasmtime_release_the_lock_file_resolves`.
  - `ono-kuang-sdk/tests/compiled.rs::should_refuse_an_artifact_another_engine_version_wrote_and_name_the_compile_step`.

## Alternatives considered

- **wasmtime's default (the major version).** This is the defect described above.
- **The exact wasmtime release and Ono's version.** Rejected in §3. The cost falls on every
  release, and it buys no check that wasmtime's configuration record does not already make.
- **`ModuleVersionStrategy::None` together with a check of Ono's own.** This throws away the
  check that wasmtime already performs before it maps anything.
