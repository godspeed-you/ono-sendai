# ADR-1010: An operator catalog names the authorities its artifact servers are verified against

- Status: accepted
- Date: 2026-11-10
- Spec refs: K11A §2.5, §6.2, §6.3, §11, §21.2; ADR-0606, ADR-0607
- Issues: #153
- Decided by: agent (autonomous)

## Context

ADR-0607 fetches a catalog's network artifacts with `ureq` over rustls against the Mozilla root
set `webpki-roots` embeds, and rejected the system certificate store because it would make the
trust of a fetch depend on the machine's configuration. An enterprise artifact server presents a
certificate its own private authority issued, so the fetch is refused — and the operator K11A
§2.5 and §21.2 describe, who runs an internal mirror and places an operator catalog under the
configuration directory (`<config>/kuang/catalogs/*.yaml`, `CatalogVerification::Operator`),
had no path but a system package or a local copy.

Two answers were open: the platform verifier, or an operator-configured authority.

## Decision

An operator catalog may declare `catalog.certificate_authorities`: a list of PEM files, each
absolute or relative to the catalog file's directory. When the list is not empty, that
catalog's HTTPS artifacts are verified against **exactly those authorities**, in place of the
public roots; when it is empty, nothing changes and ADR-0607's root set applies.

- Only an operator catalog may name authorities. The built-in catalog and a catalog of unknown
  origin that name one are refused as `package.invalid`, as a plain `http://` artifact is refused
  outside an operator catalog (K11A §6.3).
- A named file that cannot be read, is not PEM, or holds no certificate refuses the fetch with
  `plugin.catalog_unavailable`, naming the file. Nothing is fetched and nothing is installed:
  falling back to the public roots would trust a server the operator did not name.
- The scope is the catalog that names them. Another catalog's fetches, the remote transport and
  everything else keep their own trust.
- The digest the catalog vouches for and the package signature are checked exactly as before;
  the authority decides only which server the bytes may come from.

## Consequences

- The operator's trust is stated where the operator already states the catalog, in one file
  they own, and it is the same on every host the file is deployed to — the property ADR-0607
  chose the webpki roots for.
- "In place of" rather than "in addition to": `webpki-roots` carries trust anchors, not
  certificates, so `ureq`'s root set cannot be the union without a second TLS stack, and an
  internal mirror is served by the internal authority anyway. A catalog whose artifacts live on
  public and private servers at once is two catalogs.
- `docs/contracts/kuang/catalog.v1.yaml` gains the field. Proven by
  `crates/ono-kuang-protocol/tests/catalog.rs::should_let_only_an_operator_catalog_name_certificate_authorities`
  and four tests in `crates/ono-cli/tests/acquisition.rs` against a loopback HTTPS server a
  private authority issued: refused without the authority, fetched and installed with it,
  refused when a different authority is named, and refused naming the file when it cannot be
  read.

## Alternatives considered

- **The platform verifier** (`rustls-platform-verifier`). Makes every catalog's trust depend on
  the machine's certificate store, which ADR-0607 rejected for good reason, and widens the trust
  of the public catalog on every host whose store an administrator extended for other purposes.
- **A shell configuration key** (`plugins.extra_roots`). User-scope configuration would let a
  user's setting widen the trust of the operator's catalogs, and it would apply to every catalog
  at once rather than to the server the authority belongs to.
- **Union with the public roots.** Not expressible without converting trust anchors back into
  certificates or carrying a second TLS configuration; see *Consequences*.
