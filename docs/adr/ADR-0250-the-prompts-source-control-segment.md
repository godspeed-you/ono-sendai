# ADR-0250: The prompt's source-control segment is a branch, read from the checkout

- Status: accepted
- Date: 2026-08-29
- Spec refs: §4.2 (the prompt as a HUD, and its suggested `vcs` segment), §14.3, §34 (the prompt
  budget), §30/§47 (settings); ADR-0010, ADR-0094
- Decided by: agent (autonomous)

## Context

Spec §4.2 suggests six semantic prompt segments and gives `vcs` as `git:main*`, "optional
source-control state". Five of the six were implemented; the string `vcs` appeared nowhere in
`crates/`. The same section constrains what may be added: "The prompt SHOULD remain short.
Information that is not actionable SHOULD not be shown permanently."

Two questions had to be answered before it could be written: where the information comes from,
and how much of it is shown.

## Decision

**The segment is `git:<branch>`, read from the checkout's own files.**

`vcs_branch` walks up from the working directory looking for `.git`, follows the `gitdir:`
pointer when it is a file — a worktree or a submodule — and reads `HEAD`. A `ref: refs/heads/x`
gives `x`; a detached `HEAD` gives the first seven characters of the commit, which is what every
other tool shows and what a person can compare. Anything else gives nothing.

**No `git` process is forked.** The prompt is drawn before every line the user types, and spec
§34 budgets it; a fork per keystroke-cycle is the one cost a prompt may not have. Reading two
small files is a stat walk and one `read`.

**The dirty marker of the specification's example (`git:main*`) is not shown.** Deciding whether
a tree is dirty means walking it against the index, which is precisely the work §34 forbids here,
and a marker that is sometimes wrong is worse than no marker. §4.2's table is explicitly a list
of *suggested* segments, so this is a choice inside what the section allows rather than a
departure from it.

**`prompt.vcs` turns it off** (bool, default `true`), declared in the settings catalogue like
every other key (ADR-0094), because a segment shown on every line is exactly the kind of thing a
user must be able to remove.

The segment sits after the path and before the jobs count, painted `ui.dim`: it is context, not
the answer to anything.

## Consequences

`local://~ git:main > ` inside a checkout, and the v0.2 prompt unchanged outside one — so no
existing case or test sees anything new unless it runs inside a repository.

The branch can be one refresh stale in the sense that it is read at prompt time, which is exactly
when it matters. A repository whose `HEAD` cannot be read shows no segment rather than an error:
a prompt is not the place to report that something was unreadable.

Encoded by `crates/ono-cli/tests/prompt.rs::should_name_the_branch_in_the_prompt_when_the_working_directory_is_a_checkout`,
`::should_leave_the_branch_out_of_the_prompt_when_there_is_no_checkout` and
`docker/acceptance/cases/113-prompt-segments.case`.

## Alternatives considered

- **Running `git rev-parse --abbrev-ref HEAD`.** Correct in every corner case git has, and a
  process fork on every prompt. Rejected on §34.
- **Linking a git library.** A large dependency, and its value is in the operations a prompt does
  not perform.
- **Showing the dirty marker by walking the worktree.** Rejected above; if it is ever wanted, it
  belongs behind a setting that is off by default and a cache that watches the index.
- **Leaving the segment out**, since §4.2 marks it optional. Rejected: it is the one segment of
  the six that a person working in a repository reads on every line, and "optional" is about
  whether the shell may omit it, not about whether this shell should.
