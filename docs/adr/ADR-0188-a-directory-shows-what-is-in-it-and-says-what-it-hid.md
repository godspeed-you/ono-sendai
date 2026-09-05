# ADR-0188: A directory shows what is in it, and says what it hid

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §15.4, §15.1, §3.4, §3.6, §33.3, §34.2, §2.17
- Decided by: agent (autonomous, S4d/S4e)

## Context

§15.4: "A directory place MUST support normal path navigation and MAY also expose semantic
neighbors", listing children first; and then the rule that makes it interesting — "The spatial
renderer MUST NOT enumerate huge directories by default. It SHOULD cluster or summarize when
entry counts exceed the view budget."

Until this increment a directory place had no children at all. Its exits were `mount`
(`unsupported`), `openers` (`unsupported`) and `owner` (`unknown`), and standing in a directory
told you nothing about what was in it. The 400-entry test passed for the worst possible reason:
nothing was listed because nothing was ever read.

Two questions had to be answered. Where do children come from — a relationship edge, or the
hierarchy? And what is bounded — the read, or the view?

## Decision

**Children are hierarchy, and they come out of the path tree the index already holds.**
§3.4 lists "Directory -> child Directory" among the *hierarchical* edges, not among the
relationship graph, so no relation is declared for them and `relations.yaml` is unchanged.
`SpatialIndex::path_children` is the reverse of `set_path_parent`, and
`ono_spatial_query::neighborhood_of` puts a `children` group first for any place that has them.
Only entries this session actually observed appear: §33.3 makes the filesystem query-driven, and
the index never invents a child nobody read.

**The read is whole; the view is bounded.** `storage::observe_children` asks `get dir <path>` for
the entire listing and files every entry under the place. The count §15.4 asks to be summarised
is a statement about how many entries there *are*, and a count taken from a truncated read would
be a number from nowhere (§2.17). What bounds the answer is the neighbourhood budget that already
bounds every other group (§34.2): eight members shown, the total kept, and the difference
disclosed as `hidden_count` — rendered as "392 more not shown".

**The listing is remembered like any other provider answer.** The observation cache of ADR-0186
is keyed by the query rather than by the target, so `dir` asked about `/etc` is not `dir` asked
about `/var`, and standing in a directory and looking twice reads it once.

**A refusal is a refusal.** A directory the user may not read records `children` as withheld with
its §35.2 state and the provider's own message, never as an empty listing (§42.4).

## Consequences

- `enter /etc; look` shows `children 241`; a 400-entry directory shows eight and says
  "392 more not shown". Both are §15.4.
- `up` from any entry reaches the directory it was listed in, because the listing sets the path
  parent of every child — the same field `up` from a file consults (ADR-0187).
- Entering a very large directory costs one `readdir` plus a `stat` per entry, once per TTL. That
  is the price of §15.4's summary being a real count. §8.2's clustering — grouping the entries by
  kind or by name instead of counting them — is a further increment on top of this one, and the
  field it would fill (`ono.map-cluster/1`'s dimension) already exists.
- §15.4's other optional neighbours are *not* delivered here: `open-by processes` needs an
  `lsof`-shaped provider, `owned-by users` is an expensive relation nobody has asked to load,
  and `changed recently` is a snapshot difference (§25.4). Each says what it is rather than
  showing zero.
- Exit test: `spatial_storage_missing::should_summarize_a_large_directory_instead_of_enumerating_it`,
  which now passes because a bound was applied rather than because nothing was read.

## Alternatives considered

- **Declare a `directory.contains_entry` relation.** It would make `follow child` a sentence, and
  it would put in the relationship graph something §3.4 explicitly calls hierarchy — the same
  mistake as filing a process's `children` under both.
- **Read only the first N entries.** Cheaper, and it makes the summary a lie: "8 entries, 0
  hidden" for a directory of four hundred.
