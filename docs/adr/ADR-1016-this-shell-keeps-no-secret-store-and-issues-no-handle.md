# ADR-1016: This shell keeps no secret store, and `secrets.request` issues no handle

- Status: accepted
- Date: 2026-11-10
- Spec refs: §17.5, §31.12, §31.20, §31.21, §35.3; K11P §26 (Kubernetes reference mapping,
  `secret.use`), §27.2; `docs/contracts/kuang/protocol.v1.yaml` (`secrets.request`,
  `secrets.release`, `network.request`); `docs/contracts/kuang/capabilities.v1.yaml`
  (`secret.use`); issue #203; ADR-0015, ADR-0568, ADR-0573
- Decided by: agent (autonomous)

## Context

Issue #203 asks for `secrets.request` to be served, or for an explicit, recorded reason why it is
not. ADR-0568 left it at `provider.unavailable` with "there is no store". The decision now has to
say what a store would be and whether one can be honest and safe within v0.6.5, whose scope is
stabilisation: no new product surface beyond what the contracts already name.

A secret store in the sense of §31.20 is three things, and each must exist for a handle to mean
anything:

1. **A source the operator controls.** Where a secret comes from. No contract names one: there is
   no `set secret`, no secret target, no configuration key, no file the shell reads secrets from.
   §31.20 rules out the obvious substitute — "Plugins MUST NOT receive bulk access to environment
   variables … or secret stores merely because they run inside a shell" — and reading
   `API_TOKEN` from the shell's environment because a package asked for `api-token` would be that
   access by another name.
2. **A handle the broker issues and expires.** This exists: the supervisor checks the `secrets`
   scope, audits the name, maps an opaque handle to it, and `secrets.release` drops it.
3. **A host operation that consumes the handle**, injecting the material without exposing it
   (§31.20: `http.request(..., auth=secret#A72)`). The contracts name exactly one:
   `network.request`'s `credential`. ADR-0573 decided the host does not serve `network.request`
   (a request is the package's own protocol over `network.connect`), and deferred `credential`
   and the `tls` protocol "with the secret store". `process.exec` takes plain environment strings;
   letting a handle be expanded there would hand the material to a program the package chose,
   which is the raw exposure §31.20 reserves for "a stronger capability and explicit warning".

## Decision

**`secrets.request` stays `provider.unavailable`, and says why.** No handle is issued for a name
inside the granted scope, because a handle would name a secret nobody could have stored and that
no call could use; issuing one anyway would be a fake — a package would believe it held a
credential. The refusal names the three reasons in one sentence and points here. Everything
around it stays as it is and is now pinned by tests:

- a name outside the granted `secrets` scope is `capability.scope_violation` at the broker,
  audited with the name that was asked for, before the shell is asked anything;
- a package denied `secret.use` is refused at the broker;
- the shell's environment is never consulted: a variable that happens to carry the secret's name
  issues no handle and its value appears nowhere;
- there is no `secrets.list`, and a package learns nothing about names it was not granted.

A store arrives as one change with all three parts: an operator-facing source with its own
contract (and its own threat model: at rest, in memory, in history, in `--agent` output), a
consumer — `network.request` with `credential`, which means reversing ADR-0573's decision 1, or
TLS on `network.connect` — and handle expiry tied to the invocation that requested it. That is new
product surface and belongs to a milestone that plans it, not to v0.6.5.

## Consequences

- K11P §26's Kubernetes reference permission "Use Kubernetes credentials" (`secret.use`) can be
  granted, is audited and is scope-checked, and still delivers no credential. A Kubernetes package
  today reads its credentials from its kubeconfig under its scoped `filesystem.read` grant, which
  K11P §27.2 already describes as the honest boundary of this tier.
- Nothing about the refusal can leak: there is no material in the shell to leak.
- Tests: `crates/ono-cli/tests/host_services.rs`
  (`should_refuse_a_granted_secret_request_and_say_why_this_shell_keeps_no_store`,
  `should_never_answer_a_secret_request_from_the_environment`,
  `should_refuse_a_secret_outside_the_granted_names_and_record_the_name_asked_for`);
  `crates/ono-cli/tests/host_domains.rs`
  (`should_say_the_shell_has_no_secret_store_and_record_the_attempt`); acceptance cases 178 and
  213.

## Alternatives considered

- **Environment variables as the store** (`ONO_SECRET_<NAME>`). Rejected: §31.20 forbids exactly
  this ambient access, every child process inherits the environment, and nothing would consume
  the handle anyway.
- **A file of secrets under the config directory, mode 0600.** Rejected for this milestone: it is
  a new operator surface with no contract, no way to set or rotate a secret from the shell, no
  redaction story for `--agent` and history, and still no consumer.
- **A handle that `process.exec` expands into the program's environment.** Rejected: the material
  reaches a program the package chose, which is raw exposure without the stronger capability and
  warning §31.20 requires.
- **Issuing handles that resolve to nothing.** Rejected: a fake store. The package would hold a
  handle and every use of it would fail later, further from the cause.
