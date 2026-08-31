# Ono-Sendai Project History

How the shell was built, in the order the dependencies allowed. This document is about the
*construction* of Ono-Sendai; the product itself is described in [`README.md`](README.md), its
reasoning in [`PHILOSOPHY.md`](PHILOSOPHY.md), and what it currently guarantees in `docs/`.

Release-by-release changes live in `docs/releases/`. The live work board — what is in progress,
what is deferred, what is known broken — is [`docs/STATE.md`](docs/STATE.md). Neither is
duplicated here.

## The original specification

The project started from one document: `docs/ono_sendai_shell_spec_v0.2.md`, a narrative
specification detailed enough that command metadata, object schemas, the error taxonomy, the
grammar and the test matrices are all derivable from it. It is immutable and checksummed — where
it turned out to be ambiguous, silent or wrong, the deviation was recorded in an ADR and the
document was left exactly as written. The complete list of divergences is therefore one `grep`
away, in `docs/decisions/`.

The specification's §37 laid out ten phases, A to J, in dependency order. Each finished phase is
tagged in git with the acceptance case that proves it:

```bash
git tag -n99 phase-a          # what Phase A delivered, and what proves it
git switch --detach phase-a   # the tree exactly as that phase left it
```

## The phases

| Phase | Delivered |
|---|---|
| **A** | **Language and Unix shell foundation** — the parser, the execution model, quoting, jobs and signals. The point at which `ono` was already usable as a login shell, before it was interesting. |
| **B** | **Value system and native pipelines** — the typed stream engine. Values with types, units and null-ness travelling between stages instead of characters. |
| **C** | **Linux core providers** — process, file, user, mount, interface, socket and service, read from the system's own interfaces rather than from another tool's output. |
| **D** | **Consistency and discoverability** — the registries in `docs/spec/` as the single source for dispatch, `help`, semantic completion, `explain` and the generated reference. The phase that made the language learnable. |
| **E** | **Contextual systems interface** — the context stack: `enter`/`leave`, `@`-reuse, and a prompt that says which context you are in. |
| **F** | **Live system semantics** — `watch`, event streams, tables that update in place, and native stages that can be backgrounded as jobs. |
| **G** | **Relationship graph** — `trace`, graph values, and the provenance and confidence that keep an observed edge distinguishable from a guessed one. |
| **H** | **Remote links** — the protocol, the agent (`ono --agent`), capability negotiation, and remote hosts that behave like local contexts. |
| **I** | **KUANG/11 extension runtime** — the capability broker, the audit trail, the SDK, and a deterministic test host every package must pass. |
| **J** | **Advanced TUI views** — `view`, and a cursor that sets `@`, added only where the semantics justified a full-screen view. |

## v0.3 — External command adaptation

The first enhancement layered on the base specification
(`docs/ono_sendai_shell_spec_v0.3_external_command_adapters.md`).

`ps`, `ip`, `ss`, `lsblk`, `findmnt`, `lsns`, `stat`, `df`, `find`, `git`, `lsof`, `curl`,
`systemctl` and `journalctl` became typed *when a typed consumer follows* — the adapter rewrites
the invocation to the tool's own machine-readable form and decodes it into the same schemas the
native providers use — and stayed raw bytes otherwise. `raw` bypasses the layer unconditionally;
`adapt` demands structure and fails visibly rather than guessing.

This is the release that made "Unix remains underneath" a technical property rather than a
promise.

## v0.4 — The spatial systems interface

The second enhancement (`docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md`), and the
one that gave the machine a geography.

Fourteen commands carry the model — `look`, `near`, `find place`, `enter`, `follow`, `jump`,
`back`, `up`, `home`, `trail`, `map`, `map links`, `pin`, `unpin` — and the tranche's house rule
was that every acceptance scenario must discover its object *cold*, by place and property alone,
before anyone types its name. Places got an identity that survives pid reuse, a denied
neighbourhood became a reported state rather than an absence, and a linked host became a place
with its own root.

## How it was built

Strictly test-driven and largely agent-driven, under the contract in
[`AGENTS.md`](AGENTS.md): no production code without a failing test first, the specification
immutable, every decision the specification left open made autonomously and recorded as an ADR,
and a containerised acceptance suite — the real binary, installed as an unprivileged user's login
shell, with the network cut — as the referee for whether a capability exists at all.

`scripts/release-check.sh` prints `release-check: the shell is release-ready`, and that line,
not anyone's judgement, is the project's definition of done.
