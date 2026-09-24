# ADR-0915: An artifact is examined and read through one descriptor, on a walk that follows no link

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.2 §31.10, §31.36; ADR-0870 §4, §7; ADR-0914
- Issues: #126
- Decided by: agent (autonomous)

## Context

ADR-0870 §4 made the safety argument for mapping a compiled artifact depend on a check: the
artifact and its store belong to this user or root, and nobody else may write them. An
independent review of #126 found three ways around that check as it was implemented:

- It examined the artifact and the store **by path** with `stat`, which follows links, and
  `Component::deserialize_file` then **opened the path again**. Between the check and the map,
  anybody who could change a directory on that path could put another file under the name.
- It examined **only the artifact and its store**. The store is
  `$XDG_CACHE_HOME/ono/kuang/compiled`. Whoever may write `$XDG_CACHE_HOME/ono`, or any directory
  above it, may rename `kuang` away and put in a tree of their own that is owned and moded
  correctly, and then choose the ELF the shell runs as native code. The same applies to root
  running `ono` with a user's environment (`sudo -E`): the user's cache is the user's to rewrite.
- `kuang-compile` created the store with `DirBuilder::recursive`, which follows a planted link
  (`compiled -> /somewhere`), and checked only the final directory. A root-run install could
  therefore write through a link, or leave root-owned directories in a user's cache.

## Decision

**1. The walk.** A store is reached from `/` one directory at a time. Each directory is opened
with `openat(parent, name, O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)` relative to the descriptor of
the one before it, which is the one that was examined. Each is then examined with `fstat` on its
own descriptor. A link anywhere on the walk refuses the store as `untrusted`: `ELOOP`, or
`ENOTDIR` on a name that `statat(…, AT_SYMLINK_NOFOLLOW)` shows to be a link. A directory this
user may search but not read is opened `O_PATH`, which is all the walk needs of it.

The directories above the store are the operator's layout, and links there are legitimate
(`/home -> /var/home`). So the deepest existing directory above the store is resolved with
`realpath` first, and the walk then examines the directories it resolves to. A link swapped in
after that resolution is met by the walk, which refuses it. The store itself is the tool's own
directory and is never followed.

**2. The rule, per directory.** It belongs to root or to the effective uid, and nobody else may
write it (`mode & 0o022 == 0`), as in ADR-0870 §4. That already made root refuse a user's tree:
under `sudo -E`, `/home/<user>` belongs to neither root nor the effective uid. Directories
*above* the store have two exemptions, the ones OpenSSH's `StrictModes` and its Debian
refinement make:

- a **root-owned sticky** directory (`/tmp`, mode `1777`): anybody may add an entry, and nobody
  may rename or remove an entry that is not theirs. The next directory down must still pass on
  its own.
- a directory **of this user whose group is this user's private group**: the group has the
  user's name, is the user's primary group and lists no other member. On a `USERGROUPS` system
  (Ubuntu, Fedora; umask `002`) every directory a user makes is `775`, with a group that is
  that user alone. Refusing these would refuse `~/.cache` on a default Ubuntu account. A
  directory that carries a POSIX access ACL gets no exemption, because an ACL can grant write
  to a named user behind the group bits.

The store and the artifact get neither exemption. `kuang-compile` makes them `0755` and `0644`,
and ADR-0870 §4 stays as it was for them.

**3. One descriptor, read into memory.** The artifact is opened with
`openat(store, name, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)`. It must be a regular
file, and it is held to the rule with `fstat` on that descriptor. Its bytes are then read from
that same descriptor and handed to `Component::deserialize`. Between the check and the use, no
name is looked up again. `O_NONBLOCK` keeps a FIFO from blocking the open. The regular-file check
then refuses it.

Reading the bytes into memory was chosen over two alternatives. One is mapping by
`/proc/self/fd/N` through `deserialize_file`. The other is `Module::deserialize_open_file`,
which has no component counterpart in wasmtime 47. Reading costs memory: wasmtime copies the
bytes into its own mapping, so a load holds the artifact twice for a moment. After that, each
shell holds a private, anonymous copy, where a file mapping would have shared the unchanged
pages through the page cache. The measured cost is below, and the gains are concrete:

- no dependence on `/proc` being mounted, which a chroot or a minimal sandbox may not have;
- what runs is exactly what was examined. A private file mapping still shows changes that
  someone else writes to its pages before they are first touched. A copy does not.

The one `unsafe` block stays one block, in `map`, with its `// SAFETY:` rewritten to this
argument.

**4. The tool walks the same way.** `kuang-compile` reaches its store with the same walk and
makes each missing directory with `mkdirat(…, 0755)` before opening it without following. So it
refuses to write below a directory, or through a link, that the shell would refuse to read. The
artifact is created with `openat(store, partial, O_CREAT | O_EXCL | O_NOFOLLOW)`, renamed into
place with `renameat` on the same store descriptor, and removed with `unlinkat` on failure.

## Consequences

- ADR-0870 §4 now covers every directory from `/` to the artifact. ADR-0870 §7's safety argument
  holds by check and not only by construction. The "file is not modified while mapped" clause no
  longer carries weight, because the shell loads a copy.
- A user whose cache sits below a directory that a shared group or everybody can write gets
  `untrusted`, with the directory named. That includes a `775` home or `~/.cache` whose group is
  not private. The refusal says what to correct.
- What the check still cannot see, stated so nobody assumes it:
  - a descriptor someone opened for writing before a file was made owner-only;
  - another account whose *primary* group is this user's private group, which only an
    administrator can arrange and which `/etc/group` does not list;
  - a flipped bit inside a correctly owned artifact (ADR-0870 §7).
- Memory: an artifact is held once per shell that loaded it, as anonymous memory, and twice for
  the moment of the load. Measured on this tree: the example component, built for
  `wasm32-wasip2` in the debug profile (12,814,965 B), compiles to a 7,373,424 B artifact; the
  empty component's is a few KiB. A shell with several large components loaded pays their
  artifacts' size in resident memory where a file mapping would have shared them — the trade
  this record makes for a load that cannot be raced.
- Tests:
  - `ono-kuang-sdk/tests/compiled.rs`:
    - `should_refuse_an_artifact_that_is_a_symbolic_link`
    - `should_refuse_a_store_that_is_a_symbolic_link`
    - `should_refuse_an_artifact_below_a_directory_others_can_write`: `0777`, and a user-owned
      `1777`; `775` with a private group is accepted
    - `should_refuse_to_write_below_a_directory_others_can_write`
    - `should_refuse_to_write_through_a_symbolic_link`
  - `ono-kuang-supervisor` `compiled::ownership_rule`: the directory rule, including a shared
    group, root's sticky directory, a user's sticky directory, and a user's tree seen by root.
  - `compiled::descriptor::should_load_the_file_it_examined_though_its_name_is_replaced_before_the_read`:
    it examines through the loader's own functions, replaces the name, and loads from the
    descriptor. A lookup by name then meets the replacement.
  - The race itself, a swap between `fstat` and `read`, has no deterministic test. The argument
    is that no name is resolved between the two.

## Alternatives considered

- **Keep the path-based check and add the ancestors.** This still leaves the window between the
  check and `deserialize_file` opening the path again.
- **Refuse every link on the way, ancestors included.** This refuses every home reached through
  a link (Fedora Silverblue's `/home -> /var/home`) and gains nothing. After resolution, the
  directories that are walked are the ones examined.
- **No private-group exemption (upstream `StrictModes`).** It is simpler. It also refuses a
  default Ubuntu account's `775` directories, with a remedy (`chmod g-w`) that users would have
  to repeat for every directory their umask creates.
- **Map through `/proc/self/fd/N`.** This keeps the page-cache sharing, but it depends on
  `/proc`, and it maps a file whose pages can still change underneath.
