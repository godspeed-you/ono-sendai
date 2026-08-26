# ADR-0002: Containerised acceptance as the definition of done

- Status: accepted
- Date: 2026-08-26
- Spec refs: 34, 35.4, 37, 50
- Decided by: agent (autonomous)

## Context

The project is built by autonomous agents that must decide for themselves whether a goal is
reached. A passing test suite is a weak signal for a *shell*: unit tests are satisfied by code
that has never been installed, never met a foreign process table, never run as a non-root user
and never been anybody's login shell. The user's requirement is a release-ready product, with no
MVP exit, so "done" needs a definition a machine can evaluate.

## Decision

- The **container is the referee**. `scripts/acceptance.sh` builds `docker/Dockerfile` and runs
  every case in `docker/acceptance/cases/` against the real `ono` binary inside the image, as an
  unprivileged user whose login shell is `ono`, with `--network=none`.
- **A capability is not delivered until an acceptance case proves it in the container**, added in
  the same increment as the capability itself (`docs/ACCEPTANCE.md` section 2).
- Cases are flat text files with `case:`, `run:`, `exit:`, `stdout-matches:`, `stdout-contains:`.
  A bespoke five-directive format needs no dependency, stays readable in a diff, and cannot grow
  into a framework that tempts anyone to test implementation details.
- Cases assert observable outcomes only, consistent with AGENTS.md section 11.
- The runtime image installs `procps`, `iproute2`, `util-linux` and `coreutils` so the provider
  phases have a real process table, real sockets, real mounts and real users to answer from.
- `scripts/release-check.sh` is the **stopping rule**: quality gate, then acceptance suite, then
  a scan of `docs/ACCEPTANCE.md` for unticked boxes. Any unticked box fails the check, so the
  checklist is executable rather than decorative.
- Podman is accepted as a drop-in for Docker; the script picks whichever exists.

## Consequences

- An agent cannot mistake a tidy repository for a finished product: the release gate reads the
  checklist and fails while anything remains open.
- Acceptance cases accumulate into a regression suite that describes the product in user terms,
  which is also the material for documentation and demos.
- Every capability costs a container round trip. That is the intended price; it is what keeps
  "works on the build machine" from being mistaken for "works".
- The image must stay small and buildable offline after the base image is cached, or the
  feedback loop degrades. Watch the build time as a real constraint.

## Alternatives considered

- **Unit and integration tests only** — rejected: they cannot detect that the binary is
  uninstallable, unusable as a login shell, or dependent on the build tree.
- **A Rust acceptance runner inside `cargo test`** — rejected for now: it would either need a
  container library or run outside the container, and the point is the boundary. Revisit if the
  case format outgrows five directives.
- **systemd inside the container** — deferred to phase C, where the `service` provider needs it.
  Bootstrapping a systemd container now would slow every run for a capability that does not
  exist yet.
