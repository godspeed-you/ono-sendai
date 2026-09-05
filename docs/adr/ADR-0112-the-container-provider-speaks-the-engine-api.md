# ADR-0112: The container provider speaks the engine API over the runtime's socket

- Status: accepted
- Date: 2026-08-27
- Spec refs: §5, §8.3, §9.1 (Container), §10.5, §23, §31.57, §35.3, §39 (question 15), §50;
  ADR-0012, ADR-0015, ADR-0075
- Decided by: agent (autonomous)

## Context

Spec §9.1 promises `get container`, `get image` and the container mutations, and §37 schedules
no phase for a container provider, so `docs/contracts/commands/container.yaml` carried every command
as `phase: planned` and `targets.yaml` gave `container` and `image` no schema. The RED suite
`crates/ono-cli/tests/containers_packages_missing.rs` asserts the delivered behaviour: a fake
Docker Engine API on a Unix socket named by `DOCKER_HOST`/`CONTAINER_HOST`, `E0401` naming the
sockets tried when none answers, and canonical `ono.container/1` / `ono.image/1` records built
from the engine's JSON.

Spec §23 and §31.57 fix the method — a provider speaks the daemon's API and never parses the
human output of a tool — but fix neither how the runtime is found, nor what the records carry,
nor what "the provider" is when Docker and Podman both serve the same API (§39, question 15).
This ADR records those decisions.

## Decision

### 1. One provider, `container-engine`, for every runtime that serves the Engine API

`crates/ono-provider-container` is a new crate: a hand-written HTTP/1.1 client over a Unix
socket (`Content-Length` and chunked bodies, one request per connection) and a `Provider` that
reads `GET /containers/json?all=1`, `GET /containers/{id}/json` and `GET /images/json`. It never
runs `docker` or `podman`. The provider id is **`container-engine`**: Docker and Podman are not
two providers but two sockets serving one API, and which one answered is a fact about the
socket, recorded in the record's provenance source (`unix:///run/podman/podman.sock`). Two
runtimes on one machine are served in socket order; qualifying them (`docker:container`) is left
to the day a user has both and needs to say which.

### 2. The runtime is found the way `docker` and `podman` find it

`DOCKER_HOST` and `CONTAINER_HOST` name a `unix://` socket. When either is set, **only** what
they name is tried — as `docker` does not fall back to `/var/run/docker.sock` when
`DOCKER_HOST` points elsewhere — so a configured runtime that is down is reported as down,
never quietly replaced. A URL of another scheme (`tcp://`, `ssh://`) is reported as a transport
this provider does not speak rather than ignored. When neither is set, the well-known sockets
are tried in order: `$XDG_RUNTIME_DIR/docker.sock`, `$XDG_RUNTIME_DIR/podman/podman.sock`,
`/var/run/docker.sock`, `/run/podman/podman.sock`.

The probe is a plain connect, made afresh on every `availability()` and every query: a runtime
started after the shell is found the next time it is asked for. When nothing answers, the
provider is `Unavailable` with a reason naming every socket tried and the variable each came
from, and the registry turns that into `Ono-Sendai-E0401`. An empty list is never the answer
to "no runtime" (spec §10.5, §35.3).

### 3. `ono.container/1` and `ono.image/1`

`docs/contracts/schemas/container.v1.yaml`: identity `[id]` (the engine's full id); `name` is the
first entry of `Names` without its leading slash — a label the engine lets a user change, so not
identity; `image` (the reference), `image_id` (the digest that identifies the `ono.image/1`),
`state` (the engine's lifecycle word, `unknown` where the schema does not model it), `created`,
`labels`. The listing and the inspection carry these under different keys (`Names`/`Name`,
`Image`+`ImageID`/`Config.Image`+`Image`, `State`/`State.Status`, Unix seconds / RFC 3339); the
provider reads both shapes into the one record.

`docs/contracts/schemas/image.v1.yaml`: identity `[id]` (the content digest); `reference` is the
first `RepoTags` entry, null for an untagged image (`<none>:<none>` is the engine's sentinel for
"none", not a tag); `tags`, `size` as a bytesize, `created`.

`docs/contracts/schemas/container-event.v1.yaml` is the ADR-0078 envelope for `watch container`,
keyed `container`.

### 4. `get container` lists everything the engine knows

The `--all` option is removed from `ono.container.get`. `docker ps` hides stopped containers
by default and a shell that copied the habit would make `get container | where state ==
running` — the contract's own example — meaningless. Every container is listed and the pipeline
filters (spec §5). `get container <id>` and `enter container <name>` push the handle down as
`GET /containers/{handle}/json`, which the engine resolves by id, id prefix or name; `get image
<reference>` filters the listing by tag or digest prefix.

### 5. The contracts move to phase C

Every command in `container.yaml` is `phase: C`; `targets.yaml` gives `container` and `image`
their schemas. The commands this increment does not yet bind — the mutations, `enter`, `watch`,
`trace` — are delivered by the following increments (ADR-0113, ADR-0114) and are bound only
when the provider advertises their capability (ADR-0068 §3) or their id is registered, so a
phase-C command with no implementation still answers `E0101` rather than a stub.

## Consequences

- `get container | select name state image | to json` against an engine socket is the engine's
  listing, mapped; `get image` likewise; both answer `E0401` naming the socket when nothing
  listens. Tests: `crates/ono-cli/tests/containers_packages_missing.rs::should_report_provider_unavailable_when_no_container_runtime_answers`,
  `::should_report_provider_unavailable_when_no_runtime_answers_for_images`,
  `::should_list_containers_from_the_engine_api_when_a_runtime_socket_answers`,
  `::should_list_images_from_the_engine_api_when_a_runtime_socket_answers`; the HTTP client,
  endpoint discovery and record mapping are unit-tested in the crate.
- `docs/contracts/providers/container-engine.yaml` declares the provider; the conformance suite
  in `crates/ono-cli/tests/providers.rs` holds it to the declaration.
- The client speaks only `unix://`. A `tcp://` or `ssh://` `DOCKER_HOST` is refused with the
  reason; adding a transport is adding one `connect` path, not a redesign.
- The provider is Docker-API-shaped. A runtime that serves only the CRI or the containerd API
  is a different provider.

## Alternatives considered

- **Running `docker ps --format json` / `podman ps --format json`.** Rejected: spec §23 and
  §31.57 prefer the daemon's API where one exists, the CLI formats differ between the two tools
  and between versions, and the CLI would need to be installed where the socket alone suffices.
- **A full HTTP client crate (`hyper` + `hyperlocal`).** Rejected: the engine API needs one
  request per connection and two body framings; a full stack is a dependency every cold start
  pays for (spec §34) to save 200 lines that are tested in isolation.
- **Two providers, `docker` and `podman`.** Rejected for now: they would be the same code
  behind two ids, and the conformance suite would have to tolerate whichever set a machine has.
  Spec §39's question 15 stays open until a user needs both at once.
- **Honouring `--all` as `docker ps` does.** Rejected (§4 above).
