# ADR-0198: Denied is not missing, and a directory nobody may read is not a place to stand in

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §30.2, §35.1, §35.2, §35.3, §40, §44.8, §49.8, §53; v0.2 §16 (`io.permission_denied`)
- Decided by: agent (autonomous), delivering v0.4 §50 Phase S11

## Context

`spatial.permission_denied` is one of the fourteen codes §40 requires, and §35.2 keeps
`permission_denied` distinct from `empty` and from a path that is not there. Two paths through
the filesystem side of the shell did not honour that.

**A denied path was reported as missing.** `read_path` collected the file provider's answer and
dropped its errors, so `enter /root/.bashrc` — where the provider correctly answers
`Ono-Sendai-E0302 io.permission_denied` — became `spatial.not_found`, "the path does not exist".
That is a statement about the machine, and it is false: the shell has no idea whether the file is
there, and §53 settles that unknown and denied are not the same answer as empty or missing.

**A directory nobody may read became the working directory.** §30.2 and §53 make entering a
directory a `cd`, and the `cd` happened whether or not the directory could be used. Standing in a
directory with no permissions leaves a shell that cannot spawn anything at all — every external
command fails with `io.permission_denied` on the cwd — and nothing had said why.

## Decision

1. **The file provider's refusal keeps its meaning.** `read_path` answers `Found`, `Denied` or
   `Absent`. A collected error of kind `permission` makes the answer `Denied`, and
   `observe_place_at` raises `spatial.permission_denied` naming the provider's reason.
   `spatial.not_found` stays the answer for a path nothing answered for and nothing refused.
2. **`enter <directory>` is refused with `spatial.permission_denied` when the directory cannot be
   read.** The place is legible from its parent; standing in it is not. §35.1 forbids revealing
   contents the provider could not answer for, §15.4 makes a directory place its listing, and
   §49.8 requires the shell to still be a shell afterwards. The refusal moves neither the place
   nor the working directory.
3. **Neither refusal offers a way around itself.** §35.3: the help says the boundary is real and
   names running the inspection as a user who may read it, and never `sudo`.

## Consequences

- `enter /root`, `enter /root/.bashrc` and a mode-`000` directory are all
  `Ono-Sendai-E1008 spatial.permission_denied`, and the shell that refused them still runs
  programs.
- A directory with execute permission but no read permission can no longer be entered. That is
  deliberate: §15.4 makes the place a listing, and a listing this user may not read is the
  boundary §35.2 asks to be shown rather than a place to stand in silently.
- Encoded by `spatial_identity_missing::should_refuse_a_path_this_user_may_not_read_as_denied_rather_than_as_missing`,
  `::should_refuse_to_stand_in_a_directory_this_user_may_not_read`,
  `::should_keep_the_working_directory_usable_when_a_denied_directory_is_named`, and
  `docker/acceptance/cases/097-spatial-permission-honesty.case`.

## Alternatives considered

- **Map the provider's refusal onto `spatial.not_found` and mention permissions in the help.**
  Rejected: the code is the contract a script reads (§40), and §35.2 requires the states to stay
  distinct, not to be distinguishable by reading English.
- **Let `enter` succeed and leave the cwd where it was.** Rejected: §53 settles that entering a
  directory changes cwd, and a place whose one documented effect is skipped is a place that lies
  about what it did.
