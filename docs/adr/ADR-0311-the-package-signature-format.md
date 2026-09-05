# ADR-0311: The package signature format

- Status: accepted
- Date: 2026-08-29
- Spec refs: §31.9, §31.36, §31.79; `docs/contracts/kuang/lifecycle.v1.yaml` → `verification`
- Decided by: agent (autonomous)

## Context

Spec §31.36 keeps four questions apart — integrity ("are these the exact bytes referenced?"),
signature ("did a key sign these bytes?"), publisher trust ("do I trust that key?") and runtime
isolation ("what can the code do anyway?") — and requires `verify plugin` to answer all four.
Until now this build answered three. `signature` was the constant string `absent`, `publisher`
and `key` were always null, `trust` was always `unknown`, and `Ono-Sendai-K11004`
(`package.signature_invalid`) was defined in both error registries and constructed by no code
path. A code no path constructs is a promise nobody keeps, and the `install` transition of
`lifecycle.v1.yaml` lists `package.signature_invalid` among the errors it can raise.

The specification fixes none of the mechanics: not the algorithm, not the file, not what the
signature covers, not where trusted keys are kept. Every one of those is this ADR's to decide.

## Decision

### 1. Ed25519 detached signatures, in a `signature.yaml` beside the manifest

A signed package carries one extra file, `signature.yaml`, in its package directory. It is a
`kuang-signature/1` document:

```yaml
format: kuang-signature/1
algorithm: ed25519
key: ed25519:<64 hex characters>
signed:
  package: dev.example.users
  version: 0.1.0
  publisher: dev.example
  files:
    - path: adapters.yaml
      sha256: <64 hex characters>
    - path: manifest.yaml
      sha256: <64 hex characters>
signature: <128 hex characters>
```

**Ed25519**, because it is the smallest complete answer: fixed 32-byte keys and 64-byte
signatures, no parameter negotiation to get wrong, no key-size policy to age, and a single pure
Rust implementation (`ed25519-dalek`) with no C toolchain. It is also what the ecosystem this
product lives in already uses for the same job — SSH host keys, `minisign`, Sigstore's default.
`algorithm:` is read and checked rather than assumed, so a second algorithm is additive; anything
else is refused as K11004 rather than ignored.

**Hex, not base64**, everywhere a key or a signature is spelled. `sha256:<hex>` is already how
this product writes an `integrity` field, and one encoding in a security-relevant file is one
fewer thing for a reader to get wrong. Spec §31.36's example prints `key ed25519:AB12...`, which
this form satisfies.

**Detached, and beside the manifest**, not inside it: the manifest is one of the files the
signature covers, so it cannot also contain the signature. `signature.yaml` is not itself part of
the signed file set, so a package can be signed without rewriting anything it already had.

### 2. What is signed is a canonical description, not the file

The bytes a key signs are not `signature.yaml` and not a YAML re-serialization of anything. They
are a line-oriented canonical form built from the fields alone:

```text
kuang-signature/1
package <id>
version <version>
publisher <publisher>
file <sha256 hex> <path>
...
```

with the `file` lines sorted by path and every line terminated by `\n`. A verifier in any
language can reproduce these bytes from the parsed fields; nothing depends on a serializer's
quoting, key order or line wrapping, so a signature survives the file being reformatted. Paths
that are empty or carry a control character are refused when the description is built, because
they could otherwise forge a line of the canonical form; a repeated path is refused because it
would make the commitment ambiguous.

### 3. The signature covers exactly the artifact, both ways

`PackageSignature::check` passes only when all of these hold:

- the signature bytes verify under the key the document names, over the canonical description
  (`verify_strict`, so the malleable and small-order-point edge cases are refused);
- `signed.package`, `signed.version` and `signed.publisher` equal the manifest's;
- every file the artifact is made of appears in `signed.files` with the same digest;
- every file in `signed.files` is present in the artifact.

The last two are separate checks on purpose. A signature that covers a subset would let an
attacker add a file; a signature that covers a superset means the artifact is not the one that
was signed. Both are K11004, and the message names the file that broke it.

The set of files "the artifact is made of" is the existing `artifact_files(manifest)` — the
manifest plus the runtime entry plus every declared contribution — which is already what
`integrity_of` hashes. Integrity and signature therefore cover the same bytes and answer
different questions, which is exactly §31.36's distinction.

### 4. `ono-kuang-protocol` owns the format and touches no filesystem

The module is pure over `(path, sha256)` pairs the caller supplies. The walk of a package
directory stays in `ono-cli`'s KUANG host, which already knows what a package artifact consists
of. This keeps the wire contract testable without a directory, and keeps one definition of
"what a package is made of".

## Consequences

- One new third-party dependency, `ed25519-dalek 3` (with `curve25519-dalek`, `ed25519`,
  `subtle`, `zeroize`). It is the workspace's only asymmetric primitive; `sha2` was already
  present and is shared, since `ed25519-dalek 3` tracks `sha2 0.11` as this workspace does.
- Key generation reads `/dev/urandom` directly rather than adding `rand`/`getrandom` as a
  direct dependency. The supported platforms are `linux-amd64` and `linux-arm64`; a system
  without `/dev/urandom` gets a structured error rather than a weak key.
- `SecretKey` never appears in a `Debug` output as anything but its public half.
- A second algorithm is additive: `algorithm:` is a checked field, and a document naming one
  this build does not implement is K11004 with the algorithm named, never a silent pass.
- What this does *not* answer: publisher trust (ADR-0312), transparency logs (spec §31.36's
  `transparency` stays `unknown`, honestly, because no log is configured), and revocation of a
  key that was trusted and later was not, beyond removing it from the trust store.

## Alternatives considered

- **OpenPGP detached signatures.** The format every distribution already uses, and the reason to
  reject it: a full OpenPGP implementation is a large dependency with a large parser attack
  surface, aimed at a web of trust this product does not have. §31.36's trust model is an
  operator's own store, which needs none of it.
- **Sigstore / transparency-log-first.** The right long-term answer for a public registry, and
  the wrong one for a build whose only core source scheme is `path:`. The `transparency` field
  stays `unknown` and this ADR does not close it.
- **Signing the archive rather than the file list.** `path:` sources are unpacked directories;
  there is no archive to sign. A per-file commitment also survives repacking and says which file
  broke when one does.
- **A signature block inside `manifest.yaml`.** Self-referential: the manifest is the first thing
  a signature must cover.
