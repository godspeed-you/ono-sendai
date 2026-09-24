# ADR-0917: The compile step and the loader follow the session's environment, and the refusal names the store

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.2 §31.10, §31.79; K11P §12; ADR-0602, ADR-0870 §5, §6; ADR-0915
- Issues: #126
- Decided by: agent (autonomous)

## Context

ADR-0870 made `kuang-compile <component>` the remedy of `load.component_not_compiled`
(`Ono-Sendai-K11105`), and made `install plugin` run the same tool. Two independent reviews of
#126 found that the remedy and the install step could each disagree with the loader about which
store was meant:

- The loader and the install step resolved `XDG_CACHE_HOME`, `HOME` and `PATH` from the
  **process** environment. The shell keeps its own session environment (`Session::env`,
  `set env`), and every command it runs receives that environment instead. After
  `set env XDG_CACHE_HOME = …`, the remedy typed in the session wrote to one store and the loader
  read another. `install plugin` searched the login `PATH` and wrote where the session's
  `kuang-compile` would not.
- The refusal named the command but not the store. A `kuang-compile` run by hand in another
  environment (another terminal, `sudo`, a different `XDG_CACHE_HOME`) wrote somewhere the loader
  never looked, and the message did not show why.
- The K11105 raised by `install plugin` itself (the tool is not found, or the tool refused the
  bytes) carried no metadata. The contract says K11105's metadata names the component, the reason
  and the command.

## Decision

**1. The session's environment decides the store and the tool.** On every pipeline,
`Session::publish_host` computes the stores from the session's `XDG_CACHE_HOME` and `HOME`
(`compiled::stores_in(compiled::user_store_in(…))`) and hands them to the KUANG host. Three code
paths read from there: `load plugin` (`LoadConfig::compiled`), `inspect plugin`'s discovery
load, and install. Before a session has published, the host falls back to the process's stores.

`install plugin` runs `kuang-compile` the way it runs any command of the session: with the
session's environment (`env_clear` + the session's variables), found beside `ono` or on the
session's `PATH` (absolute entries only; see the previous commit). It passes the operator's store
explicitly as `--store`, so the store written is the store read, whatever the tool's own default
would be.

**2. The refusal names where it looked, and the command names the store when it has to.**
A `missing` refusal says which stores held no artifact. The `command`, in the message and in
the metadata, is `kuang-compile <component>` when the first store searched is the one
`kuang-compile` would choose in the environment the shell was started with. Otherwise it is
`kuang-compile --store <store> <component>`. The short form therefore stays short in the
ordinary case, which keeps acceptance cases 350 and 351 unchanged, and the long form is exact
whenever the short one would be wrong.

**3. The install-time refusal carries the contract's metadata.** When `install plugin`'s compile
step fails, K11105 carries:

| key | value |
|---|---|
| `reason` | `tool_unavailable`: no executable `kuang-compile` beside `ono` or on an absolute `PATH` entry, or it could not be started |
| | `compile_failed`: it ran and refused; its own words are in the message |
| `component` | the component in the package source |
| `command` | the command that compiles it, with `--store` |
| `engine` | as the load refusal says it |

`docs/contracts/errors.yaml` and `docs/contracts/kuang/errors.v1.yaml` list these next to the load
reasons.

## Consequences

- `set env XDG_CACHE_HOME = …` in a session moves the store that `load plugin`,
  `inspect plugin` and `install plugin` use. The remedy typed there works.
- The environment in which `ono` itself was started no longer decides anything about artifacts
  once a session exists.
- Tests:
  - `ono-cli/tests/plugins.rs`:
    - `should_load_a_component_from_the_store_the_sessions_environment_names`
    - `should_compile_into_the_store_the_sessions_environment_names_when_installing`
    - the `--store` and store-naming assertions
    - the install refusal's metadata
  - `ono-kuang-sdk/tests/compiled.rs`: the TestHost store is not the default, so its command
    carries `--store`.

## Alternatives considered

- **Only name `--store` in the refusal and keep reading the process environment.** Rejected. It
  would leave `install plugin` writing where the session's loader does not read, and the
  session's `PATH` ignored.
- **Always name `--store`.** Exact, but it lengthens every refusal and changes the documented
  short command (`MIGRATION.md` §10, cases 350 and 351) for no case in which the short one is
  wrong.
- **A `stores` list in the metadata.** The message names the stores, and the `command` names the
  one to write. A list would be a second place for a script to read the same fact from.
