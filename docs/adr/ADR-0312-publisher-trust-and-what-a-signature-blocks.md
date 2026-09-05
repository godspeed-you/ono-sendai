# ADR-0312: Publisher trust, and what a signature blocks

- Status: accepted
- Date: 2026-08-29
- Spec refs: §31.9, §31.36, §31.79; ADR-0311, ADR-0015/ADR-0245 rule 4;
  `docs/contracts/schemas/{verification-result,plugin,plugin-package}.v1.yaml`,
  `docs/contracts/kuang/lifecycle.v1.yaml`
- Decided by: agent (autonomous)

## Context

ADR-0311 decides how a package is signed and how a signature is checked. Two questions are left,
and spec §31.36 answers neither: **whose keys are accepted**, and **what a bad answer does**.

§31.36 is explicit that trust is a separate question from the signature — "a valid signature from
an unknown key is `unknown`, never `trusted`" — and equally explicit that KUANG/11 "MUST remain
safe enough to run unsigned local development packages in a visibly untrusted, capability-limited
mode". So the policy cannot be "signed or refused". It has to distinguish *no claim* from *a
false claim*.

The schemas already carry the vocabulary this has to land in, and the two of them do not agree
on it: `ono.verification-result/1.trust` is
`[user-trusted, system-trusted, untrusted, unknown]`, while `ono.plugin/1.trust` and
`ono.plugin-package/1.trust` are `[signed, verified, local, unknown, untrusted]`. Both are
level-2 contracts; neither is wrong. They answer different questions — one says where the
decision to trust the key came from, the other says what is known about the artifact — so this
ADR maps between them rather than changing either.

## Decision

### 1. Two trust stores, and the store is where trust comes from

A key is trusted because it is written in a trust store, and nowhere else. There are two:

| Store | Path | Answers |
|---|---|---|
| system | `/etc/ono/kuang/trust.yaml`, or `$ONO_KUANG_SYSTEM_TRUST` | `system-trusted` |
| user | `<config dir>/kuang/trust.yaml` (ADR-0010) | `user-trusted` |

The system store belongs to whoever administers the machine; the user store belongs to the
operator. Both are read; the system store is consulted first, so a machine-wide decision is not
silently narrowed by a user file. Neither store is ever written by installing a package — a
package cannot make itself trusted, which is the whole point of keeping the store outside the
plugin home (the same reason §31.19 keeps capability policy there).

The format is `kuang-trust/1`:

```yaml
format: kuang-trust/1
keys:
  - publisher: dev.example
    key: ed25519:<64 hex characters>
    trust: trusted        # trusted | revoked
    comment: the key the SDK example package is signed with
```

An entry matches a signature when **both** the key and the publisher match. A key that signs for
a publisher it is not enrolled for is not trusted for it, which is what stops one accepted key
from vouching for every namespace.

`trust: revoked` is a positive statement, not an absence: it is how an operator says "this key
was trusted and no longer is", and it produces `untrusted`, which blocks. A key that is simply
not in either store produces `unknown`, which does not.

### 2. The mapping, in one table

| signature | key in a store | `verification-result.trust` | `plugin.trust` | blocks? |
|---|---|---|---|---|
| absent | — | `unknown` | `local` | no |
| valid | not present | `unknown` | `signed` | no |
| valid | system, `trusted` | `system-trusted` | `verified` | no |
| valid | user, `trusted` | `user-trusted` | `verified` | no |
| valid | either, `revoked` | `untrusted` | `untrusted` | **yes** — K11005 |
| invalid | — | `unknown` | `untrusted` | **yes** — K11004 |

`transparency` stays `unknown`. No transparency log is configured and none is faked; spec
§31.36's own example prints `transparency unknown`.

### 3. What blocks

Two answers block, and they are the two that mean *a claim was made and it is false*:

- **`signature: invalid`** — the package carries a signature and it does not verify. K11004,
  `package.signature_invalid`.
- **`trust: untrusted`** — the signing key is revoked in a trust store. K11005,
  `publisher.untrusted`.

A blocking failure prevents installing and prevents loading, and never produces a prompt offering
to continue (ADR-0245 rule 4). `signature: absent` and `trust: unknown` are warnings that appear
in `verify plugin`'s `warnings` and in the trust field of every table the package appears in.

This makes `blocking` in `lifecycle.v1.yaml` too coarse to say what it meant: the `signature`
check is `blocking: false` and yet `install` lists `package.signature_invalid` among its errors.
The field is therefore replaced by **`blocking_values`** — the list of values of that check which
block — on all seven checks. `blocking_values: []` is a check that never blocks; the previous
`blocking: true` on `integrity` becomes `blocking_values: [invalid]`, which is what the code
always did (an `unknown` integrity is a warning, not a refusal).

### 4. Verification is re-run at load, not only at install

`lifecycle.v1.yaml` already required this — "Integrity is re-verified at load, not only at
install: a file changed on disk afterwards must not load" — and nothing did it. `load plugin` now
runs the same verification `install plugin` runs and refuses on a blocking failure, before the
package's process is spawned. Integrity, signature and trust are therefore all checked against
what is on disk *now*, which is the only moment at which the check means anything.

### 5. The install plan says what the signature said

`install_plan`'s `signature` field was the constant `unsigned`. It now carries the state and the
publisher it attests to, which is the form spec §31.9's own example prints
(`signature      valid / dev.ono-labs`): `unsigned` when absent, `valid / <publisher>` when
valid. An `invalid` signature never reaches the plan, because verification precedes it.

### 6. One walk of the package directory, beside the format

ADR-0311 §4 put the walk of a package directory in `ono-cli`'s KUANG host, on the grounds that
the signature module should touch no filesystem. The module still does not: the walk is a
separate module, `ono_kuang_protocol::artifact`, and it moved there because the host is not its
only caller. The signing tool needs the same answer to "which files is this package made of",
and two walks that disagreed would be a signature that verifies on one side and not the other.
`ono-cli` is a binary crate and cannot be depended on, so the definition lives beside the format
it feeds and both sides call it.

That walk is now every file under the package directory except `signature.yaml` — including the
ones no manifest field names, which is a change to what `integrity` covered before. What a
manifest declares is what a package *contributes*, not what it *consists of*: an adapter pack's
fixtures, a data table a command answers from, a second binary the entry point execs. An
integrity hash that skipped them answered spec §31.36's "are these the exact bytes referenced?"
while most of the bytes went unlooked at. A symbolic link is recorded by its target rather than
followed, so a package's hash cannot come to depend on a file outside the package and a
repointed link cannot pass unnoticed.

## Consequences

- `Ono-Sendai-K11004` and `Ono-Sendai-K11005` are constructed and reachable. Both are provable
  from a shell: sign a package, change a byte, `verify plugin`.
- Unsigned packages behave exactly as before — installed, loadable, and visibly `local` /
  `absent` / `unknown` in every table. Every existing acceptance case that lays out an unsigned
  package by hand keeps its answers, which is the compatibility constraint §31.36 imposes.
- An operator who trusts a key gets `verified` and `user-trusted`; nothing else changes for them.
  Trust buys visibility, not capability: a `verified` package is granted no more than a `local`
  one, because §31.18's grants are explicit and independent of who signed.
- Re-verifying at load costs one hash of the package's files per load. The packages in question
  are a manifest, an entry binary and a handful of YAML files.
- Revocation is a local list, not a fetched CRL, and there is no expiry on a key. Both would need
  a source of truth this build does not have; `registry:` is not implemented (ADR-0311).

## Alternatives considered

- **One store, with a `system: true` flag per entry.** Fewer files, but an operator's own file
  could then claim system trust, which is exactly the boundary the two paths exist to hold.
- **Trust by publisher name alone, key learned on first use.** Trust-on-first-use is the right
  shape for host keys, where the alternative is nothing; here the alternative is an explicit
  enrolment, and TOFU would make the first install the unprotected one.
- **Refusing unsigned packages by default.** Directly against §31.36's MUST, and it would make
  every `path:` development package unusable — the source scheme this build actually supports.
- **Making `trust: unknown` block under a setting.** A setting nobody sets is a check nobody
  runs; and an operator who wants that today gets it by revoking nothing and enrolling the keys
  they accept, then reading `trust` in `get plugin`. Left undone rather than half-done.
