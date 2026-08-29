# ADR-0333: The pathological environments of spec §34 are built, not assumed

- Status: accepted
- Date: 2026-08-29
- Spec refs: §34 (performance requirements and its pathological environments), §34.1 (startup
  avoids eager discovery), §11.1 (streaming), §12.5 (POSIX pipes and stderr), §16.5 (partial
  failure), §18.3 (rendering live values); `docs/ACCEPTANCE.md` §4.3; ADR-0015
- Decided by: agent (autonomous)

## Context

Spec §34 closes with a sentence the checklist quoted and the suite did not honour: "Performance
tests SHOULD include pathological environments: tens of thousands of processes/paths, slow NSS,
high-latency links, huge stdout and unbounded streams." `docs/ACCEPTANCE.md` §4.3 said its
figures were measured "on the pathological fixtures of spec section 34", and of the five
environments exactly one existed — twenty thousand paths, with two hundred extra processes, in
case `100`. Case `060` measures an ordinary container. A budget met on an idle host says nothing
about the host a shell is reached for.

## Decision

**Four acceptance cases build the four missing environments, one environment each**, so a failure
names the environment that produced it:

| Case | Environment | Built by |
|---|---|---|
| `151` | tens of thousands of processes | 10 000 forked children blocked on a pipe |
| `152` | thousands of sockets and connections | 5 000 listening unix sockets over 13 processes |
| `153` | a deep and wide filesystem | 50 000 entries in one directory, 200 levels of nesting, 100 000 files over 1 000 directories |
| `154` | a stalled provider, huge stdout, an unbounded stream | a `sleep`-forever tool on `PATH` under the names the adapter layer runs, 100 MB through the shell, `watch process` |

**Every case measures the §34 budgets, prints the figure whether it passes or fails, and asserts
the spec's number** — the same shape case `060` and case `100` already use, so two runs can be
diffed. The three figures §34 names that a whole `ono` run can carry are measured in each
environment: cold start (100 ms), first rows of a provider (50 ms), and a parse-only pipeline.
Figures §34 does not name — a whole process table, a whole 50 000-entry listing, a `find` across
100 000 files — are measured and printed too, and asserted only against a generous ceiling, so a
regression is visible without inventing a budget the specification does not set.

**The fixtures are forks, not execs.** Ten thousand `sleep` processes cost the container ten
thousand images; ten thousand forks of one Perl interpreter share their parent's pages until they
write, and `/proc` cannot tell the difference — which is the only thing the process provider
looks at. The same reasoning puts the sockets in a handful of children: the descriptor limit is
per process.

**"Slow NSS" and "high-latency links" are built as a provider that never answers at all**, which
is the strictly harder case and the one the container can build honestly with networking
disabled. What case `154` asserts is everything that must *not* wait for it — start-up, parsing,
and every provider that answers from the kernel — and that a statement which does need it ends
when it is asked to rather than having to be killed.

**A case reports the size it actually reached**, not the size it asked for. A container that
cannot fork ten thousand children says so and the budgets are asserted against the number it did
reach, with a floor below which the environment is not pathological any more and the case fails.
A performance case that silently measured an idle host would be worse than no case at all.

## Consequences

`docs/ACCEPTANCE.md` §4.3's claim that the budgets are measured on §34's pathological fixtures is
true from this commit, and it names the case that builds each one.

Case `151` and `153` are slow by construction — building the fixtures dominates their runtime —
so their timeouts are 600 s and 900 s. They are ordinary cases and run in every acceptance run;
splitting the suite into fast and slow halves would create a half nobody runs.

`docker/acceptance/fixtures/perf/` holds the three fixtures. They exit with the container, and
each blocks on a pipe held by its parent so that killing the parent takes the children with it.

## Spec deviation

- Section: spec §34
- Text: "Performance tests SHOULD include pathological environments: tens of thousands of
  processes/paths, slow NSS, high-latency links, huge stdout and unbounded streams."
- Instead: slow NSS and high-latency links are built as one environment — an external tool on
  `PATH` that never answers — rather than as two, and the socket environment reaches thousands
  rather than tens of thousands.
- Why: the acceptance container runs with networking disabled (`--network=none`), so a
  high-latency link cannot be built inside it, and a slow name service and a slow link are the
  same thing from the shell's side: a provider that has not answered yet. Building the stall as
  *never* answering is stricter than building it as slow, so nothing is lost by having one
  environment instead of two. The socket count is bounded by the per-process descriptor limit
  times a number of helper processes that does not itself distort the process table; five
  thousand listening sockets is two orders of magnitude past an ordinary host and is what the
  container can hold without the fixture becoming the thing being measured.

## Alternatives considered

- **One case for all four environments.** Rejected: a 40-minute case whose failure says only
  "OVER BUDGET" names nothing, and the fixtures interfere — 10 000 processes make the socket
  figures meaningless.
- **A synthetic `/proc` fixture** instead of real processes. Rejected: the provider reads the real
  `/proc`, and a fixture that replaced it would measure the fixture.
- **Skipping the environment when the container cannot build it.** Rejected: a performance case
  that passes by not measuring is the failure mode `docs/ACCEPTANCE.md` §3 exists to prevent.
