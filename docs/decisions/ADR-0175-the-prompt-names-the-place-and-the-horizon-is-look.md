# ADR-0175: The prompt names the place, and the startup horizon is `look`

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §5, §21.1, §21.2, §21.3, §24.1, §29.1, §49.5, §53; v0.2 §4.1, §4.2
- Decided by: agent S6 (autonomous)

## Context

§50 assigns the entry experience of §5 and the prompt semantics of §21 to no phase at all. Both
are normative and both are what a person meets first, so S6 takes them.

§5 requires an interactive start to "establish place and nearby possibilities without requiring an
explicit discovery command", and lists what the horizon must carry: host identity, the canonical
domains, compact counts, a bounded set of landmarks, and a prompt showing the spatial scope. §53
makes it the default interactively. §29.1 is the other half of the same rule: nothing spatial may
be printed where there is no terminal.

§21.1 lists the prompt's semantic components — link/host, the current place, privilege, context
warnings — and §21.2 forbids rendering the trail into it: "Ono MUST NOT blindly render the entire
navigation trail in the prompt", showing `<host>/<current-place-kind>/<display-name>` instead.

v0.2 §4.2 already put the working directory in the prompt, and §30 keeps cwd and place separate
state. Both belong there.

## Decision

**The horizon is `look`.** `repl::run` runs the source `look` once, at a terminal, before the
first prompt, and prints it through the ordinary renderer. It is not a second renderer with its
own idea of what the root looks like, which §49.5 would make a defect; it is the same place view
`look --json` serialises. `spatial.startup_horizon = false` and `spatial.enabled = false` each
switch it off, and it is drawn only when standard input is a terminal, so a script's streams carry
nothing it did not ask for.

**One spelling of a place, in three views.** `ono_spatial_query::resolve::concise_path(index, id)`
is §21.2's rule as a function: a canonical space is its own path (`local`, `local/compute`); an
observed object is `<link>/<kind>/<display-name>` (`local/process/nginx`), because its
`place_path` names the parent chain, which answers §27.2's question and not this one. The prompt,
the place view's heading and the full-screen map's header all read it, so all three say the same
thing about where the session is.

**The prompt keeps the working directory.** The place is added to the link segment, never
replacing the path: `local/compute://~/work >`. §21.1 asks for link and place; v0.2 §4.2 asks for
the directory; §30 makes them different state, so showing one and hiding the other would be a lie
about a session that has `cd`-ed and `enter`-ed to different places. At the root the place adds
nothing to the link and the prompt is exactly what v0.2 printed.

**The privilege marker stays where v0.2 put it.** §21.3 requires privilege, remote and namespace
changes to be recognisable "even in minimal colorless terminals"; the ` root` segment and the `#`
marker already are, and they are not colour.

## Consequences

- The horizon costs one `look` — about 0.17 s in a debug build on a 400-process host — and §5's
  "MUST NOT block startup on expensive global scans" is met by `look`'s own bound, not by a
  second budget. When a provider is slow the horizon is as slow as `look`; making the counts
  asynchronous, which §5 only recommends, is a later increment and belongs with S7's live view.
- The place view's heading gained a third column, `SYSTEM / web01   local`, which is what makes
  `look` carry §5's "current host/context identity" as well as the place.
- `docs/spec/schemas/place-view.v1.yaml` did not change: the concise path is computed from the
  `place` record the view already carries.
- The exit status of the horizon does not become the session's: it is snapshotted and restored.

## Alternatives considered

- **A dedicated horizon renderer.** Rejected by §49.5: two renderers of the root would eventually
  disagree, and the one nobody types would be the one that rots.
- **Replacing the cwd in the prompt with the place.** Rejected: §30 keeps them separate, and
  `cd deeper; enter compute` must not hide the directory a program will run in.
- **Putting the concise path in the `ono.place-view/1` contract.** Rejected: it is presentation of
  fields the record already carries, and §22's rule against layout in the semantic contract points
  the same way.
