# ADR-0975: A directory points out the entries that changed recently

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §3.7, §15.4, §24.3, §26.2, §26.3; v0.5 §13.5; ADR-0163, ADR-0188; issue #216
- Decided by: agent (autonomous)

## Context

§15.4 lists `changed recently` among the semantic neighbours a directory place may expose.
ADR-0188 left it out as "a snapshot difference (§25.4)", and v0.5 §13.5 has since ruled that the
shell keeps no snapshot comparison of its own beside the temporal engine. `near --changed` is a
filter over what the session *observed* recently, and at a directory just listed that is every
entry.

A file states when it last changed: `ono.file/1` carries `modified`. The landmark engine already
turns such a statement into `recently_changed` for a process (`started`), a service (`since`)
and a container (`created`), with `spatial.look.change_window` as the window.

## Decision

**A file or directory modified inside the change window is a `recently_changed` landmark**, with
`modified <instant>` as its evidence. The rule reads the object's own statement of its last
change; where a record states more than one — a file's birth and its last write — the latest is
the change.

At a directory place the entries that changed recently are therefore the landmarks `look`
prints, and `near` ranks them ahead of the entries that did not (a landmarked member ranks
first, §6.2). No relation is declared and no group is added: `changed recently` is a reason an
entry is worth looking at, which is what §3.7 calls a landmark.

`near --changed` keeps its meaning — observed inside the window (§6.2, §24.3).

## Consequences

Easy: `enter /etc/nginx; look` names the file somebody edited two minutes ago; the window is
the one setting `look`'s change section uses, so the two cannot disagree.

Hard: a build directory or `/tmp` under load has many recent entries, and each is a landmark
until the landmark budget of the view cuts the list. `recently_changed` is a `notice`, so it
never reorders a map (§26.3). A file whose content changed with its `mtime` preserved — `cp -p`,
a restore — is not pointed out; the ledger's `object.changed` is the evidence for that case.

Encoded by `crates/ono-spatial-query/tests/landmarks.rs::should_promote_a_file_that_was_modified_inside_the_change_window`,
`crates/ono-cli/tests/spatial_storage.rs::should_point_out_the_entries_of_a_directory_that_changed_recently`
and acceptance case `135-directory-changed-recently`.

## Alternatives considered

**A `changed` exit group at a directory place.** Exits are relations or hierarchy (§3.4, §11),
and "changed recently" is neither: it is a property of some of the children. A group would list
the same places `children` lists, under a second word.

**Make `near --changed` read `modified`.** One flag would then mean "observed recently" for a
process and "modified recently" for a file, and §24.3 ties it to what was observed.

**Compare two listings of the directory.** The snapshot comparison v0.5 §13.5 retired; the
temporal ledger already answers it where evidence exists.
