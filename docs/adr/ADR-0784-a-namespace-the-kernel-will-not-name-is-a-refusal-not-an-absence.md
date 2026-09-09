# ADR-0784: A namespace the kernel will not name is a refusal, not an absence

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.4 §35.1, §35.2, §35.3, §40, §41.2, §42.4; v0.2 §35.3
- Decided by: agent (autonomous)

## Context

`enter process/1; look` reported the `namespaces` exit of pid 1 as `empty` on an ordinary
unprivileged login, and `follow namespace` answered `spatial.no_relation` — "this place has no
`namespace` to follow". Neither statement was established by anything. `/proc/1/ns/` belongs to
root and discloses nothing to this user; what the shell knew was that it had not been told, and
what it printed was that there was nothing to tell.

v0.4 §35.2 lists six permission states and says "These states MUST remain distinct", with its own
worked example: `files — permission denied for 14 process FDs` is preferable to `files — 0`.
§42.4 states the consequence for a provider — "Denied information must produce
`permission_denied` or `unknown`, never false empty collections" — and §35.3 is the rule
underneath both: an unknown is never fabricated. A refusal rendered as an absence is a fabricated
fact about the system.

The spatial layer was already built for this. `ProviderBridge` reads a record's relation fields
through a `Reader` that keeps refusals apart from absences: a field carrying an `ono.error/1` is
withheld and reaches the place view as `permission_denied`, while a field carrying nothing is an
absent neighbour. That is why the `files` and `sockets` exits of the same place already said
`permission denied`. The `namespaces` exit did not, because the field it is built from —
`pid_namespace` on `ono.process/1` and `ono.process-detail/1` — arrived as null.

The cause is in the provider, and it is a property of `procfs` rather than of the code that reads
it. A namespace link reads `<kind>:[<inode>]` where it reads at all. Where the caller may not look
through it, kernels differ in how they refuse: some fail the `readlink` with `EACCES`, and some
let the call succeed and return a name of zero bytes. This host does the second —
`readlink("/proc/1/ns/pid", …) = 0` under `strace`, against 16 bytes for `/proc/self/ns/pid`.
`namespace_inode` handled `EACCES` as a refusal and folded the empty answer into the arm for "a
link in a shape this shell does not model", which is `Value::Null`. Null is an absence, so the
group composed to `empty`, so `follow namespace` found no members and reported that the relation
was not there.

`crates/ono-cli/tests/spatial_contracts.rs::should_serve_every_relation_it_declares_and_declare_every_relation_it_serves`
failed on this host for exactly that reason: `namespace` is declared in the relation registry, and
§41.2 makes a declared relation a name the shell must know.

## Decision

**A namespace link the kernel will not name is reported as the provider's own permission error,
not as null.** Concretely, in `linux.procfs`:

- `readlink` returning `<kind>:[<inode>]` — the inode, as before;
- `readlink` returning an empty name — `ono.error/1` with `io.permission_denied`, because a
  namespace link always carries a name where it carries anything, so zero bytes is the kernel
  declining to disclose (§35.1);
- `readlink` failing with `EACCES` — the same refusal, as before;
- `readlink` failing with `ENOENT` — null, because a kernel with no namespace to name has none,
  and that is a genuine absence;
- any other shape, and any other failure — null, unchanged: not knowing is not the same as being
  refused, and neither is an inode this provider may invent.

The distinction between the second and the fourth case is the whole decision: **a read that
returned nothing is `empty`; a read the kernel refused is `permission_denied`.** Nothing else
changes. The bridge already carries a field's error to the group it builds, `follow` already maps
a `permission_denied` group to `spatial.permission_denied`, and both keep doing so.

Consequently `spatial.no_relation` is left where §40 wants it: a relation name this kind of place
does not have. It is no longer reachable for `namespace` on a process, because the group is no
longer empty — it is refused, and says so.

The contract text moves with the behaviour: `docs/contracts/schemas/process.v1.yaml` and
`process-detail.v1.yaml` said "Null when the link is not readable", and now say null on a kernel
that has no namespace to name and an error value when the kernel will not name the one it has.
`docs/reference/schemas.md` is regenerated from them.

## Consequences

- `look` on a place whose namespace is withheld prints
  `namespaces  permission denied — /proc/1/ns/pid: the kernel named no pid namespace for this
  process`, which is §35.2's own example applied to a second exit; it already read that way for
  `files` and `sockets`.
- `follow namespace` on such a place answers `spatial.permission_denied` (`Ono-Sendai-E1008`)
  carrying the provider's sentence, rather than `spatial.no_relation`.
- Spatial identity is unchanged. §10.2's `pid_namespace` component reads the field through
  `Value::as_int`, which fails for an error exactly as it failed for a null, so the component
  stays `unknown` and no process changes identity. `ono-spatial-core`'s
  `should_record_an_unreadable_pid_namespace_as_unknown_rather_than_as_the_root` still holds.
- The two schemas now admit an error value in `pid_namespace`, as they already did in
  `open_files` and `sockets`. Anything reading the field must go on treating it as unknown rather
  than as a number.
- Whether the refusal is observable is a property of the host, so the test that observes it
  decides at runtime rather than being ignored (v0.4.1 §38.1): it skips as `missing_privilege`
  where the run may read `/proc/1/ns/pid`, and the skip is declared in
  `docs/contracts/hardening/expected_test_skips.yaml`.
- Encoded by
  `crates/ono-cli/tests/spatial_contracts.rs::should_report_an_exit_the_kernel_refuses_as_denied_rather_than_as_empty`
  (the group state and the `follow` refusal) and by
  `crates/ono-cli/tests/spatial_contracts.rs::should_serve_every_relation_it_declares_and_declare_every_relation_it_serves`,
  which was red on this host before the change.

## Alternatives considered

- **Fix it in the group builder: treat an empty `namespaces` group on a process as denied.** The
  bridge would then be guessing about a field it did not read. Only the provider knows why the
  namespace is missing, and a layer that invents the reason is the fabrication §35.3 forbids —
  the same defect one level up.
- **Report the empty readlink as `unknown` rather than `permission_denied`.** §42.4 permits
  either, and `unknown` is defensible for a shape nobody modelled. It is wrong here because the
  cause is known: §35.1 asks whether the user could legitimately query the information, and this
  user could not. `unknown` would also send `follow` down the §34.3 path that says "`--resolve`
  pays for it", offering a way to pay for something no payment unlocks.
- **Probe `/proc/<pid>/ns/` for readability and derive the state from that.** A second syscall to
  learn what the first already answered, and one that can disagree with it between the two calls.
  The empty name is the kernel's own refusal and is enough.
- **Leave the provider alone and widen the failing test's tolerance.** That is weakening a test to
  get a green result (AGENTS.md §7, §14). The test states the contract correctly; the shell did
  not meet it.
