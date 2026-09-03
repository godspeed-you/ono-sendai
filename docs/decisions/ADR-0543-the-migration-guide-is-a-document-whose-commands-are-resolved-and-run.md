# ADR-0543: The migration guide is a document whose commands are resolved and run

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §63.1–§63.5, §66.8, §4.2, §4.4, §8.2, §8.3, §9.4, §9.5, §16.2, §18.1, §38.1
- Decided by: agent (autonomous)

## Context

§63 lists five migrations and §66.8 makes one of them — "remote client authorization migration is
documented" — a release criterion. The repository had no migration document at all; the Wiki's
Install page had an "Upgrading" section that stopped at v0.4.0.

Only one of the five needs an operator to do anything, and it is the one that breaks a working
setup if they do not: v0.4.1 intentionally stops accepting anonymous TLS clients, so a direct TCP
agent that worked yesterday refuses every client today. The other four are a description of
behaviour that changed underneath: the identity file moves itself, a plugin that only ran because
a confinement control silently failed now refuses to start, a v0.4.0 client fails safely rather
than downgrading, and a test that returned early is now a declared skip.

A migration guide is the one document nobody reads until they are already stuck. That is what
makes a stale command in it expensive: the reader has an agent refusing connections and a command
that does not exist, and no way to tell which of the two is wrong.

## Decision

**`docs/MIGRATION.md` is the guide**, structured as §63's five paths in §63's order, with the
required one second and marked as required. The Wiki's Install page carries the same migration in
its "Upgrading" section, at the length a Wiki reader wants, and links to the full guide.

**Every `ono` invocation the guide prints inside a fenced block is resolved against the
contracts** by `xtask::reference::check_migration_guide`, on every gate run:

- a command spelling has to be one the registry answers to — a renamed verb or target turns the
  gate red where the guide is, not where a reader finds out;
- a capability id named after `--allow` has to be one `docs/spec/capabilities.yaml` declares,
  because §9.5 grants exact ids and a guide printing a retired one teaches an operator a command
  that fails *after* they have already turned anonymous access off;
- a flag has to be one the binary's own usage text lists;
- and the guide has to print at least one invocation, because a migration described without
  commands is one nobody can follow.

**The sequence is run, not only resolved.**
`crates/ono-cli/tests/client_keys.rs::should_accept_the_migration_sequence_the_documentation_prints`
reads the scripts out of the guide, substitutes the fingerprint `--print-peer-key` actually
produced for the guide's `sha256:...` placeholder, runs them against the real binary and asserts
the store afterwards says what the guide said it would — observation by default, and exactly the
one action the guide grants. Reading the scripts from the document rather than retyping them is
the point: a test with its own copy passes while the document rots.

**§63.2 end to end is case `182-unknown-client-is-refused`**, which already prints a peer key,
authorizes it on the agent host, links over TCP and reads — and refuses the client nobody
authorized, which is the half a migration guide exists to prevent somebody meeting by surprise. A
second case running the same sequence would be a second copy of an existing proof.

## Consequences

- The one required migration is documented in two places, both checked, and one of them is where
  a user upgrading a package will actually look.
- The guide states the refusal an operator meets if they skip the step, verbatim, including that
  it carries the fingerprint to add — so the migration is recoverable from the error message even
  by somebody who never read this page.
- `docs/MIGRATION.md` is versioned by section rather than by file. The next release adds a
  heading; nothing about the check assumes there is only one.
- The resolver reads the first two words of each statement, which is how the registry is addressed
  and how every example in this repository writes one. A guide that printed a pipeline whose
  second stage was renamed would not be caught. The alternative is binding every stage, which
  `xtask::contracts::check_examples` already does for `ono` code blocks — this check is about the
  shell invocations a migration prints, which are the ones an operator pastes.
- §4.8.12's box for this issue named the tests correctly; both now exist under those names.

## Alternatives considered

- **Put the migration only in the Wiki.** Rejected: §66.8 makes it a release criterion, and the
  Wiki is a separate repository no gate run can reach (ADR-0536).
- **Put it in the release notes.** Rejected as the only home: release notes are read once, at
  release. Someone upgrading eight months late needs the same page, and `docs/releases/` is
  organised by date rather than by what breaks.
- **Write a new acceptance case for §63.2.** Rejected: case `182` already runs that exact
  sequence end to end. A second case would be a second copy of a passing proof, and AGENTS.md §4
  is against exactly that.
- **Check the guide's commands by executing all of them.** Rejected for the agent commands:
  `--agent --listen` needs a second process and a port, which is what the acceptance suite is for.
  The commands a *reader types on their own machine* are executed; the ones they type on a server
  are resolved.
