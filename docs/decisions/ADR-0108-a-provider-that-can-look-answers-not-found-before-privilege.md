# ADR-0108: A provider that can look answers not-found before it asks for privilege

- Status: accepted
- Date: 2026-08-27
- Spec refs: §16.5, §17.2, §43
- Decided by: agent (autonomous, integrating `implementation-identity` into `implementation`)

## Context

ADR-0088 §2 sends a selector nothing resolved to the provider as the user named it, because
the kernel refuses an unprivileged caller before it looks and that refusal outranks "not
found". ADR-0102 §1 promises the opposite for accounts: `remove user nobody-such` "stays the
E0301 row naming the account" — written when the shell still turned a failed resolution into
that row itself. Merged, the identity provider's privilege gate answered first, and
`remove user nobody-such-user-ono` reported `io.permission_denied` for an account that does not
exist (`identity_missing.rs`, acceptance case 043).

## Decision

The rule of ADR-0088 §2 stands: the provider is asked. Which refusal comes first is the
provider's to decide from what it can see. A provider whose object table is readable without
privilege — the account database — looks first and answers `io.not_found` for an object that is
not there, in the shell's own words (`no user answers to name …`), before its privilege gate. A
provider that cannot look without privilege — the kernel's routing and mount tables — keeps
answering permission first, as ADR-0088 §2 describes.

For the identity provider that means `remove`/`set user`, `remove`/`set group` and `add group
--member` check that the account exists before anything else; `add user`/`add group` create and
check nothing.

## Consequences

- `identity_missing.rs::should_report_a_structured_not_found_when_removing_a_user_that_does_not_exist`
  and case 043 pass with the provider asked; `network_missing.rs` keeps its permission-first
  rows.
- A provider added later chooses by the same test: can it tell the object is absent without
  privilege? Then it says so first.
