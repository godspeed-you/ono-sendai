# ADR-0113: Container mutations are engine requests, and the engine's status is the outcome

- Status: accepted
- Date: 2026-08-27
- Spec refs: §9.1 (Container), §11.5, §11.6, §16.5, §17.1, §43, §52; ADR-0006, ADR-0068,
  ADR-0084, ADR-0112
- Decided by: agent (autonomous)

## Context

`container.yaml` declares `start`, `stop`, `restart`, `remove` and `set` over `container.manage`.
ADR-0068 §3 binds a mutating command exactly when the provider for its target advertises the
capability and answers the verb in `act`; ADR-0112 delivered the provider without any of them.
The RED suite fixes the observable contract: one `ono.action-result/1` row per target whose
`operation` is the command id, `success` with `changed: true` when the engine did what it was
asked, a `failed` row carrying `E0301` for a container the engine does not know and `E0302` for
one it refuses to touch, and the request that reached the engine (`POST …/start`, `POST
…/update`, `DELETE /containers/…`).

## Decision

### 1. One HTTP request per action, on the engine's own endpoints

| operation | request | arguments carried |
|---|---|---|
| `start` | `POST /containers/{id}/start` | — |
| `stop` | `POST /containers/{id}/stop?t=N` | `--timeout` as whole seconds |
| `restart` | `POST /containers/{id}/restart?t=N` | `--timeout` as whole seconds |
| `remove` | `DELETE /containers/{id}?force=…&v=…` | `--force`, `--volumes` |
| `set` | `POST /containers/{id}/update` with `{"Memory": …, "NanoCpus": …}` | `--memory` in bytes, `--cpus` × 10⁹ |

`{id}` is the identity the object was resolved to — the engine's full id — so an action reaches
exactly the container that was enumerated, never a name that was reassigned in between
(spec §27.3, ADR-0015). A `set` that names no property is refused before resolution by the
generic mutation (ADR-0084); one that names an unknown property is `provider.unsupported`.

### 2. The engine's status code is the per-target outcome

- `2xx` — `success`, `changed: true`. The engine reports no "nothing to do" on these endpoints
  except through `304`, so a `2xx` is a change.
- `304 Not Modified` — `skipped`: the container was already started or already stopped.
- `404` — `failed` with `io.not_found` (E0301).
- `401`/`403` — `failed` with `io.permission_denied` (E0302): the socket's owner said no.
- `409` — `failed` with `safety.confirmation_required` (E0701): the engine refuses without
  `--force`, which is the same shape as the shell's own bulk guard — an explicit word is needed.
- anything else — `failed` with `provider.unavailable` carrying the engine's message.

A `--dry-run` answers `skipped` with the request that would have been sent. The budget for a
request is thirty seconds plus the stop timeout the user asked for, since the engine answers a
`stop` only once the container has stopped.

### 3. `container.manage` is advertised without elevation

`docs/contracts/capabilities.yaml` gives it `elevation: conditional`: whether a user may act is
decided by the socket's permissions, and the engine's `403` is the structured form of that
decision. The provider therefore advertises the capability as an ordinary one and lets the
engine refuse, rather than guessing from the uid.

## Consequences

- `start|stop|restart|remove container web | to json` is one ActionResult row and the engine
  saw the request; `stop container nope` is a `failed` row with E0301 and exit status 1; a
  refusing engine gives a `failed` row with E0302 and exit status 1. `explain stop container
  web` names `container-engine` as the provider and `mutate` as the risk. Tests:
  `crates/ono-cli/tests/containers_packages_missing.rs::should_{start,stop,restart,remove}_a_container_through_the_engine_api_when_the_runtime_accepts`,
  `::should_update_a_memory_limit_through_the_engine_api_when_setting_a_container`,
  `::should_fail_with_not_found_when_stopping_a_container_the_runtime_does_not_know`,
  `::should_fail_with_permission_denied_when_the_runtime_refuses_the_stop`,
  `::should_name_the_provider_and_the_risk_when_explaining_a_container_stop`.
- `kill container` is not declared and not delivered; `stop --timeout 0` is the engine's kill.
- `pause`/`unpause` and `exec` are not declared by the contract and stay undelivered.

## Alternatives considered

- **Inspecting before acting, to answer `skipped` from the state.** Rejected: the engine already
  answers `304` for exactly that case, and a second request would race the first.
- **Refusing mutations from a uid that does not own the socket.** Rejected: group membership
  (`docker`), rootless sockets and ACLs make the uid a poor predictor; the engine's answer is
  the truth and arrives in milliseconds.
