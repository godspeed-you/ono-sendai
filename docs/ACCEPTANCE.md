# ACCEPTANCE

**This document defines when the work is finished. An agent run ends when
`scripts/release-check.sh` passes — and not one step earlier.**

There is no MVP milestone in this project and no proof-of-concept exit. A shell that
demonstrates the idea is not the deliverable; a shell a person can set as their login shell and
keep is the deliverable. Everything below is written so that a machine, not a judgement call,
decides whether that has been reached.

---

## 1. The three referees

| Referee | Command | Answers |
|---|---|---|
| Quality gate | `scripts/gate.sh` | is the code correct, linted, tested, documented, contract-consistent? |
| Acceptance suite | `scripts/acceptance.sh` | does the real binary do the real thing in a clean Linux container? |
| Release gate | `scripts/release-check.sh` | are the first two green **and** is the checklist in section 4 fully ticked? |

The quality gate proves the code is sound. It cannot prove the product exists: unit tests pass
in a workspace that has never been installed anywhere. That is what the container is for.

## 2. Why the container is the referee

`scripts/acceptance.sh` builds `docker/Dockerfile` and executes every case in
`docker/acceptance/cases/` against the `ono` binary inside the resulting image, as an
unprivileged user whose login shell is `ono`, with networking disabled.

This is deliberately hostile to the ways a shell can be secretly broken:

- it is a machine that has never seen the source tree, so nothing depends on the build directory;
- the user is not root, so privilege boundaries are met the way real users meet them;
- the process table, services, sockets, mounts and users are the container's own, so provider
  output is checked against a system nobody tuned for the test;
- `ono` is the login shell of a real account, so it must survive being started as one.

**A capability is not delivered until an acceptance case proves it in the container.** Passing
unit tests are necessary and not sufficient. Add the case in the same increment as the feature.

## 3. What each case must be

Cases assert what a user observes — printed output, exit status, resulting system state — never
how the result was produced (AGENTS.md section 11). A case that has to change when the
implementation is restructured is a defective case.

Format and directives: see `docker/README.md`.

## 4. Release checklist

Tick an item only when it is proven by an automated case or test that runs in the gate. An
unticked box means `scripts/release-check.sh` fails, which means the run continues.

### 4.1 Phase completion

Each phase's success criterion from spec section 37, each proven by a named acceptance case.

- [x] **A — Unix shell foundation.** `ono` can be used interactively in place of Bash for
      ordinary work: parsing, quoting and escaping, environment variables, `cd`, redirection,
      external commands and pipelines, exit status, signals, job control, history, config.
      Proven by `010-replaces-bash-for-ordinary-work`, with `020`–`029` covering each part:
      external commands and status, cwd and environment, redirection, pipelines, PTY ownership,
      quoting and expansion, an interactive session, job control, history across a restart,
      quiet startup, restricted configuration, and the prompt.
- [x] **B — Value system and native pipelines.** Typed values flow end to end through `where`,
      `select`, `sort`, `take`, `skip`, `each`, `count`, `measure`, with JSON/YAML/CSV/text
      conversion, backpressure, and rendering separated from data.
      Proven by `041-typed-values-flow-end-to-end` (every phase-B transform and format against
      parsed data) and `040-object-pipeline` (the §12.3 boundary in both directions).
- [x] **C — Linux core providers.** `process`, `file`/`dir`, `user`/`group`, `env`,
      `mount`/`filesystem`, `interface`/`route`/`neighbor`, `socket`/`connection` and
      `service` are answered from the kernel and systemd, not from parsed text.
      Proven by `042-inspection-without-text-parsing` (a typed field selected from each), with
      `docs/spec/providers/*.yaml` and `ono-cli/tests/providers.rs` pinning what each provider
      advertises.
- [x] **D — Consistency and discoverability.** Command, verb and schema registries exist under
      `docs/spec/`; `help`, completion, `type`, `inspect` and `explain` are driven by them;
      docs and provider conformance tests are generated from them.
      Proven by `043-discoverable-from-the-shell` (help, type, inspect, explain from the
      registries; explain never executes) and `044-semantic-completion` (a declared target
      completed on a real terminal), with `docs/reference/` staleness and provider drift both
      failing the gate.
- [ ] **E — Contextual systems interface.** Context stack, `enter`/`leave`, object-aware
      selectors, prompt and HUD, interactive selection, structured reuse of recent results.
- [ ] **F — Live system semantics.** `watch`, the event/snapshot model, in-place rendering,
      native background jobs, stable object identity.
- [ ] **G — Relationship graph.** Graph values, relationship providers, `trace` for process,
      service and socket, tree and graph rendering, provenance and confidence.
- [ ] **H — Remote links.** Remote protocol, agent, SSH fallback, provider negotiation, security
      model, remote prompt, multiplexed streams.
- [ ] **I — KUANG/11 extension runtime.** The production path of spec section 31: manifests,
      capability model, isolation, host API, contribution model, audit trail, SDK, test host and
      conformance suite.
- [ ] **J — Advanced TUI views.** Only where semantics justify them, per spec section 37.

### 4.2 Per-capability quality bar (spec section 50)

For **every** advertised command, in the container:

- [ ] `help` is complete for every command, and every documented example parses and executes.
- [ ] Completion produces correct candidates for every command, option and argument position.
- [ ] Every command's output schema is inspectable via `inspect`/`type` and matches what it emits.
- [x] Behaviour is deterministic when output is redirected or the terminal is not a TTY —
      `034-redirected-output-is-deterministic` runs the same script to a terminal, to a file and
      through a pipe and requires all three to be byte-identical, with no escape sequence
      reaching a file.
- [x] Every failure is a structured error of the taxonomy in spec section 43, never a bare
      string — `033-errors-are-structured` checks that each failure names a code of the form
      `Ono-Sendai-ENNNN`, for the resolution, I/O, parse and type families.
- [ ] Privilege boundaries and race conditions are covered by tests, including denial paths.
- [ ] No provider parses unstable human-readable text except where declared an adapter fallback.
- [ ] Unknown data is `null`, never fabricated and never silently zero.
- [ ] Output looks intentional in an 80-column and in a 200-column terminal.

### 4.3 Performance (spec section 34)

Measured in the container, on the pathological fixtures of spec section 34 — tens of thousands
of processes and paths, slow NSS, high-latency links, huge stdout, unbounded streams:

- [x] cold start < 100 ms (target < 50 ms) — `060-performance-budgets`, measured as a median of
      40 runs in the container and asserted against the 50 ms *target*, not the 100 ms cap
- [ ] warm prompt < 30 ms
- [ ] keystroke to render < 8 ms typical
- [ ] first completion results < 50 ms from local metadata
- [x] parse and highlight update < 5 ms for ordinary command lines — `060-performance-budgets`
      bounds a whole pipeline run, startup included, at 50 ms; the parser's own measurement
      (2.4 microseconds for a four-stage line) is in `crates/ono-parser/tests/robustness.rs` and
      the editor's keystroke-to-frame budget in `crates/ono-editor/tests/latency.rs`
- [ ] first rows of `get process` < 50 ms
- [ ] renderer updates only when state changes

### 4.4 Interoperability and safety

- [x] External programs run correctly under a PTY, including full-screen applications —
      `024-pty-applications` proves the child owns the real terminal, and
      `031-full-screen-application` runs a pager that takes the terminal into raw mode and the
      alternate screen, draws, and gives it back with the shell still usable.
- [x] Signals, job control and process groups behave as they do under Bash for foreground work —
      `030-signals-and-process-groups` proves the foreground command runs in its own process
      group and dies when that group is signalled; `025-job-control` covers background, `jobs`
      and `fg`; and `crates/ono-cli/tests/signals.rs` drives a real Ctrl-C through a real
      pseudo-terminal, where the shell survives it, the command does not, and the status is 130.
- [ ] Text, bytes and objects are never silently confused at an interop boundary.
- [ ] Destructive operations show scope before acting; privilege and remote target are visible.
      Partly proven: `032-resolution-is-inspectable` covers the resolution half — which binary a
      name reaches, including a shadowing one earlier in `PATH` (ADR-0015 T10, T11).
- [ ] Fuzzers run clean over parser, serializers, remote protocol, plugin protocol and the
      procfs/netlink decoders.
- [ ] The threat model of spec section 49 has a test for each stated risk.

### 4.5 Delivery

- [x] `ono` installs and runs as a login shell in the container as an unprivileged user —
      `003-login-shell` and every interactive case, which run as the unprivileged `case` user.
- [x] Startup loads no plugin eagerly and queries no network-backed configuration —
      `027-startup-is-quiet`, in a container with networking disabled.
- [x] Generated documentation is reproducible from the registries and committed docs match it —
      `xtask/tests/reference.rs` regenerates every page and requires the committed files to be
      identical, and `spec-check` runs the same comparison in the gate (ADR-0018).
- [ ] `docs/STATE.md` has an empty *In progress* section and no unexplained *Deferred* entries.
- [ ] Every `#[ignore]`d test is either removed or justified in *Deferred* with an ADR.

## 5. Stopping rule

An agent stops when `scripts/release-check.sh` prints `release-check: the shell is
release-ready`. Any other outcome means there is a next task in `docs/STATE.md`.

Running out of easy work is not a stopping condition. Neither is a green quality gate, a passing
acceptance suite on its own, a completed phase, or a tidy-looking repository. If a box in
section 4 is unticked, the work is unfinished, and the next increment starts.
