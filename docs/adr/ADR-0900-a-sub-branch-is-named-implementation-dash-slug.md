# ADR-0900: A sub-branch is named `implementation-<slug>`

- Status: accepted; superseded by ADR-0918 (in part: a sub-branch run reads `main`'s caches, not `implementation`'s)
- Date: 2026-09-24
- Spec refs: none — process decision; amends one clause of ADR-0004; AGENTS.md §12.1; issue #196
- Decided by: agent (autonomous)

## Context

AGENTS.md §12.1 allowed parallel agents sub-branches named `implementation/<crate>`, and ADR-0004
said the same. Git cannot create one. A branch is a file under `refs/heads/`, and
`refs/heads/implementation` is already a file, so it cannot also be the directory
`refs/heads/implementation/` that `implementation/<crate>` needs:

```text
$ git worktree add -b implementation/h7-spatial-performance ../h7 implementation
Preparing worktree (new branch 'implementation/h7-spatial-performance')
fatal: cannot lock ref 'refs/heads/implementation/h7-spatial-performance': 'refs/heads/implementation' exists; cannot create 'refs/heads/implementation/h7-spatial-performance'
```

Every parallel run since 2026-09-02 used `implementation-<phase>` instead. So the document named a
form nobody could use, and `.github/workflows/ci.yml` triggered on `implementation/**`, a pattern
no branch in this repository can match. The form that was actually used got no CI on push.

## Decision

**A sub-branch for a parallel agent is named `implementation-<slug>`. It is merged back into
`implementation` and never into `main`, and CI runs on a push to it.**

- AGENTS.md §12.1 names `implementation-<slug>` and says why the slash form does not work.
- `.github/workflows/ci.yml` triggers on `implementation-*` in place of `implementation/**`. The
  `*` of a GitHub branch filter stops at `/`, which is fine, because the slug has none.
- Only `main` and `implementation` save the gate's Actions caches (`save-if`), so a sub-branch
  run reads their caches and adds nothing to the 10 GB budget. That was already so for pull
  requests.
- ADR-0004 is otherwise unchanged. This ADR replaces its sentence "Parallel agents may use
  `implementation/<crate>` sub-branches" and nothing else.

## Consequences

- The documented convention can be carried out as written, and it is the one past runs used.
- Pushing a sub-branch starts a full CI run. `concurrency: ci-${{ github.ref }}` cancels an
  earlier run of the same branch, so repeated pushes cost one run each.
- `implementation-2`, ADR-0004's example of a second attempt kept beside the first, matches the
  pattern and gets CI too. That is intended: it is implementation work.

## Alternatives considered

- **Rename the trunk (e.g. to `implementation/main`) so the slash form works.** That touches
  §12.1, the gate's `main` guard, CI, every ADR that names the branch, and the remote branch
  itself. Renaming or recreating `implementation` is the user's decision (AGENTS.md §12.1), and
  the benefit is only a spelling.
- **Keep the slash form in the document and explain the workaround.** A rule whose written form
  fails when you follow it is the defect #196 reports.
- **Leave CI off for sub-branches.** Then a sub-branch is proven only by the local gate until it
  is merged, and the first CI verdict comes after the merge. The runs cost no cache budget, so
  there is no reason to wait for that.
