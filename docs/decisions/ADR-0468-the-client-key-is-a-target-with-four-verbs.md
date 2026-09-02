# ADR-0468: The client key is a target with four verbs, and adding one grants observation

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §9.4, §9.7, §56.3, §59.1, §59.2, Appendix C; spec §7.1, §9.1, §27, §50;
  ADR-0090, ADR-0104, ADR-0118, ADR-0355, ADR-0466
- Decided by: agent (autonomous)

## Context

§9.7 names four canonical targets — `get`, `add`, `set` and `remove client-key` — and permits a
different internal name "only if an ADR demonstrates that it fits the existing command registry
better without reducing clarity". It does not, so this ADR records the four as written and settles
what they answer with; the interesting decision is the second one, §9.4's default grant.

## Decision

**`client-key` is an ordinary target of `ono.shell`, answering `ono.client-key/1` records.** It
is the mirror of `host-key` (ADR-0355) from the other side of the link: a pinned host key is which
machines *this shell will link to*, an authorized client key is which clients *this machine will
serve*. Both are decisions the shell recorded rather than something a provider found, so both are
session state under ADR-0090, both live in `docs/spec/commands/remote.yaml`, and both get help,
completion, `get command` and the piped form of ADR-0118 for free by being registry commands.

The record is §9.3's model plus the store path: `fingerprint`, `label`, `observe`, `actions`,
`path`. §9.7 requires the listing to show exactly those five, and requires that the user-facing
concept stay "authorized client key" rather than a vague ACL blob — which is what a typed record
with named fields is and an opaque policy string is not.

**`add client-key <fingerprint>` grants observation and nothing else, and there is no option on it
that grants an action.** §9.4 is the rule; the shape is the enforcement. An `--allow` on `add`
would make the safe form the longer one, and the first thing anybody would paste into a runbook is
the form with the flag. Granting an action is `set client-key <fingerprint> --allow <id>`, a second
deliberate command, which is also where §9.5's exact-id parsing lives (ADR-0469).

**`add` refuses a client that is already authorized**, with `io.already_exists` (E0303). Adding
twice would otherwise silently narrow a grant somebody made deliberately back to observe-only —
a mutation that reads like a no-op. Widening or narrowing is `set`, exactly as re-trusting a host
is `set host-key` rather than a second `add`.

**`set` changes only what it names.** `--allow` replaces the action allowlist and preserves the
observe state, which is §9.7's wording; `--observe` changes the observe state and leaves the
allowlist; `--label` renames. A field the command did not mention is a field the command did not
touch, so no operator loses a grant by fixing a label.

**`--allow` takes a comma-separated list in one word**, quoted where there is more than one:
`set client-key <fp> --allow "process.signal,service.manage"`. §9.7 writes `--allow
<capability>...`; the shell's `words` argument mode has no repeated-option form, and a bare comma
ends a word in the grammar. A comma-separated value is the smallest thing that fits the existing
binder, and it is a list of exact ids either way — the separator is not what §9.5 is about.

## Consequences

Easy: `get client-key | remove client-key` revokes everything, because the piped form comes with
being a registry command. An operator reads the store as objects and edits it with verbs they
already know, and the file underneath stays something they can open.

Hard: `client-key` had to be declared in four places before it existed anywhere — the schema, the
target registry, the command contracts and the provider declaration — and `ono-value` had to embed
the new schema contract. That is the cost of `docs/spec/` being the public contract (spec §27), and
`cargo xtask spec-check` is what makes the cost mandatory rather than optional.

Encoded by: `crates/ono-cli/tests/client_keys.rs::should_list_every_authorized_client_as_an_object_when_get_client_key_runs`,
`::should_add_a_client_key_and_show_it_in_the_next_listing`,
`::should_grant_observe_only_when_a_client_key_is_added_without_grants`,
`::should_change_exactly_the_grants_named_when_set_client_key_runs`,
`::should_remove_a_client_key_so_the_store_no_longer_lists_it`,
`::should_carry_help_and_completion_for_every_client_key_command`,
`crates/ono-cli/tests/authenticated_link.rs::should_refuse_the_next_connection_from_a_revoked_client_key`,
cases `182` and `186`.

## Alternatives considered

**`authorize client` / `revoke client` as verbs of their own.** Reads well in a runbook. Rejected:
spec §7.1's verb set is closed and `authorize` is not in it, and §9.7 names the four this ADR
implements.

**`--allow` repeated once per capability.** Closest to §9.7's `<capability>...`. Rejected for now:
the argument binder keeps one value per option, so implementing it means changing `ono-command`,
which is a different increment with a different blast radius. Recorded in `docs/STATE.md` under
*Found, not yet filed*.
