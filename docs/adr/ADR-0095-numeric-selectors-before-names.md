# ADR-0095: A numeric selector is declared before the name it could be mistaken for

- Status: accepted
- Date: 2026-08-27
- Spec refs: §6.1, §26.2; ADR-0009, ADR-0019, ADR-0021
- Decided by: agent (autonomous)

## Context

`docs/contracts/commands/identity.yaml` declares `get user` with `name: string` first and `uid: int`
second, and `get group` likewise. Binding is positional over alternatives: a bare word binds to
the first free selector whose declared type admits it (`bind.rs`), and every word is a string.
So `get user 0` bound `0` to `name`, asked NSS for an account called `0`, and answered nothing —
while spec §6.1 says a selector resolves to the object it names, and `identity.yaml` promised
the `uid` selector in the same breath. `process.yaml` never had the problem: it declares
`pid: int` before `name: string`, so `get process 4419` and `get process nginx` both bind.

## Decision

**A command that accepts both a number and a name declares the numeric selector first.** The
int selector refuses a word that is not a number, so the name still binds; the name selector
would accept anything, so it must come last. `identity.yaml` is reordered — `uid` before `name`,
`gid` before `name` — and the rule is the convention for every future contract with the same
pair. No binding code changes: the order of alternatives is the contract's to state, and the
contract is where a reader looks.

A login name that is all digits is the price. It binds as an id, exactly as in `getent passwd`;
`get user --name 0` is not offered because `get user` takes no options, and such a name is a
misconfiguration NSS itself warns about.

## Consequences

Easy: `get user 0` and `get group 0` resolve root; `get user root` still binds as a name;
help pages list the selectors in the order binding tries them.

Encoded by: the uid/gid cases of `crates/ono-cli/tests/options_and_selectors_missing.rs` and
acceptance case `041-config-and-resolve`.

## Alternatives considered

- **Prefer a typed selector in `bind.rs` when a word parses as its type** — rejected: it makes
  the binding order depend on the argument rather than on the contract, which `help` cannot
  explain, and `process.yaml` shows the contract can already say it.
- **A `--uid` option** — rejected: `identity.yaml` declares `uid` as a selector, and spec §6.1
  writes selectors positionally.
