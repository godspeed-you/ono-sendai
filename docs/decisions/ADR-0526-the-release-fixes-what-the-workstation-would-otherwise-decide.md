# ADR-0526: The release fixes what the workstation would otherwise decide

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §2.8 (reproducible means immutable inputs), §46.2 (source date), §46.3
  (locale and timezone), §46.4 (file ordering and ownership), §65.11 (mutable release inputs)
- Issues: #104 — consumed by #105 (the comparison that fails when one of these moves)
- Decided by: agent (autonomous)

## Context

`scripts/package.sh` derived `SOURCE_DATE_EPOCH` from the release commit and passed it into the
build container. That was one of four inputs §46 names, and the other three were whatever the
machine happened to be.

The consequence is not theoretical, and writing the test found it twice in ten minutes:

- **the packaged mtimes rendered differently on the same bytes.** `dpkg-deb --contents` formats
  a member's mtime in the *reader's* timezone. The first run of
  `should_normalize_file_ownership_and_mode_in_every_produced_package` read `2023-11-14 23:13`
  where the archive holds `1700000000`, because the test process had inherited `TZ=Europe/Berlin`
  from the developer's session. Nothing was wrong with the package; the reader was unfixed. A
  tool that *writes* a rendered timestamp into a control field rather than reading one has the
  same exposure and no test to notice it.
- **the archive records numeric ownership, not names.** Both packaging tools write uid 0 / gid 0
  and no `uname`/`gname`, so `dpkg-deb` prints `0/0`. The assertion had to say what the format
  actually guarantees, which is the identity and not its spelling.

`git log -1 --format=%ct` also fails silently outside a checkout, and `${SOURCE_DATE_EPOCH:-…}`
would have replaced an explicitly empty value with a derived one — two ways for a build to
continue with no deterministic date at all.

## Decision

**`scripts/package.sh` fixes all four determinism inputs before its first tool runs, and refuses
to build when one of them cannot be fixed.**

```text
LC_ALL=C.UTF-8   LANG=C.UTF-8   LANGUAGE=C   TZ=UTC   umask 022
SOURCE_DATE_EPOCH = $SOURCE_DATE_EPOCH, else `git log -1 --format=%ct`
packages are written as uid 0 / gid 0
```

Three rules make that more than a list of assignments:

- **The block runs before anything reads the environment**, above the argument loop and above
  `rustc -vV`, because a tool started earlier would already have taken the locale it found.
- **`${SOURCE_DATE_EPOCH-}` rather than `${SOURCE_DATE_EPOCH:-}`.** An empty value the caller set
  deliberately is not a value this script may replace. It is a value it must refuse.
- **`require_determinism` refuses; it does not fall back.** A build with no derivable date has
  exactly one alternative — the wall clock — and that is the thing §46.2 forbids. The refusal
  names the input that was missing, because the message is what a maintainer acts on.

The same four values are passed into the build container (`--env LC_ALL --env LANG --env TZ`
beside the epoch that was already there), so the container inherits the release's environment
rather than the daemon's.

`--print-determinism` prints the resolved values and exits without building. It exists so the
contract can be observed from outside at the cost of one process rather than one release build;
`xtask/tests/packaging.rs` is its only caller.

## Consequences

Easy: #105 can compare two builds and know that a difference is a difference in the *source*,
not in the shell that launched them. #108's provenance can record the epoch as a bound fact.

Hard: a maintainer packaging from an exported tarball rather than a checkout now has to set
`SOURCE_DATE_EPOCH` by hand. That is the intended trade — the alternative is an artifact whose
date nobody can reproduce, which §49.2 calls a hidden local step in everything but name.

Encoded by `xtask/tests/packaging.rs`:

- `should_set_every_determinism_input_before_a_release_build` — runs the real script under a
  German locale in `Europe/Berlin` and requires `C.UTF-8`, `UTC`, `0022`, `0:0` and the release
  commit's own timestamp to come back out;
- `should_normalize_file_ownership_and_mode_in_every_produced_package` — builds both packages at
  a fixed epoch and requires every member of both to carry it, owned by uid 0 / gid 0, with the
  declared mode and no setuid/setgid/sticky bit;
- `should_refuse_a_release_build_that_leaves_a_determinism_input_unset` — a tree git cannot date,
  and a `SOURCE_DATE_EPOCH` that is not a timestamp; both are refused, and the refusal names the
  input.

## Alternatives considered

**Set the values in `.github/workflows/release.yml` instead.** They would then hold in CI and
nowhere else, and §49.2 asks that a public release be reproducible from repository automation —
which means the script a maintainer runs locally has to be the same one. The workflow calls this
script; putting the rule in the script puts it in both places at once.

**Default `SOURCE_DATE_EPOCH` to `date +%s` when git cannot answer.** It would make the script
never fail, and every artifact it produced would be unreproducible without saying so. §2.3 fails
closed; this is the same rule applied to the build.

**Normalize ownership after the fact with `fakeroot`/`tar --owner=0`.** Both packaging tools
already write uid 0 / gid 0. A second mechanism to enforce what the first one guarantees would be
untested code that only runs when the first one breaks.
