# Container verification

`scripts/acceptance.sh` builds `docker/Dockerfile` and runs every case in
`acceptance/cases/` against the `ono` binary inside the resulting image.

A case is a flat file so that it stays readable in a diff and needs no parser:

```
# a comment
case: what a user can do
run: ono --version
exit: 0
stdout-matches: ^ono [0-9]+\.[0-9]+\.[0-9]+$
stdout-contains: some literal text
```

`run` is executed with `bash -lc` inside the container with networking disabled. `exit` defaults
to `0`. `stdout-matches` is an extended regular expression; both output checks see stdout and
stderr combined, because that is what a user sees.

Cases assert what a user observes, never how it was produced (AGENTS.md section 11). Add a case
whenever a capability becomes advertised - if it is worth telling a user about, it is worth
proving in a clean machine.
