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

**One rule: inside a job nothing resolves a relative path or starts a child through the process
working directory. Every relative path the job uses — bound, piped, captured before launch,
recorded in a plan, or handed to a child — means the job's directory, except where a link's remote
answers.**

The job's directory is the session `cwd` its evaluator was forked with (ADR-0952 §1), which
ADR-0955 made immutable; since ADR-0958 every backgrounded line is such a job. The rule is carried
by these mechanisms, each at a seam rather than in a command:

1. **Arguments.** `Scope::with_working_directory` fixes the directory a command's relative paths
   mean; a job's scopes carry the job's directory and the foreground's carry none, so the
   foreground resolves exactly as before. `CommandTable::run`, the seam every command runs
   through, anchors every relative value bound to a `path` or `list<path>` parameter after the
   context frames have filled theirs in; an expression argument is anchored when it is evaluated
   (`BoundArguments::evaluated`). Glob expansion yields the relative names it matched in the job's
   directory, anchored as they are bound. No command is special-cased.
2. **Piped objects.** A record that reaches a mutation inside a job may carry a relative path — one
   captured before the job started (`let fs = (get file a.o)`), or retained by the foreground. Its
   `path`-typed identity values and, for the filesystem's own objects, its source path are
   anchored before the action is built, so the action finds the object the record named.
3. **Omitted paths.** Anchored arguments put the directory on the provider query
   (`Query::within`); a provider that defaults a path to "here" — `get dir`, `get file`,
   `find file` — defaults to it.
4. **Children.** An action built in a job carries the directory (`Action::within`). A program a
   provider starts for it runs there — the `open file` handler, a package manager — and a
   path-shaped program or operand the provider would otherwise hand on relative is anchored: a
   `--with ./tool` handler, a `./foo.deb` archive, a mount source that is plainly a path (`./disk.img`,
   `images/disk.img`, not `tmpfs` or `server:/export`). An opaque plan action applied in a job runs
   its program in the job's directory, and a provider action applied there carries it.
   The job's own programs, glob expansion and `PATH` lookup already used the session's `cwd`.
5. **Strings that name local paths.** A plugin reference written as a path — `./pkg`, `path:pkg` —
   is anchored, and recorded absolute; so is `find plugin --source path:…`.
6. **Redirections and plans.** A native stage's redirection is opened in the job's directory
   (`Session::anchored_path`), and a `plan` made in a job resolves its operation's paths there
   before it freezes them.
7. **Links.** Inside a link frame a link's remote answers, and a directory of this machine means
   nothing there: the job's paths travel to it as written, with no directory on the query.

**The join keeps the text it joins** (`ono_provider_api::anchor_path`): `./x` becomes `<dir>/./x`,
`link/` keeps its trailing slash, `..` stays, nothing is canonicalized and symlinks are not
resolved, so the anchored path names the object the same words name in the foreground. A path that
does not exist yet is anchored like one that does; absolute paths and values of other types are
untouched.

What a job reports names the objects it acted on: its records carry the anchored, absolute path —
`get file a.o &` collects `/…/build/a.o`, not `a.o`. The foreground's output is unchanged.

ADR-0955's refusal of `cd`, `enter dir` and a restoring `leave` inside a job stands: the job's
directory is fixed, and nothing in a job moves it. This ADR replaces only that ADR's stated gap.

## Consequences

Easy: `remove file *.o &` acts on the job's files whatever the foreground does next; a job started
inside an entered directory keeps it after the foreground leaves; every native command gets the
rule without code of its own.

Accepted:

- Every `path`-typed parameter is anchored in a job — `add user --home rel` gets an absolute home.
  A job's relative path means its directory, whatever it names (review L2).
- The job's directory is named by its path at launch. Renaming it and creating another of the same
  name makes the job act in the new one; removing it makes the job's operations fail — nothing
  falls back to the process directory. That follows from naming the directory, and is a
  deliberate act rather than a race (review L3).

Hard:

- A provider that resolves a relative path held in a string field it alone interprets, and that is
  none of the above, still sees it as written. Every such site found in this build is listed
  above; a new one has to follow the rule.
- A KUANG/11 plugin provider receives anchored paths but not the directory of an omitted one; the
  host protocol carries no working directory. No plugin target defaults a path today.
- A job's records show absolute paths where the foreground would show the relative ones it was
  given.
- `$var | plan …` and `@-1 | plan …` are refused before any path is resolved (`plan` reads piped
  objects only after a command head), in the foreground as in a job, so the plan path of point 2
  is reached only through the job's own stages, which are anchored.

Encoded by `crates/ono-cli/tests/jobs_directory.rs`:
`::should_write_where_a_native_job_started_when_the_foreground_moved_before_it_wrote`,
`::should_read_a_relative_path_where_a_job_started_when_the_foreground_moved_before_it_read`,
`::should_remove_the_jobs_globbed_files_and_not_the_namesakes_where_the_foreground_went`,
`::should_list_the_jobs_directory_when_a_job_lists_without_naming_one`,
`::should_open_a_jobs_native_redirection_in_the_jobs_directory`,
`::should_keep_the_jobs_directory_when_the_foreground_leaves_the_directory_it_entered`,
`::should_run_a_jobs_programs_in_the_jobs_directory_and_leave_the_foregrounds_paths_relative`,
`::should_plan_a_jobs_relative_target_in_the_jobs_directory`,
`::should_remove_the_jobs_file_when_records_captured_before_launch_are_piped_into_a_mutation`,
`::should_run_an_opaque_plan_action_applied_in_a_job_in_the_jobs_directory`,
`::should_read_a_plugin_package_named_relatively_in_a_job_from_the_jobs_directory`,
`::should_start_a_handler_named_relatively_in_a_job_from_and_in_the_jobs_directory`,
`::should_send_a_jobs_relative_path_to_a_link_as_written`,
`::should_keep_a_trailing_slash_when_a_job_anchors_a_path`; the unit tests of
`ono-provider-api` (`anchoring`), `ono-provider-linux` (`mount_sources`, `local_packages` —
mounting and installing need root, so the anchoring itself is what is tested); acceptance case
`392`.

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
