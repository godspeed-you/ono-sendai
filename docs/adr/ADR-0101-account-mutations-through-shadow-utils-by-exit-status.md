# ADR-0101: Account mutations run the shadow-utils tools, read only their exit status, and refuse an unprivileged caller before anything runs

- Status: accepted
- Date: 2026-08-27
- Spec refs: §7.1, §11.5, §11.6, §16.5, §17.2, §23.6, §43, §50, §52; ADR-0006, ADR-0068, ADR-0102
- Decided by: agent (autonomous)

## Context

`docs/contracts/commands/identity.yaml` has declared `add|remove|set user` and `add|remove|set group`
since Phase D — the "M" cells of spec §52's matrix — with `provider_capability: user.manage` /
`group.manage`, `privilege: elevated` and `phase: planned`. Nothing implemented them: every
one answered `E0101 declared but this build implements nothing for it`.

The RED suite (`crates/ono-cli/tests/identity_missing.rs`) fixes what an unprivileged,
offline attempt must look like: one `ono.action-result/1` row, `status: failed`,
`changed: false`, the error `io.permission_denied` (E0302) or `safety.policy_denied` (E0702),
exit status 1, and the account database provably unchanged afterwards; a `remove` of an account
that does not exist is the structured not-found naming it.

Two roads exist for actually changing the database as root:

1. **Write the files** — `putpwent(3)`/`putgrent(3)` or hand-rolled I/O over `/etc/passwd`,
   `/etc/shadow`, `/etc/group`, `/etc/gshadow`, under `lckpwdf(3)`.
2. **Run the shadow-utils tools** — `useradd`, `usermod`, `userdel`, `groupadd`, `groupmod`,
   `groupdel`, `gpasswd`.

Road 1 would have to reimplement what the tools do beyond the four files: `/etc/login.defs`
and `/etc/default/useradd` defaults, uid/gid allocation ranges, subordinate ids
(`/etc/subuid`), the SELinux user mapping, the skeleton copy for a home directory, the
`nscd`/`sssd` cache invalidation, and the exact locking every other writer honours. Every one of
those is a way to corrupt an account database while looking correct in a test. Road 2 has one
hazard — the tools print text — and spec §50 forbids reading it.

## Decision

### 1. The tools, by exit status

**`add`, `remove` and `set` on `user` and `group` run the shadow-utils tools and read nothing
but their exit status** (`crates/ono-provider-linux/src/account_tools.rs`). The status is a
documented interface: each man page lists it (`4` uid/gid in use, `6` account does not exist,
`9` name in use, `1`/`10` database not updatable, `8` account in use, `12` home directory,
`2`/`3` invocation or argument). `outcome_of` maps those to the taxonomy of spec §43 —
`io.already_exists` (E0303), `io.not_found` (E0301), `type.mismatch` (E0201) for a rejected
argument, `external.exit_nonzero` (E0501) otherwise, `external.signal` (E0502) for a signal —
and an undocumented status is reported as the number it is. Whatever the tool wrote to stderr
travels as `metadata.stderr`, for the user, unparsed. A tool that is not installed is
`provider.unavailable` (E0401).

The mapping from contract to invocation is fixed and unit-tested:

| Command | Runs |
|---|---|
| `add user N [--uid U] [--home P] [--shell P] [--group G]` | `useradd [--uid U] [--home-dir P] [--shell P] [--gid G] N` |
| `remove user N [--remove-home]` | `userdel [--remove] N` |
| `set user N [--shell P] [--home P] [--group G]` | `usermod [--shell P] [--home P] [--gid G] N` (re-points the home; never moves it) |
| `add group N [--gid G]` | `groupadd [--gid G] N` |
| `add group N --member U` | `gpasswd --add U N`, once per member |
| `remove group N` | `groupdel N` |
| `remove group N --member U` | `gpasswd --delete U N`, once per member |
| `set group N --gid G` | `groupmod --gid G N` |

The tool is looked up on `PATH` and then in `/usr/sbin`, `/sbin`, `/usr/bin`, `/bin`, because
an unprivileged `PATH` rarely carries `sbin`.

### 2. The privilege gate comes first, from the kernel

**Before any tool runs, the provider checks its own effective uid; when it is not root, the
row is `failed` with `io.permission_denied` (E0302), naming the uid the shell runs as and the
tool that would have run.** The tools themselves refuse an unprivileged caller with status `1`
— the same status they use for a locked database — so reading their status would have turned a
permission problem into `external.exit_nonzero`. Asking the kernel first gives the code that is
true and does it without spawning anything, which is also what makes the RED suite's "the
account is provably unchanged" assertions trivially honest: nothing ran.

E0302 rather than E0702: no policy of the shell's forbade the operation; the operating system
would have. E0702 stays reserved for a configured safety policy (ADR-0010).

### 3. What is decided before the gate

Resolution and argument checking come before the privilege gate, so an unprivileged user gets
the *specific* refusal: `remove user nobody-such` is `io.not_found` naming the account (from
the seam of ADR-0068 §2, since `remove` resolves its selector), `set user root` with nothing
to set is E0201 naming `--shell, --home, --group`, and `--dry-run` reports the exact invocation
as a `skipped` row with `would run …` (spec §11.6 — the plan is worth seeing before elevating).

`add` does not resolve its selector: the name it carries need not exist yet (ADR-0102). For a
group, `--member` decides whether `add`/`remove` create/delete the group or change its
membership, as `identity.yaml` documents and §7.1's "membership/association" sense of `add`
intends.

### 4. Contracts

The six commands move from `phase: planned` to `phase: C` in `identity.yaml`: they are the
identity provider's, and Phase C is the phase that delivers system providers. `linux.nss`
advertises `user.manage` and `group.manage` (`docs/contracts/providers/linux-procfs.yaml`), both
`needing_elevation`, which is what binds `ProviderMutation` to them (ADR-0068 §3). Nothing
about the commands' options, types or output changed.

## Consequences

- An unprivileged shell answers every account mutation with one honest row and exit 1, in
  under a millisecond, without touching the database. A root shell changes the database the
  way the distribution expects, with its defaults and hooks.
- The root road is not exercised by the workspace's tests, which run unprivileged and must
  never change the developer's accounts. What is tested: the invocation each contract maps to,
  the status mapping, and every refusal path end to end. A privileged conformance run stays a
  deliberate, separate act (STATE.md → Next up).
- Tests: `crates/ono-cli/tests/identity_missing.rs` (11 mutation tests un-ignored),
  `crates/ono-provider-linux/src/account_tools.rs` unit tests, acceptance case
  `043-identity-sessions-and-accounts`.

## Alternatives considered

- **Write the database files directly (`putpwent` and friends).** Reimplements the
  distribution's account policy badly and races every other writer that honours the tools'
  lock. Rejected.
- **Read the tools' stderr to distinguish "permission denied" from "database locked".**
  Parsing text spec §50 forbids; the kernel answers the permission question directly.
  Rejected.
- **Let the tool run and map status 1 to E0302.** Status 1 is also "cannot update the file"
  for reasons that are not permission; the mapping would lie half the time. Rejected.
- **Answer E0702 for an unprivileged attempt.** No policy denied it; the OS would have.
  Rejected.
