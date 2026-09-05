# ADR-0177: Completion is a map, and a picker shows the difference

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §9.1, §9.4, §11.2, §27.1, §27.2, §29.3, §34, §35.2, §39.1
- Decided by: agent S6 (autonomous)

## Context

§50 assigns neither §9.4's completion nor §27.2's picker to a phase. Both are interactive, so
they are S6's.

§9.4: at `enter <TAB>` completion "MUST prioritize services visible in the current neighborhood
and then offer broader matches"; at `follow <TAB>` it "MUST show actual available relation types";
an entry "MAY show a compact count or state". Its closing sentence is the design: "Completion is
therefore not merely token completion; it is a lightweight local map."

§27.2: "Interactive ambiguity opens a picker. The picker MUST show disambiguating context", with
an example whose first column is `nginx/1842` — a name and the key that separates two places a
person calls the same thing. §29.3 forbids a script from ever seeing one.

## Decision

**Completion after a spatial verb is a listing, shown at once.** `ono_editor::Completion` gained a
`listing` field: lines to display as soon as the candidates are offered, rather than on a second
Tab. Ordinary word completion is untouched — it still earns its listing with a second Tab, and the
existing editor test that says so still passes unchanged — because a discovery listing is what the
user asked for, not a hint that more typing would resolve it. The word is still extended as far as
the candidates agree, so one Tab does both jobs.

**What is offered is what the session can see, and nothing is asked of a provider.** §34 budgets
50 ms for the first results from local metadata; an offer that blocked on `/proc` would be neither
local nor metadata. So `crate::spatial::complete` reads two sources only: the declared canonical
geography of the current place, which costs nothing and is true before anybody has looked, and the
spatial index, which holds what this session has already observed. A neighbourhood nobody has
looked at is offered as its declared geography and no more — which is honest, because that is
exactly what the shell knows.

- `enter`, `jump` and `map` offer the places: a canonical space's served children (written the way
  §5's horizon writes them, lower-case), then the objects the index holds under this place; an
  observed object offers the places its edges reach, with their kind beside them.
- `follow` offers the relation groups the index summarises for this place, each with its member
  count — or, where the group could not be read, §35.2's state word instead of a number, because a
  count of zero would claim the relation is empty.

**The picker is the resolution, drawn.** `resolved_place` already produces
`Resolution::Ambiguous(candidates)`; at a terminal it now draws them and returns the one chosen,
and everywhere else it raises `spatial.ambiguous_selector` exactly as before (§29.3). Dismissing
the picker with Esc raises the same refusal, because a picker that was dismissed answered nothing.

**A candidate carries its key.** `Candidate` gained `key` — the first identity field of the
object's canonical reference, which is the pid of a process, the target of a mount, the inode of a
socket (§11.2). `Candidate::row` writes `<name>/<key>` when the key is not the name itself, so
§27.2's three columns disambiguate: two processes both called `nginx` are `nginx/1842` and
`nginx/1902`. The same rows are what the non-interactive refusal prints, so a script and a person
are told the same thing.

The picker's cursor is a `>` in the left margin (§39.1: the focused item may not need colour), it
moves with Up/Down, Tab/Shift-Tab and `k`/`j`, Enter takes the row, and Esc and Ctrl-C leave it.

## Consequences

- `enter <TAB>` at the root teaches the six domains without documentation, which is §9.4's intent
  and §5's.
- A relation with no observations behind it is not offered — `follow <TAB>` lists what the place
  *has*, which is what §9.4 asks for and what a vocabulary dump would not be.
- The ambiguity error message improved everywhere, not only interactively; the contracts suite's
  requirement that the refusal show its candidates is unchanged and still passes.
- Completion holds the spatial state with `try_lock` and offers nothing when a command holds it.
  At a prompt no command is running, so the case does not arise; it degrades to ordinary
  completion rather than blocking, which is the right failure for a keystroke.

## Alternatives considered

- **Listing every completion on the first Tab.** Rejected: it would have required editing
  `crates/ono-editor/tests/completion.rs`, whose assertion "the first Tab does not list" is a
  contract this increment has no reason to change.
- **Observing providers during completion.** Rejected by §34's budget: Tab must answer instantly.
  The result is that completion after a `cd`-like jump into an unobserved collection offers only
  the geography — a real limit, written here rather than hidden.
- **A full-screen picker.** Rejected: §27.2's example is four lines under the prompt, and taking
  the whole screen to choose between two processes is the dashboard §49.8 warns against.
