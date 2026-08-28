# v0.4 acceptance scenarios (not yet run by the referee)

The files named `*.case.v04` in this directory are the ten acceptance scenarios of
`docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` §44, written as containerised
end-to-end cases. **They are RED**: v0.4 is not implemented, so every one of them fails today.

`scripts/acceptance.sh` collects `*.case` (`find … -name '*.case'`), so the `.v04` suffix keeps
them out of the suite and the referee stays green while the scenarios sit in the repository
waiting for the code. **The increment that delivers a scenario renames its file to `.case`** —
that rename is the acceptance step of the increment, and it must happen in the same commit as
the behaviour, exactly as `docs/ACCEPTANCE.md` §2 requires ("a capability without a passing
acceptance case is not delivered").

| File | Scenario | Also proves |
|---|---|---|
| `090-spatial-cold-start-discovery` | §44.1 cold-start discovery | §5 entry horizon, §7 canonical domains, §22 map contract, §23.2 text map, §29.1 non-interactive surface, §34 budgets |
| `091-spatial-unknown-web-service` | §44.2 unknown web service | §12, §13, §14.3, §28 pipeline↔space, §29.3 no picker in a script, §35.2 unavailable ≠ empty, §37.1 adapter identity merge |
| `092-spatial-storage-discovery` | §44.3 storage discovery | §15 storage spaces, §15.3 mount boundary, §15.4 view budget, §30 `cd` versus `enter` |
| `093-spatial-process-file-process` | §44.4 process → file → process | §11.2 graph, §11.4 inspectable relations, §11.5 confidence, §35.2 empty ≠ unknown |
| `094-spatial-network-path` | §44.5 network path | §14 network spaces, §19 remote systems as space, §35.4 no silent dialling, §43.7 no local/remote identity merge |
| `095-spatial-back-versus-up` | §44.6 back versus up | §6.6, §11.3 canonical parent, §20.1 trail schema, §40 `history_empty` / `no_parent` |
| `096-spatial-identity-replacement` | §44.7 identity replacement | §10 identity tiers and tombstones, §20.3 dead destinations |
| `097-spatial-permission-honesty` | §44.8 permission honesty | §35.1–§35.3, §27.4 freshness, §40 `permission_denied` |
| `098-spatial-live-map` | §44.9 live map | §25 live state and animation policy, §29.4 streaming, §43.4 Ctrl-C, §43.6 real change only |
| `099-spatial-raw-shell-continuity` | §44.10 raw shell continuity | §23.3 full-screen controls, §23.4 focus ≠ place, §43.4 PTY checks, §49.8 |

## Cases already in the suite

`101-spatial-find-place.case` is not one of the §44 scenarios. It is the part of them the
`find place` increment delivers on its own — §50 Phase S3's gate, "objects can be discovered
without prior exact names" — and it runs with the rest of the suite. The ten scenarios above stay
`.case.v04` because each walks through navigation verbs that do not exist yet.

## What the container can and cannot provide

Each case says so in its own header. In short: the image runs no systemd, no journald and no
container runtime, has no network beyond loopback, and every process in it belongs to one
unprivileged user. The fixtures under `../fixtures/spatial/` supply what can be supplied for
real — a listening process with workers that holds a known file open, a client that holds a
connection, and a `systemctl` stand-in whose main pid is that real process. Where a scenario
needs something the container genuinely cannot have, the case asserts the honest degradation
v0.4 §35.2 requires (`unknown` / `permission_denied` / `unsupported` are distinct from empty)
rather than skipping.

## House rules these cases follow

* Every assertion is a named claim: the run script prints `PROVED <id>: <what>` only when its
  check succeeded, and `FAILED <id>` with the first lines of output when it did not. Each case
  asserts every `PROVED` line it expects and `stdout-not-contains: FAILED`.
* Only the output of the statement under test is asserted on. `enter`, `find` and `link` print
  too, so the cases run their spatial script through a `place` helper that emits a marker and
  keeps only what came after it — an assertion that matched a rendered intermediate result
  would prove nothing about the place actually reached.
* Keystrokes that are not text (Tab, Enter, Esc, Ctrl-C) are sent by running `ono` under a
  nested `script(1)` inside the case, which gives it a real controlling terminal while the case
  generates the bytes.
* No case types the name of the object it is supposed to discover. Names appear in assertions,
  never as input, because §44 is about finding things without knowing their names.
