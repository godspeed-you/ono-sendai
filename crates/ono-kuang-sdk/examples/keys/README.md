# A demonstration signing key — worth nothing, and deliberately so

`dev.example.key` is the Ed25519 signing key the SDK's example package
(`../adapter-package/dev.example.users`) is signed with. It is committed to this repository, in
plain text, on purpose: the example exists to be read, and a signed example nobody can re-sign
teaches half the story.

**It is therefore a public key pair with no secrecy value whatever.** It signs for the reserved
example namespace `dev.example`, which no real publisher may claim, and no trust store in this
project enrols it. A package signed with it verifies (`signature: valid`) and is trusted by
nobody (`trust: unknown`) — which is exactly what spec §31.36 says a signature from an unknown
key means.

A real publisher generates their own and keeps it somewhere this file is not:

```sh
kuang-sign keygen --out ~/.config/ono/kuang/publishing.key
kuang-sign sign ./my-package --key ~/.config/ono/kuang/publishing.key
```

`keygen` prints the public half on stdout. That is the line that goes into a trust store
(`/etc/ono/kuang/trust.yaml` or `~/.config/ono/kuang/trust.yaml`, ADR-0312).

To re-sign the example after editing it:

```sh
cargo run -p ono-kuang-sdk --bin kuang-sign -- \
  sign crates/ono-kuang-sdk/examples/adapter-package/dev.example.users \
  --key crates/ono-kuang-sdk/examples/keys/dev.example.key
```

`crates/ono-kuang-sdk/tests/example_signature.rs` fails if you forget.
