# ADR-0957: A job's relative paths mean the directory it was started in

- Status: accepted
- Date: 2026-10-08
- Spec refs: spec §14.2, §17.3, §18.4; v0.4.1 §31.3; ADR-0024, ADR-0952 §1, ADR-0955
- Decided by: agent (autonomous)

## Context

A process has one working directory. The foreground session keeps the process in its own
(`Session::set_cwd`, v0.4.1 §31.3), and ADR-0955 stopped a job from moving it. The reverse stayed
open and ADR-0955 recorded it under *Consequences → Hard*: a job's native commands handed their
relative paths to the kernel when they acted, so a foreground `cd` after the job started redirected
them (issue #302). `cd ~/build; remove file *.o &; cd ~/src` expanded the glob in `~/build` and
could remove the namesakes in `~/src`. Programs a job starts were never affected — every child is
given the session's directory explicitly — and neither were the job's glob expansion and `cd`
resolution, which read the job session's `cwd`. Only the native side resolved through the process.

The invariant: a job's filesystem meaning is fixed by the job context established when it is
launched. Later foreground directory changes must not alter what a relative path belonging to the
job means, for reads and mutations alike and for every native command.

Two models were on the table: anchor the job's relative path operands on its directory before they
reach anything that acts on them, or hand every provider a directory to resolve against.

## Decision

**A job's relative paths are anchored on the job's directory where they become arguments, and the
job's provider queries name that directory.**

1. **The carrier is the invocation's scope.** `Scope::with_working_directory` fixes the directory a
   command's relative paths mean. The foreground's scopes carry none: its relative paths stay
   relative and the kernel resolves them through the directory the session keeps the process in,
   exactly as before. A job's scopes carry the job's directory, fixed at launch — the session `cwd`
   a job's evaluator was forked with (ADR-0952 §1, which ADR-0955 made immutable), or, for a line
   of native stages on ADR-0024's task path, the foreground's directory at the moment it was
   backgrounded.
2. **Anchoring is by declared type, at the one seam every command runs through.** `CommandTable::run`
   anchors the bound arguments before the implementation runs, after the context frames have filled
   theirs in: every value bound to a `path` or `list<path>` parameter that is relative is joined
   onto the directory. An argument written as an expression is anchored when it is evaluated
   (`BoundArguments::evaluated`), so `remove file $name` and `each { remove file @ }` are held to
   the same rule as a word. Glob expansion still yields the relative names it matched in the job's
   directory; they are anchored as they are bound. No command is special-cased.
3. **The join is lexical.** `.` components fall away, `..` stays, nothing is canonicalized, symlinks
   are not resolved, and a path that does not exist yet — a `write file` or `copy file` destination
   — is anchored like one that does. Absolute paths and values of other types are untouched.
4. **An omitted path means the directory too.** Arguments anchored on a directory carry it, and the
   provider query built from them names it (`Query::within`). A provider that defaults a path to
   "here" — the file provider's `get dir`, `get file`, `find file` — defaults it to the query's
   directory, and resolves a relative path in it, through `Query::resolve_path`; without a
   directory, both stay what they were.
5. **The other native consumers of a job's paths follow the same directory:** a native stage's
   redirection is opened in it (`Session::anchored_path`), and a `plan` made in a job resolves its
   operation's paths there before it freezes them. Programs, glob expansion and `PATH` resolution
   already used the session's `cwd`, which is the job's directory.

What a job reports names the files it acted on: its records carry the anchored, absolute path —
`get file a.o &` collects `/…/build/a.o`, not `a.o` — because a relative name in a job's output no
longer means anything once the foreground has moved. The foreground's output is unchanged.

ADR-0955's refusal of `cd`, `enter dir` and a restoring `leave` inside a job stands: the job's
directory is fixed, and nothing in a job moves it. This ADR replaces only that ADR's stated gap.

## Consequences

Easy: `remove file *.o &` acts on the job's files whatever the foreground does next; a job started
inside an entered directory keeps it after the foreground leaves; every native command — reads,
listings, mutations, plans, redirections — gets the rule without code of its own.

Hard:

- A provider that resolves a path the user did not write as a `path` parameter — a path inside a
  record that arrived on the pipeline from somewhere other than the job's own stages, such as a
  retained result `@-1` the foreground produced before it moved — sees it as written. A job's own
  stages produce anchored paths, so this needs a relative path made outside the job.
- A KUANG/11 plugin provider receives anchored paths but not the directory of an omitted one; the
  host protocol carries no working directory. No plugin target defaults a path today.
- A job's records show absolute paths where the foreground would show the relative ones it was
  given.

Encoded by `crates/ono-cli/tests/jobs_directory.rs`:
`::should_write_where_a_native_job_started_when_the_foreground_moved_before_it_wrote`,
`::should_read_a_relative_path_where_a_job_started_when_the_foreground_moved_before_it_read`,
`::should_remove_the_jobs_globbed_files_and_not_the_namesakes_where_the_foreground_went`,
`::should_list_the_jobs_directory_when_a_job_lists_without_naming_one`,
`::should_open_a_jobs_native_redirection_in_the_jobs_directory`,
`::should_keep_the_jobs_directory_when_the_foreground_leaves_the_directory_it_entered`,
`::should_run_a_jobs_programs_in_the_jobs_directory_and_leave_the_foregrounds_paths_relative`,
`::should_plan_a_jobs_relative_target_in_the_jobs_directory`; acceptance case `392`.

## Alternatives considered

**Hand every provider the job's directory and let it resolve.** Complete for providers, but the
change engine, redirections and every provider's own filesystem calls would each have to honour
it, and a provider that forgot would silently resolve through the process again. Anchoring at the
argument seam reaches every command at once; the query directory is kept only for what anchoring
cannot reach — a path the user left out.

**A per-thread working directory (`unshare(CLONE_FS)`).** ADR-0955 rejected it: a job's native
stages run as tasks on the shared pipeline runtime's worker threads, not on the job's thread.

**Make the foreground anchor too.** It would change what every foreground command reports (`a.o`
would become an absolute path) for no gain: the foreground's relative paths already mean the
directory the process is in.
