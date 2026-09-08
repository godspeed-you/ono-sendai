# ADR-0607: A network artifact is a digest-pinned archive, fetched to staging

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.2 §31.9, §31.36, §38; v0.4.1 §47;
  `docs/specs/kuang11/kuang11-plugin-package-acquisition-system-distribution.md` (K11A) §6, §7,
  §16.3, §20, §23 invariants 9, 11, 12, §25.1; K11P §11, §13.3, §30; ADR-0311, ADR-0601, ADR-0606
- Decided by: agent (autonomous)

## Context

ADR-0601 §3 left the network out on purpose: it had no archive format, no fetcher and no rule for
what a downloaded file may become. K11A §6 states the rules — resolve metadata without running
anything, fetch into staging, verify the digest, then the structure, then the signature, judge
trust separately, and only then commit — and §7 leaves the archive format to the implementation
as long as the existing package-directory representation is what arrives. This ADR chooses both.

## Decision

### 1. `.kuang` is a plain tar of the package directory

A network artifact is an uncompressed ustar archive of the package's files under relative paths —
the artifact files `kuang-sign` signs, plus `signature.yaml` when the package is signed — and
nothing else: no owner, no device node, no link, no absolute path, no `..`. `kuang-sign pack
<directory> --out <file.kuang>` writes one deterministically, and `kuang-sign digest <directory>`
prints the content digest the catalog release states. Unpacking refuses any entry that is not a
regular file or directory under a relative path inside the target, before writing it: a payload
chooses no path on this machine. No compression, because the payloads are a binary and a few
documents, and one less decoder is one less parser between the network and the disk.

### 2. Fetch, stage, verify, then cache — in that order, and nothing runs before the end

`kuang_acquire::acquire_network` fetches the URL with `ureq` over rustls into memory (a 256 MiB
ceiling), unpacks it under `<cache>/.staging/<id>-<version>-<nonce>/`, computes the content digest
of what it unpacked and compares it with the digest the catalog vouches for. A mismatch is
`package.integrity_failed` (K11003), and the staging directory is removed with nothing else
touched (§25.1 item 3, §25.1 item 5). A payload that hashes right is read as a package, must
identify as the id and version the catalog named, and is moved to `<cache>/<id>/<version>/` —
whereupon it is a *local copy* like any other, and ADR-0602's transaction verifies its signature,
judges its publisher and asks for consent exactly as for a directory. Nothing under staging or the
cache is ever a plugin home; execution begins from the installed copy only (§16.3, invariants 11
and 12).

**The cache is not trusted.** A cached copy is a local candidate, re-hashed on every resolution
and compared with the catalog's digest under the default preference; an altered cache is a
`plugin.source_conflict`, never a silent hit (§6.4, §25.1 item 6).

### 3. HTTPS, and plain HTTP only where an operator said so

A catalog release whose artifact is a URL must carry a digest, or the catalog document is refused
at parse. Plain `http://` is refused at parse unless the catalog is an operator catalog declaring
`insecure_http: true`; the built-in catalog can never declare it; and even then the digest and the
signature are checked as for HTTPS (K11A §6.3). The fetcher refuses a plain URL it was not told to
allow with `plugin.transport_refused` (E1606) — a second guard behind the parser's.

### 4. Offline stays offline

Nothing here is consulted when a system payload or a local copy answers: the default preference
of ADR-0606 §3 chooses those first, a `git:` artifact still resolves locally only, and a fetch
happens exactly when the network is the admitted source that has the release. A machine with no
network and a distribution package installs without a catalog refresh, and every integrity check
still runs (K11A §20).

## Consequences

- `ono-cli` gains `ureq` (rustls, no native TLS, no system certificate store beyond webpki roots)
  and `tar`; `ono-kuang-sdk` gains `tar` for `pack`. The binary grows by the HTTP client and the
  TLS roots, which the bootstrap catalog's network artifacts will need on a clean machine.
- The bootstrap catalog keeps its `git:` artifact until the Kubernetes provider publishes a
  `.kuang` beside its `.deb` and `.rpm`; then the entry gains the URL and the digest, and
  `install plugin kubernetes` fetches on a machine with neither a system package nor a copy.
- Encoded by `ono-cli/tests/acquisition.rs` — fetch, staging, digest mismatch, signature refusal,
  failed fetch, cache re-verification, plain HTTP — against a loopback HTTP server the suite runs,
  and `ono-kuang-protocol/tests/catalog.rs`.

## Alternatives considered

- **A compressed or signed container format of its own.** K11A §7 asks for no new format when
  the directory representation suffices; the signature already covers the files, and the digest
  covers the archive's content.
- **Streaming the archive straight into the plugin home.** Would execute-from-staging by another
  name; §16.3 forbids it.
- **A system certificate store.** Would make the trust of a fetch depend on the machine's
  configuration; webpki roots are the same on every host, and the digest and the signature decide
  what the bytes are anyway.
