# ADR-0124: Spatial verbs that collide with Unix programs

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §6 (spatial command language), §6.1, §6.8, §44.10; v0.2 §6.5 (resolution
  order), §8.4 (target design rule); v0.3 §1.71 (Unix muscle memory)
- Decided by: agent (autonomous); confirmed by the user 2026-08-28

## Context

v0.4 §6 makes eleven words normative spatial verbs: `look`, `near`, `enter`, `follow`, `jump`,
`back`, `up`, `home`, `trail`, `find`, `map` (plus `pin`/`unpin` in §20.4). Two of them name
programs that exist on a normal Linux system:

| Word | Program | How much it matters |
|---|---|---|
| `find` | findutils, on every system, in everyone's fingers and in scripts | enormous |
| `look` | util-linux dictionary lookup | marginal |

The rest name nothing on this host or in the acceptance image.

`find` is not merely on `PATH`: v0.3 ships the adapter `org.ono.compat.findutils.find`, so
`find /var/log -name '*.log'` already answers as typed `ono.file/1` records, and
`docker/acceptance/cases/087-adapters-unix-muscle-memory.case` pins
`find /etc/default -maxdepth 1 | wc -l` producing exactly the bytes bash produces. Giving the
bare word `find` to the spatial verb would break that case, the muscle-memory guarantee v0.3
§1.71 exists to make, and every script a user already has.

The v0.2 registry already answers this shape of question: `find` is a **verb**, and the native
commands are `find file`, `find command`, `find plugin`, `find package` — verb plus target. A
bare `find .` reaches the program; `find file /var/log` reaches the shell. That rule is
implemented, tested and inspectable through `explain`.

## Decision

The spatial verbs are ordinary native commands and resolve by v0.2 §6.5 — language keyword,
user function or alias, native command, `PATH` executable — with one rule for collisions:

1. **A spatial verb whose bare spelling names a widely used Unix program keeps the target word.**
   Today that is exactly `find`: the spatial search is spelled **`find place`**, with `place`
   as its target (v0.4 §3.3 defines `Place`), alongside the existing `find file`,
   `find command`, `find plugin`, `find package`. Bare `find` keeps reaching findutils through
   the v0.3 adapter, unchanged.
2. **Every other spatial verb takes the bare name**, because no program of consequence answers
   to it: `look`, `near`, `enter`, `follow`, `jump`, `back`, `up`, `home`, `trail`, `map`,
   `pin`, `unpin`.
3. **`look` shadows util-linux `look`.** It is a dictionary lookup that no case in this
   repository and no script we ship uses. It stays reachable as `exec:look` and by absolute
   path, and `help look` names the shadowing explicitly, as does the diagnostic when `look` is
   given an argument that is a plain word rather than a place or an option.
4. The rule for the future is the criterion, not the list: a new spatial verb takes the bare
   name unless a program of comparable reach already answers to it, in which case it takes its
   target word. "Of comparable reach" means: in coreutils, util-linux, findutils or procps, or
   present in an acceptance case.

## Consequences

- Case 087 and the v0.3 muscle-memory guarantee stay true without a special case in the
  resolver: the existing verb+target rule does the work.
- `find place --where state == "running"` is one word longer than v0.4 §6.8 writes it. That is
  the price of not breaking `find`, and it is the spelling v0.2 users already know from
  `find file`.
- The RED suites assumed the bare spelling. The increment that delivers §6.8 rewrites those
  assertions to `find place` in the same commit as the contract (AGENTS.md §7). The complete
  site list, from the test-to-phase analysis of 2026-08-28, is `spatial_navigation_missing.rs`,
  `spatial_topology_missing.rs` (six sites spelling `find process --where …`),
  `spatial_relationships_missing.rs`, `spatial_storage_missing.rs`, and the acceptance
  scenarios **090, 091, 092, 094, 095, 096 and 097** — the first list named only three of the
  seven cases and is corrected here. Two tests
  (`spatial_navigation_missing.rs::should_run_the_native_spatial_find_and_keep_the_external_find_reachable_when_both_exist`
  and its topology counterpart) assert the *opposite* rule in their comments and messages:
  they must be restated to this ADR's rule, not merely re-spelled, and that restatement is the
  test edit AGENTS.md §7 permits when the contract decides against the assumption a RED test
  made.
- **The spatial type is an option, not a second target word.** §9.3's `find service` and the
  suites' `find process --where …` become `find place --type service` / `--type process`,
  matching `near --type <type>` in §6.2. One verb, one target, the type as an option: adding
  `find <type>` as a second spelling would put the target slot back in play for every schema
  name and re-open exactly the collision this ADR closes.
- `look` becomes unavailable as a bare word for its util-linux meaning. Anyone who wants the
  dictionary tool writes `exec:look` — the same escape hatch every shadowed name has.
- `explain look` and `explain find place` must both show which step of §6.5 matched, so the
  shadowing is inspectable rather than folklore.

## Alternatives considered

- **The spatial verb wins `find` and the program moves to `exec:find`.** Rejected: it breaks a
  green acceptance case, contradicts v0.3 §1.71, and would make the shell hostile to exactly
  the user v0.4 §52.3 describes — one who arrives with Unix fingers.
- **Resolve by argument shape** (a path-looking argument means the program, an option like
  `--where` means the spatial verb). Rejected: v0.2 §6.5 requires resolution to be explicit and
  inspectable; guessing from arguments is the ambiguity that rule exists to forbid.
- **A namespace prefix for every spatial verb** (`spatial:find`). Rejected: it makes the
  primary interface of v0.4 second-class in its own shell, and §6 spells the verbs bare.
- **Rename the spatial search to a new word** (`search`, `locate`). Rejected: `locate` is also a
  program, and inventing a synonym for a verb the registry already has is exactly the target
  design rule of v0.2 §8.4 warning against verb proliferation.
