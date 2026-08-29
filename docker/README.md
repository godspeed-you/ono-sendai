# Container verification

`scripts/acceptance.sh` builds `docker/Dockerfile` and runs every case in `acceptance/cases/`
against the `ono` binary inside the resulting image, as an unprivileged user whose login shell
is `ono`, with networking disabled (`docs/ACCEPTANCE.md` §2).

## Case format

A case is a flat key/value file so that it stays readable in a diff and needs no parser of its
own. A value is either the rest of the line, or a bare `|` followed by lines indented two
spaces — which is how a case carries a whole script without inventing quoting rules:

```
# a comment
case: what a user can do
run: |
  echo one
  echo two
exit: 0
stdout-contains: one
```

## Directives

| Directive | Meaning |
|---|---|
| `case:` | what the case proves, in the words of a user. Required. |
| `run:` | the script to execute, as a line or a `\|` block. Required. |
| `stdin:` | text fed to the script's standard input, as a line or a `\|` block. |
| `exit:` | expected exit status. Defaults to `0`. |
| `pty:` | `true` runs the script under a real controlling terminal (spec §29.3). |
| `columns:` / `lines:` | terminal size — as `COLUMNS`/`LINES`, and as the pty's window size when `pty: true`. Proves the 80-column and 200-column requirements of `docs/ACCEPTANCE.md` §4.2. |
| `env:` | a `NAME=value` pair for the container. Repeatable. |
| `capability:` | a Linux capability the case needs, as `--cap-add` names it (`NET_ADMIN`, `SYS_ADMIN`). Repeatable. Only for a case whose whole point is a privileged path; the default is no capability at all. |
| `user:` | the container user to run as. Only `root`, and only together with `capability:` — a capability is useless to the unprivileged `case` user. |
| `security:` | a `--security-opt` value the case needs, such as `apparmor=unconfined` for a case that mounts. Repeatable, and only for a privileged case: the host's AppArmor profile denies `mount(2)` even to `CAP_SYS_ADMIN`. |
| `timeout:` | seconds before the case is killed and failed. Defaults to `30`. |
| `stdout-contains:` | literal text that must appear. Repeatable. |
| `stdout-not-contains:` | literal text that must not appear. Repeatable. |
| `stdout-matches:` | extended regular expression that must match. Repeatable. |
| `stdout-not-matches:` | extended regular expression that must not match. Repeatable. |
| `stdout-equals:` | the entire output, exactly, with carriage returns stripped. Repeatable. |

Every assertion in a case must hold. All output checks see stdout and stderr combined, because
that is what a user sees.

`run` is executed with `bash -lc` inside the container. Under `pty: true` it runs through
`script(1)`, so the program under test has a genuine controlling terminal rather than a pipe —
the only way to prove that full-screen programs, job control and TTY-conditional rendering
actually work instead of assuming they do.

## Rules

Cases assert what a user observes — printed output, exit status, resulting system state — never
how the result was produced (AGENTS.md §11). A case that has to change when the implementation
is restructured is a defective case.

Add a case whenever a capability becomes advertised: if it is worth telling a user about, it is
worth proving in a clean machine. A capability without a passing case is not delivered
(`docs/ACCEPTANCE.md` §2).

`004-harness-self-test.case` exercises the harness itself — blocks, stdin, environment, terminal
size and a real TTY. It exists because a referee that has quietly stopped checking would let
every later case pass while proving nothing (AGENTS.md §14).

## Running

```bash
scripts/acceptance.sh                  # build the image, run every case
scripts/acceptance.sh --keep-image     # keep the image for the next run
scripts/acceptance.sh --no-build pty   # reuse the image, run cases whose name contains "pty"
```
