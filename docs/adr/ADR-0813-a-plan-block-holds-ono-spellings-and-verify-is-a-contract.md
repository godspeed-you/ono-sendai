# ADR-0813: A plan block holds Ono spellings, and a `verify` line is a contract rather than an action

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.6 §5.1, §5.2, §6.1, §6.2, §23.1, §31, §64; v0.2 §7, §7.1, §40.1
- Decided by: agent (autonomous)

## Context

§5.2 and §31 write the reference plan block as:

```text
plan {
    replace file /etc/nginx/nginx.conf from ./nginx.conf
    validate config nginx
    restart service nginx
    verify service nginx state == running
    verify socket :443 exists
}
```

Three of those five lines are not spellings this shell has.

`replace` is not in `docs/contracts/verbs.yaml`, and v0.2 §7 keeps the verb registry deliberately
small: "third-party modules reuse an existing verb whenever the semantics match", with §40.1
requiring a verb review before a new one is added. The semantics here match `copy` and `write`
exactly.

`validate config nginx` has no verb and no target: `validate` is not a verb, and `config` in this
shell is the *shell's* configuration (`get config`, `set config`), not a service's. What the line
describes is nginx's own `nginx -t`, which is a provider-specific action.

`verify service nginx state == running` uses a verb that exists, and means something different from
the four lines above it. The first three change the system; this one asks a question about it
afterwards. §23.1 calls that a verification contract, and §4.7's APPLYING phase does not run it.

## Decision

**A plan block holds ordinary Ono command spellings, and nothing else.** A statement is resolved
through the same registry the shell resolves a command through, which is what makes §6.1's
plannability question answerable at all: an operation is plannable when a contract declares its
semantics, and the contract is the one `docs/contracts/commands/` already holds.

So §31's block, in this shell, is:

```text
plan {
    copy file ./nginx.conf to /etc/nginx/nginx.conf
    restart service nginx
    verify service nginx state == running
    verify socket :443 exists
}
```

Three consequences, and each is a rule rather than a workaround:

1. **A `verify` statement becomes a `VerificationContract`, not a `PlanAction`.** §5.2's block
   describes what the plan does *and* what it will check, and §23.1 requires the second. The
   builder splits on the head: `verify` lines go to the verification set, everything else to the
   action graph. A block of nothing but `verify` lines is a plan that mutates nothing and needs no
   contract of its own, which §23.1 permits.
2. **`replace` is not added as a verb.** §7's rule is to reuse where the semantics match, and
   `copy file <from> to <to>` and `<pipeline> | write file <path> --overwrite` are both already
   there. A fourth spelling for writing a file would be a verb review's worth of cost for a synonym.
3. **`validate config nginx` is not plannable in v0.6, and says so.** §6.2 is the answer: an
   operation with no provider contract declaring its semantics refuses with
   `change.action_not_plannable`, and §6.2's second sentence is the route to making it plannable —
   "An external-command adapter from v0.3 MAY expose a plannable action if its contract is
   explicit." An nginx adapter that declares `nginx -t`'s target scope, its effects and its
   verification would make the line work, and writing one is a different piece of work from this
   tranche.

## Spec deviation

- Section: v0.6 §5.2, §31, §64
- Text: "`replace file /etc/nginx/nginx.conf from ./nginx.conf`" and "`validate config nginx`"
- Instead: `copy file ./nginx.conf to /etc/nginx/nginx.conf`, and `validate config nginx` is
  refused as not plannable until an adapter declares its contract.
- Why: v0.2 §7 governs the verb vocabulary and §40.1 requires a review before adding one; both
  spellings the specification uses are illustrative prose about *what* the plan does rather than
  normative statements about how it is typed. §6.1 and §6.2 are normative, and they say an
  operation with no declared contract is not plannable — which is the honest answer for
  `validate config nginx` rather than a special case.

## Consequences

§31's reference workflow is provable end to end with what the shell has: a file is replaced, a
service is restarted, and two verification contracts are checked afterwards. The configuration
validation step is the one thing missing from it, and it is missing visibly — the plan refuses to
include an action nobody declared, rather than including one whose effects Ono invented.

The split on `verify` means a block's line count and its action count differ, which is right:
§20.2's `planned` block lists actions and its `verification` block lists checks, and a reader
comparing the two against what they typed sees where each line went.

## Alternatives considered

**Add `replace` and `validate` as verbs.** Rejected: §40.1 asks for a verb review, `replace`
duplicates `copy` and `write`, and `validate` would be a verb whose only target is a thing no
first-party provider serves.

**Treat an unrecognised block line as an opaque action.** Rejected: it inverts §6.2's default.
An operation Ono cannot reason about refuses, and §6.3's escape is an explicit request rather than
a fallback.
