# Changing things in Ono-Sendai

A shell sits unusually close to mutations. It is where files get deleted, services get restarted,
routes get changed, processes get killed and packages get installed, and in every other shell the
command line is the point of commitment. v0.6 puts a layer between the two:

```text
INTENT  ->  PLAN  ->  IMPACT  ->  PROTECTION  ->  APPLY  ->  VERIFY  ->  (RECOVER)
```

Everything below follows from one rule, which is worth reading twice before the commands:

> **Ono does not invent the future or recoverability.** Where a provider can prove an effect, Ono
> states it. Where it cannot, the answer is *unknown*. Where a recovery point genuinely covers
> something, Ono says what it covers — and, in the same breath, what it does not.

That is why several things here refuse, and why the protection block is never one word.

## A plan is not a dry run

```text
local:// > plan restart service nginx
```

does not restart nginx. It produces an object:

```text
PLAN / a82f  rev 1

intent
  restart service nginx

targets
  nginx.service

planned
  1  snapshot rpool/ROOT/debian
  2  restart nginx.service
  3  verify service running

impact
  direct        nginx.service
  related       4 worker processes, :80, :443
  possible      14 active client connections
  unknown       application-level client retry behaviour

protection   PARTIALLY_PROTECTED  1/2
  <-> rpool/ROOT/debian@ono-a82f          filesystem-consistent

not covered
  process runtime          a restart replaces the worker set; nothing restores a PID
  active TCP sessions      connections being served may be cut

risk          MODERATE
reboot        no

PLAN NOT EXECUTED
```

A dry run prints what it would do. A plan is a typed value: it has an identity, a revision, a
digest, frozen targets, an ordered action graph, preconditions, effects at four confidence
classes, a coverage matrix, verification contracts and a risk class. It survives shell exit, it
can be piped, and `to json` serialises it.

`PLAN NOT EXECUTED` at the bottom is not decoration. Nothing about creating or inspecting a plan
touches the target system, and that line is where you can see it.

### Three ways to make one

```text
plan restart service nginx                              one action

plan {                                                  several, in order
    copy file ./nginx.conf /etc/nginx/nginx.conf --overwrite
    restart service nginx
    verify service nginx state == running
    verify socket :443 exists
}

get service | where state == failed | plan restart service     one plan over the matched set
```

A `verify` line is not an action: it becomes a check the plan runs afterwards, which is what §23.1
asks every plan that mutates to carry. And a line naming an operation no provider contract
declares — `validate config nginx`, say, which is really nginx's own `nginx -t` — refuses rather
than being guessed at. ADR-0813 records why.

The third form is worth pausing on. It produces **one** plan whose targets are the services that matched
*at that moment*. If a fifth service starts failing before you apply, it does not join the plan.
That is deliberate: a plan you read is a plan you can apply, and a set that grows between reading
and applying is a set you did not read.

The block form describes actions. It is not a workflow language — there are no loops, no functions,
no background jobs, and there will not be. Where you want those, you want a program, and Ono runs
programs.

### What cannot be planned

```text
local:// > plan sh -c 'rm -rf /somewhere'
Ono-Sendai-E1714 change.opaque_action_forbidden
```

Ono cannot reason about what an arbitrary command touches, so it does not pretend to. An operation
becomes plannable when a provider declares its target schema, its mutation semantics, the
capabilities it needs, its preconditions, its effects, how it executes, whether it is idempotent,
what recovery it has — or explicitly has none — and what can be verified afterwards.

There is an escape for the case where you accept the uncertainty, and it is honest about what it
costs: an opaque action's impact and reversibility are classified unknown, and a plan containing
one can never be `PROTECTED`, however much of the filesystem is snapshotted.

### Plans and the past

```text
local:// @12:17 [PAST] > plan restart service nginx
Ono-Sendai-E1715 change.historical_context_read_only
```

A plan you intend to apply is resolved against the world it would change. Historical state can be
inspected; it cannot be the base for a mutation. `now` returns to the present.

## Protection is a matrix, not a badge

The single most important thing v0.6 does is refuse to answer "is this reversible?" with a boolean.

Protection is computed per **mutation domain**. A plan that replaces a config file and restarts a
service touches three of them, and they have three different answers:

```text
mutation domain             needs                 covered by
filesystem-persistent       the exact prior bytes  ZFS snapshot rpool/ROOT/debian@ono-a82f
process-runtime             a healthy service      restarting it again
network-runtime             nothing                — excluded
```

The word at the top of the block is composed from those rows and can never be set independently.
Six words are possible:

| word | means |
|---|---|
| `UNPROTECTED` | nothing covers the persistent effects this plan requires |
| `COMPENSATABLE` | no prior state image exists; inverse actions can restore an acceptable semantic state |
| `PARTIALLY_PROTECTED` | some important effects are covered and others are not |
| `PROTECTED` | every persistent state mutation the plan requires has a validated recovery path |
| `TRANSACTIONAL` | one provider guarantees atomic commit and rollback for the whole scope |
| `UNKNOWN` | the recovery properties could not be established |

`PROTECTED` is a claim about persistent state, and it is shown with its exclusions or not at all.
A plan can be `PROTECTED` and still be the most dangerous thing you do that week — restoring a root
snapshot is protected, needs a reboot, and discards everything since. **Risk and recoverability are
separate axes**, and the plan shows both.

### What a snapshot does not cover

Three things are worth stating plainly, because all three are places where a reasonable person
would assume otherwise.

**A snapshot is not a backup.** A ZFS or Btrfs snapshot lives on the storage it protects. If the
pool is lost, so is the recovery point. Ono calls these local recovery points and never implies
they protect against device corruption or pool loss.

**A snapshot of a parent does not cover a child.** If `/` is `rpool/ROOT/debian` and `/data` is
`tank/data`, snapshotting the first protects nothing in the second, however the paths nest. Ono
resolves each target to the persistence object that actually holds its state, and says which:

```text
local:// > inspect plan a82f --resolution

/etc/nginx/nginx.conf
  mount        /
  filesystem   zfs
  dataset      rpool/ROOT/debian
  recovery     zfs snapshot available

/data/customer.db
  mount        /data
  filesystem   zfs
  dataset      tank/data
  recovery     zfs snapshot available    (a separate asset)
```

**A Btrfs snapshot is not recursive across nested subvolumes.** A snapshot of `@var` contains an
*empty directory* where a nested `@var/lib-app` was mounted. Ono scans for the boundaries and
creates a separate asset for each subvolume a plan requires — and the coverage of a parent snapshot
explicitly excludes every nested subvolume beneath it.

**Runtime and external effects are never covered by storage.** Killing a process is irreversible
whatever the filesystem offers. An HTTP request that has been served has been served. Both stay in
the plan's `not covered` block after protection, because that is what §2.13 is for.

## Applying

```text
local:// > apply a82f

PREPARING
  [1/1] create ZFS recovery point            PASS

PROTECTION READY
  rpool/ROOT/debian@ono-a82f

APPLYING
  [1/3] replace nginx.conf                   PASS
  [2/3] validate nginx config                PASS
  [3/3] restart nginx.service                PASS

VERIFYING
  service running                            PASS
  listener :443                              PASS

PLAN VERIFIED

recovery point retained
  recovery/r-a82f
  expires in 24h
```

Three things happen in a fixed order, and the display keeps them apart on purpose.

**Preparation runs immediately before the first mutation**, so the recovery point reflects the
state that is about to change rather than the state from when you were writing the plan. If a
required protection action fails, nothing is mutated at all — the plan becomes `PREPARE_FAILED`,
and that is a different fact from `APPLY_FAILED`, which means what ran, ran.

**Drift stops the apply.** The preconditions the plan froze are rechecked first. A file whose hash
moved, a service that was replaced, a package at a different version — any of them refuses, names
what changed, and mutates nothing. `rebase plan a82f` resolves the plan again against the world as
it is now, as a new revision, leaving the sealed one exactly where it was.

**Verification is a separate question from success.** A command that exited zero has proved that it
exited zero. Whether the state you asked for exists is asked afterwards, against the world. A plan
whose actions all succeeded and whose required check failed is `FAILED`, and its recovery assets
are kept.

### Gates

For an ordinary plan, `apply` is the commitment and there is no prompt. Two things add one:

```text
local:// > apply f31c
Ono-Sendai-E1723 change.risk_not_accepted

  18/18 frontend nodes will restart.
  No healthy serving member is excluded.
  Protection does not preserve active client sessions.

  apply f31c --accept-service-outage --accept-risk
```

The gate says what is actually risky rather than asking whether you are sure. A script supplies
the same acknowledgements as flags and never waits for a prompt — a non-interactive run that
cannot satisfy its policy fails with a structured error instead of blocking.

## Recovering

`recover` **plans** recovery. It does not perform it.

```text
local:// > recover a82f

RECOVERY PLAN / r91c

NEWER STATE AT RISK
  nothing — the restore set is one file, unchanged since the recovery point

source
  plan a82f
  recovery/r-a82f

recommended method
  selective restore

will restore
  /etc/nginx/nginx.conf

will preserve
  /etc/ssh/sshd_config   changed after the plan
  /etc/hosts             changed after the plan

runtime
  nginx restart required
  TCP sessions cannot be restored

risk
  MODERATE

RECOVERY NOT EXECUTED
```

The extra step exists because recovery can destroy things. Between your change and now, other
things happened: files were written, packages were updated, snapshots were taken. A rollback that
ignores them is a data-loss event with a friendly name.

So Ono compares the recovery target against the current state first, classifies every newer object
as preserved, discarded, conflicting or unknown, and shows the result **above** the restore detail.
Then you apply the recovery plan like any other plan — it has an impact, a risk class, verification
contracts and a lifecycle of its own.

### The method matters more than the asset

Ono prefers the method that loses the least, in this order:

1. the owning provider restores its own object — a package downgrade, a database point-in-time restore
2. selective restore of the wanted objects out of the recovery asset
3. clone the asset somewhere else and copy the wanted state out
4. compensation — an inverse action, which restores semantics and not prior state
5. subvolume replacement
6. full dataset rollback
7. offline or next-boot recovery

The common case is the second, and the reason is worth an example. Your plan changed one config
file at 10:00. At 12:00 a package update rewrote forty files in `/etc`. At 13:00 you want the
10:00 config back. Rolling the dataset back to 10:00 would take the package update with it.
Selective restore takes the one file and leaves the rest, and that is what Ono chooses.

### When recovery would destroy something

```text
local:// > apply r91d
Ono-Sendai-E1810 recovery.destructive_history_not_accepted

  rolling tank/data back to @ono-b91c would destroy:
    tank/data@later-1
    tank/data@later-2
  and discard an estimated 18 GiB of changed blocks.

  apply r91d --accept-newer-state-loss
```

Ono never adds a destructive flag on your behalf. If a rollback needs newer snapshots, bookmarks or
clones destroyed, every one of them is named and the recovery does not run until you say so. A
recovery whose drift analysis could not be completed is gated for the same reason: an unanalysed
recovery is not a safe one.

### What recovery verification claims

```text
RECOVERY VERIFICATION

persistent state
  nginx.conf             RESTORED
  package version        RESTORED

runtime
  service state          RESTORED
  worker PIDs            DIFFERENT / EXPECTED
  TCP connections        NOT RECOVERABLE

external side effects
  1 webhook request      NOT RECOVERABLE

result
  PERSISTENT STATE VERIFIED
  FULL WORLD EQUIVALENCE NOT CLAIMED
```

Ono will not tell you a rollback succeeded, because "succeeded" is not a thing that can be true of
a whole machine. It tells you which domains came back. New worker PIDs after a restart are
`DIFFERENT / EXPECTED` — recovery never claimed to restore a process identity, and reporting that
as a failure would be as wrong as reporting it as a success.

## Recovery assets

```text
local:// > get recovery

ID        TYPE    PLAN   AGE   COST        EXPIRES   STATUS
r-a82f    zfs     a82f   14m   312 MiB ~   23h46m    ready
r-91aa    btrfs   91aa   3h    1.8 GiB ~   21h       ready
r-77c0    file    77c0   2d    4.1 KiB     held      ready
```

The `~` means estimated. Copy-on-write space accounting is not the space that would be freed by
deleting a snapshot, and Ono says so rather than printing a number that looks exact. A snapshot is
never reported as free.

Assets are kept for 24 hours after a successful verification, by default. Two rules make that safe:

- **Assets of a plan that did not succeed are not removed by the ordinary rule.** A `FAILED`,
  `DEGRADED` or `RECOVERY_FAILED` plan keeps its recovery points until you decide otherwise.
- **Cleanup that would strand a plan refuses.** Removing an asset a retained plan still depends on
  answers `recovery.cleanup_blocked` and names the plans that would become unrecoverable.
  `remove recovery r-a82f --dry-run` shows the same thing without removing anything.

## Policy

```toml
[change]
default_protection = "prefer"      # off | prefer | require | maximize
default_strategy = "sequential"

[recovery]
retention = "24h"
min_filesystem_free = "10%"

[recovery.zfs]
prefer_selective_restore = true
allow_destructive_rollback = false
```

`prefer` is the interactive default: discover cheap protection, include it in the plan, create it
during preparation, and abort before mutation if it fails. `require` refuses to apply when a
required domain cannot reach the plan's protection class. `off` creates nothing and still tells you
what was available. `maximize` adds every non-conflicting mechanism inside the configured cost
limits — it does not mean "snapshot the host".

The four profiles of Appendix H (`interactive`, `cautious`, `fleet`, `scripted`) expand to those
same settings and hide nothing. One rule governs all of it: **the strictest requirement in force
applies**, whichever of configuration, profile or plan states it. A profile cannot loosen a plan
and a plan cannot loosen a profile.

## What this is not

v0.6 is a safe prospective interface for changes made through Ono and its providers. It is
deliberately not a configuration-management system, a deployment DSL, a workflow engine, a
distributed transaction manager, a backup product, or a universal undo button. Where an effect
cannot be undone, Ono says so; where a change spans several providers, Ono calls it a change set
and not a transaction; and where it does not know, it says that too.

## Troubleshooting

| you see | what it means |
|---|---|
| `change.plan_drift_detected` | the world moved after the plan was sealed. Nothing was changed. `rebase plan <id>` |
| `change.prepare_failed` | protection could not be created. **Nothing was mutated.** |
| `change.apply_failed` | a mutating action failed. Some of it ran. `inspect plan <id>` shows exactly what |
| `change.verification_failed` | the actions succeeded and the state you asked for does not exist |
| `recovery.coverage_insufficient` | `require` policy, and a domain has no validated recovery path. The matrix says which |
| `recovery.cleanup_blocked` | an asset a retained plan needs. The refusal names the plans |
| `recovery.plan_incomplete` | a fact recovery needed could not be established, so it was blocked rather than guessed |
| `change.plan_already_applying` | another session holds the apply claim |
