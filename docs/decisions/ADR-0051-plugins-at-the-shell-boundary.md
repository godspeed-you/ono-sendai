# ADR-0051: Plugins at the shell boundary

- Status: accepted
- Date: 2026-08-26
- Spec refs: §31.5, §31.8, §31.10, §31.22, §31.64; ADR-0011, ADR-0040, ADR-0041
- Decided by: agent (autonomous)

## Context

The supervisor deliberately stops short of the shell: it exposes contract-shaped contribution
tables and leaves discovery, session state and invocation routing to the integration step
(ADR-0040). This records how the shell does them.

## Decision

### Installed is a directory layout

A package is installed when a directory under `ONO_PLUGIN_PATH` (else
`~/.config/ono/plugins`) holds its `manifest.yaml` and its runtime entry. `get plugin` reads
that set and overlays the session's runtime states (spec §31.8); a directory whose manifest
does not validate is reported as a failure entry, never silently skipped — an installed package
that cannot load is a fact about this machine, and `ono.*` claims fail validation exactly as
§31.5 requires. The install/verify/signing pipeline of §31.26 stays a later increment
(ADR-0040); until then, installing is file placement, which is honest about what it is.

### Loaded is session state

`load plugin <id>` validates, negotiates — a denied required capability refuses before the
binary ever starts — and keeps the `LoadedPlugin` on the session, exactly as links and frames
are session state. The default policy is the broker's default deny: the example package loads
`degraded` with its optional capabilities denied, and `load` prints the state and the
contributed command ids, so what the package can and cannot do is visible at the moment it
becomes reachable.

### `<package>:command` is the module namespace of ADR-0011

A head whose namespace is not `ono:`/`exec:`/`fn:` resolves against the loaded packages — full
id or its last segment — and invokes the contribution over the plugin protocol. The values it
streams seed the rest of the pipeline exactly as a native producer's would, so
`echo:emit --count 3 | to json` is ordinary pipeline composition with a subprocess at its head.
An unloaded package's namespace is a structured refusal naming `load plugin`.

### K-codes travel in the message, for now

Folding the K11 error family into `ono_core::ErrorCode` is its own increment (ADR-0040 §3);
until then a supervisor error crosses as `provider.unsupported` with the K-code in the message —
visible, greppable, never silently dropped.

## Consequences

- The whole §31 path is exercisable on any machine with two files and one env var, which is
  what the acceptance case does in the container.
- `get plugin | where state == loaded` does not yet compose (the session commands print rather
  than stream); registry-backed `ono.plugin/1` records are the follow-up, with `get audit`.
- Tests: `ono-cli/tests/plugins.rs` — discovery, load-and-invoke through a pipe, and the
  unloaded-namespace refusal.

## Alternatives considered

- **Mount contributed commands into the CommandRegistry at load.** Right eventually (§31.64
  origin tracking); today the registry is `&'static` and contributions are session-scoped, so
  the module-namespace route delivers the semantics without a static-lifetime redesign. The
  registry integration rides with the `ono.plugin/1` record work.
- **A plugin daemon shared across sessions.** Rejected: §31.10's lifecycle is per-supervisor,
  and a session that exits taking its plugins with it is the least surprising ownership.
