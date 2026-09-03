# ADR-0564: Ctrl-Up and Ctrl-Down walk the history without an anchor

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.2 §20, §34; v0.5 §29.4; v0.7 §21.1, §21.3; ADR-0005
- Decided by: agent (autonomous)

## Context

`Ctrl-Up` and `Ctrl-Down` reach the editor and are bound to nothing, so they vanish (issue
#122). The bare arrows are bound, and what they do is not what their name suggests: `Up` builds
an anchor from the text before the cursor and walks to the previous entry that *starts with* it
(`Editor::history_previous`). That is readline's `history-search-backward`, and it is the right
default — a user who has typed `get ` and presses `Up` wants the last `get`, not the last
command. But it leaves the other walk unreachable: there is no key that steps to the previous
entry whatever has been typed, so an entry that does not start with the typed text can only be
reached by clearing the line or by `Ctrl-R`.

The issue names the open decision: `Ctrl-Up` could be the unanchored walk, or it could be a
second key for the anchored one, matching a personal inputrc where `Ctrl-Up` is the prefix
search. The specification does not settle it. v0.2 §20 is about what a history entry records,
v0.5 §29.4 only insists that command recall stays command recall, and v0.7 §21.1 says the existing
history navigation remains authoritative and §21.3 that later bindings integrate with the one
keymap. No later enhancement reserves either key.

## Decision

**`Ctrl-Up` and `Ctrl-Down` are the unanchored walk.** Two actions beside the existing pair —
`HistoryPreviousUnanchored` and `HistoryNextUnanchored` — step to the previous and next entry
whatever is typed. The bare arrows and `Ctrl-P`/`Ctrl-N` keep the anchored walk.

Both walks share one `HistoryNav`: the line being typed is saved once, when a walk starts, and
comes back when either walk steps past the newest entry, exactly as the anchored walk does today.
The anchor is taken from the text before the cursor when the walk starts and is *applied* only by
the anchored steps, so the two kinds of step can be mixed inside one walk: `Ctrl-Up` to an older
entry, then `Up` to the previous one that starts with what was typed.

## Consequences

- Both movements are reachable, and each has one key. Giving `Ctrl-Up` to the anchored walk
  would have given one movement two keys and left the other with none.
- A user whose inputrc makes `Ctrl-Up` the prefix search gets the other walk on that key here.
  The keymap is configurable (`Keymap::bind`), so that preference is a binding away.
- `Ctrl-R` is untouched; v0.5 §29.4 keeps it as command recall, and this ADR adds nothing to it.

## Alternatives considered

- **`Ctrl-Up` as a second key for the anchored walk**, the inputrc reading. Rejected: it makes
  the unanchored walk unreachable, which is the defect.
- **Swap the arrows: bare `Up` unanchored, `Ctrl-Up` anchored**, as bash does by default.
  Rejected: v0.7 §21.1 makes the existing history navigation authoritative, and the anchored
  default is the better one at a shell whose lines start with a small vocabulary of verbs.
- **Make the anchored walk fall through to the unanchored one when nothing matches.** Rejected:
  a key that does two different things depending on the history's contents is a key nobody can
  predict.
