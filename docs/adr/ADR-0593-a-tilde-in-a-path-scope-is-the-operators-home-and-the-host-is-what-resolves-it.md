# ADR-0593: A tilde in a path scope is the operator's home, and the host is what resolves it

- Status: accepted
- Date: 2026-09-07
- Spec refs: §31.16, §31.18, §31.19, §31.20, §31.80; v0.4.1 §16.1, §2.3;
  `docs/contracts/kuang/capabilities.v1.yaml` (`scope_types.path-glob`, `filesystem.read`);
  `docs/architecture/external-system-provider.md` §7.4, §27.1, §27.3; ADR-0015 (T11, T14),
  ADR-0022 §3, ADR-0570
- Decided by: agent (autonomous)

## Context

The Kubernetes provider's manifest declares `filesystem.read` scoped to `~/.kube/config` and
`~/.kube/*.yaml` — the conventional place a kubeconfig lives, and exactly the path-scoped,
brokered access §27.3 of the provider contract asks for instead of a blanket grant. Through a real
host the declaration reached nothing:

- the supervisor starts a native instance with `HOME` set to its sandbox working directory
  (`sandbox.rs`, v0.4.1 §16.1's environment sanitisation), so a package that expanded `~` itself
  named `<sandbox>/.kube/config`, a file that does not exist;
- the broker matched a granted `path-glob` against the *canonicalised absolute* path the package
  asked for (ADR-0015 T14), so a pattern beginning with a literal `~` matched nothing a package
  could ask for, whichever of the two the package sent.

Every operator therefore had to widen the grant by hand with an absolute path, and the provider's
own board recorded that the manifest's declared scope was "ineffective in a real sandbox". The
finding is generic: any package that reads conventional configuration from the operator's home
meets it, and the sandbox's `HOME` is correct — a package must not inherit the operator's
environment (§31.20, §31.80) — so the fix cannot be to leak the home into the sandbox.

## Decision

**A leading `~` in a `path-glob` scope entry, and in a path a package hands to a filesystem or
program host call, means the operator's home directory as the host knows it. The host resolves it
before checking and before use; the package never expands it and never learns it from the
environment.**

Precisely:

1. `Policy` carries the operator's home: `HOME` of the shell's own process, canonicalised where it
   exists, captured when the policy is built (`Policy::deny_all`), overridable with
   `Policy::with_home` for a host whose operator is not its process — the test host exposes it as
   `TestHost::home`.
2. A scope entry that starts with `~/` (or is exactly `~`) is resolved against that home when it
   is matched, never when it is written: the manifest, the stored policy and `get capability` keep
   the `~` the operator granted, and the resolution happens at the check. A host with no home
   resolves `~` to nothing, and a pattern that still begins with `~` is dropped from the match set
   rather than matched as a literal directory name — a scope that cannot be resolved denies.
3. A requested path that starts with `~/` is resolved the same way, then canonicalised, then
   checked (ADR-0015 T14, unchanged): `..` components and symlinks are resolved *before* the glob
   sees the path, so a traversal out of a granted directory and a link pointing out of one both
   land outside the scope and are refused. `filesystem.read` and the `programs` scope of
   `process.exec` are the two call sites; a future filesystem call takes the same route.
4. Only a leading `~`. `~user` is left as written, because the host does not speak for other
   users; a `~` anywhere else is a character in a name.

Nothing about the sandbox changes: the instance's `HOME` is still its working directory, the
environment is still four variables, and the package learns the operator's home only as the
directory of a file it was granted — which it could read from the file's own path before.

## Consequences

The Kubernetes provider's manifest is effective as written. `load plugin
io.github.godspeed-you.kubernetes --grant filesystem.read` grants `~/.kube/config` and
`~/.kube/*.yaml`, and `get k8s-cluster --context prod` reads the operator's kubeconfig with no
`--kubeconfig` and no widened scope — the default configuration works through a real host, which
is what §7.4 of the provider contract means by environment-derived configuration.

The security properties are the ones ADR-0015 already promised, now holding for `~`:

- the declared file resolves and is readable
  (`should_resolve_a_tilde_in_a_declared_scope_and_in_a_request_against_the_operators_home`);
- an unrelated file in the same home stays denied, and the denial is audited
  (`should_keep_the_rest_of_the_operators_home_denied_under_a_tilde_scope`);
- `..` cannot escape the scope (`should_refuse_a_traversal_out_of_a_tilde_scope`);
- a symlink under the scope pointing outside it is refused, not followed
  (`should_not_follow_a_symlink_out_of_a_tilde_scope`);
- a host with no home matches nothing
  (`should_match_nothing_for_a_tilde_scope_when_the_host_has_no_home`).

All five drive the example package's `read-file` command through the test host, in
`crates/ono-kuang-sdk/tests/conformance.rs`. The package receives only the bytes of the paths it
was granted; it receives no descriptor and no listing.

What changes for a package author: send `~/…` through as written. Expanding it in the package
against the sandbox's `HOME` was always wrong, and now it is also unnecessary.

## Alternatives considered

**Forward the operator's `HOME` into the package's environment.** Rejected: §31.20 and §31.80
keep the operator's environment out of the sandbox, and a package that knows the home directory
still has no way to read it except through the broker — so the leak would buy nothing the broker
does not already provide, at the cost of a variable the sanitisation exists to withhold.

**A host call that returns the operator's home for the package to compose paths with.** Rejected:
it moves the resolution to the wrong side of the boundary. The check must run against the value
the operation will use, and a path the package composed from a directory it was told is a path
the package composed. The host resolving `~` itself is one fewer thing a package can get wrong.

**An explicit host-side token, e.g. `$OPERATOR_HOME/.kube/config`.** Rejected in favour of `~`
because the manifests already say `~`, every kubeconfig-consuming tool spells it that way, and a
second spelling for the same meaning is one more thing to document. The semantics — resolved by
the host, only at the front, only the operator's own home — are the token's; the spelling is the
conventional one.

**Resolve `~` at grant time and store the absolute path in the policy.** Rejected: an `always`
grant written to `policy.yaml` would then carry one machine's home directory, and a policy file
copied to another account would grant the previous account's files. Resolving at the check keeps
the stored grant portable and its meaning tied to whoever the host is serving.
