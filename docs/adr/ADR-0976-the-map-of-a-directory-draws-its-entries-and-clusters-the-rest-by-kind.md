# ADR-0976: The map of a directory draws its entries and clusters the rest by kind

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §3.4, §8.1, §8.2, §8.3, §15.4, §23.6, §34.2; ADR-0188; issue #216
- Decided by: agent (autonomous)

## Context

ADR-0188 gave a directory place its `children` and bounded the view of them, and left §8.2's
clustering of the entries — "grouping the entries by kind or by name instead of counting them" —
as the next increment. The map of a directory place drew the directory, its mount and nothing
that is in it: the entries are hierarchy (§3.4), the map's horizon was built from relationship
edges only, and a place with 241 entries mapped to four nodes.

## Decision

**The horizon of a directory place holds its entries**, one containment hop from the centre,
read from the listing `look` reads (ADR-0188). The projection draws as many as the node budget
allows — a directory ranks ahead of a file, as §8.1 puts a file at the detail level — and
**clusters the rest along §8.2's directory subtree, one cluster per kind of entry**:
`/etc (directories)` and `/etc (files)`, each reporting how many entries it stands for.

The cluster's `grouping` is `directory_subtree`. Its id is derived from the directory and the
kind, so it is the same in a later run and `map --expand <id>` reaches the same group (§8.3).

## Consequences

Easy: `enter /etc; map` shows where one can go from here and says "116 more in /etc
(directories)", "99 more in /etc (files)"; every entry is drawn or counted, none is dropped
(§23.6).

Hard: mapping a directory costs its listing — one `readdir` and a `stat` per entry, once per TTL
(ADR-0188) — where it used to cost nothing because nothing was read. The entries take most of
the text map's thirty nodes, so the directory's relationship neighbours are listed under "also
here" and no longer compete for drawn nodes.

Kinds are the two kinds of place the index has for a path, file and directory. A symlink, a
socket and a device node are files to the index and are counted with them.

Encoded by `crates/ono-spatial-query/tests/map.rs::should_cluster_the_entries_of_a_directory_by_kind_when_they_exceed_the_budget`,
`crates/ono-cli/tests/spatial_map.rs::should_draw_the_entries_of_a_directory_and_cluster_the_rest_by_kind`
and acceptance case `136-directory-map-clusters`.

## Alternatives considered

**Cluster by name** — a prefix or an extension. A directory's names follow no scheme the shell
could know; `/etc` would yield a cluster per letter or one of everything. The kind is a fact of
each entry.

**One cluster for the directory** — what the canonical-collection dimension gives without a
change. It says "215 more in /etc", which is the count ADR-0188 already shows in `look`.

**Keep entries out of the map** — the map of a directory then shows nothing §15.4 calls
navigation, which is what a map is for.
