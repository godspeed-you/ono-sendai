# ADR-0967: A contract may vouch that an example needs no privilege

- Status: accepted
- Date: 2026-11-09
- Spec refs: spec §35.3, §36.5, §50; ADR-0935, ADR-0941
- Decided by: agent (autonomous)

## Context

The command conformance suite (ADR-0935) runs every documented example whose contracts say it is
safe in a hermetic scratch shell and holds the values to the declared output. Its classification
excludes every command whose contract says `privilege: conditional`, because "conditional" cannot
tell the generator whether *this* example needs privilege. That excluded most of the provider-
backed reads — `get file`, `get dir`, `get socket`, `get connection`, `trace file`, `trace user`,
`get container` — whose output is exactly the provider data the suite exists to hold to a schema
(#295). The system-facing provider layer is v0.6.4's subject, and the net did not reach it.

The contract knows what the generator cannot: which invocation shapes read what every user may
read. `get file /etc/hostname` needs no privilege on any Linux system; `get file /root` does. The
command's privilege stays `conditional`, because it depends on the argument; the example can say
which side of the condition it is on.

## Decision

**A command contract may list `unprivileged_examples`**: the subset of its `examples` the contract
vouches for as needing no privilege. For such an example the generator drops the one reason the
command's own `privilege: conditional` gave for skipping it; every other reason — another command
in the pipeline, a mutating verb, a redirection, a deferred schema — still applies, and the suite
runs the example unprivileged and holds its values to the declared output. The run is the proof:
an example that does need privilege fails in the suite, where the claim was made.

`spec-check` holds the marker to its meaning: every vouched example must be one the command
documents, and only a `conditional` command may vouch — a command whose privilege is `none` runs
anyway, and one whose privilege is `required` cannot be vouched out of it.

Where the environment, not the privilege, is what stops a vouched example — `get container` and
`get image` need a container engine, and a fresh environment runs none — the exemption register
says so, as it does for every example that names an object a fresh environment does not have.
The marker and the exemption together are the honest statement: the read needs no privilege, and
this environment has nothing to read.

The vouched examples name objects every Linux system has: `/etc/hostname`, `/etc`, `lo`, `root`,
and the caller's own socket table.

## Consequences

Easy: ten more provider-backed reads are held to their schemas on every gate run — `get file`,
`get dir`, `read file`, `trace file`, `enter file`, `enter dir`, `get socket`, `get connection`,
`trace interface`, `trace user`.

Hard: a vouched example is a claim about every machine the suite runs on, so it must name what
every machine has; `trace user root` reads what this user may read of root's processes and files,
and §35.3 makes the rest denied rather than invented, which the suite accepts.

Encoded by the generated `crates/ono-cli/tests/command_conformance.rs` entries for the ten
commands, `xtask/src/contracts.rs`'s marker checks, and the exemptions for the two container
reads in `docs/contracts/conformance/command_examples.yaml`.

## Alternatives considered

**Make the examples mappings** (`{run: …, privilege: none}`) — rejected: `examples` is read by the
reference generator, `help`, completion and the parser check as a list of lines; a second shape
would touch every reader for one bit of information.

**A per-example privilege probe in the generator** — rejected: the generator reads contracts, not
the machine, and a probe would make the generated suite differ by host.

**Relax `privilege: conditional` to `none` on the read commands** — rejected: the condition is
real. `get file /root` does need privilege, and the contract would be lying to `explain`.
