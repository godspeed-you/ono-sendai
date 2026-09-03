# ADR-0435: The legacy host key is copied once, and then left alone

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §8.1, §8.2, §8.3, §8.4, §4.5, §56.3; ADR-0010, ADR-0353, ADR-0434
- Decided by: agent (autonomous)

## Context

v0.4.1 §8.1 gives the direct-link client a persistent identity and names its file
`~/.config/ono/link_identity.pem`. A machine that has already run `ono --agent --listen` has a
`host_key.pem` in the same directory, holding an identity that peers have already pinned, and
§8.2 forbids quietly generating a second one beside it. It also asks the implementation to say
which shape it chose:

> An ADR MUST document whether the implementation keeps one canonical file plus a compatibility
> symlink/read fallback or performs a one-time copy.

The three candidate shapes are not equivalent. A symlink makes the two paths one file, so the two
process roles can never disagree and can never be separated again. A read fallback keeps two
paths and no file, resolving the identity afresh on every start. A copy makes two files that are
equal once and independent afterwards.

## Decision

**A one-time copy, and the legacy file is never written to or deleted again.**

`ono_cli::trust::link_identity(directory)` walks §8.2's ladder in the order §8.2 writes it:
`link_identity.pem` if it exists; otherwise `host_key.pem` if it exists *and parses*, copied
across; otherwise a fresh identity generated into `link_identity.pem`. The copy goes through a
temporary file in the same directory, opened `create_new` with mode `0600`, `fsync`ed and
renamed, so §4.5's "the old file preserved until the new file has been fsynced successfully" is
satisfied by construction and the canonical path is never a half-written private key.

**Rule 2's `and parses` is load-bearing and is implemented literally.** A `host_key.pem` that is
not an identity is not an identity to inherit. Refusing there would leave a shell unable to make
a direct link because of a stray file that no current code path writes, and §8.2 rule 3 already
says what to do instead: generate. The unreadable legacy file is still not deleted (rule 4), so
nothing about it is lost and an operator can see what happened.

**Rule 5 — "both files MUST NOT diverge silently if both are explicitly configured for the same
process role" — is met by having one file per role and no default that names both.** The listening
agent's identity comes from `--host-key` when the flag is given and from the ladder above when it
is not, so the two paths are never both in play for one role at one time; the explicit flag
replaces the default rather than sitting beside it. There is deliberately no `--link-identity`
flag in this increment: a second override on the same object is the exact configuration §8.2 rule
5 warns about, and nothing needs one yet.

## Consequences

Easy: after the copy, `ono --print-peer-key` and `ono --agent --print-host-key` print the same
fingerprint on a machine that already had a host key, which is what §8.5 requires, and they keep
printing it after the legacy file is eventually removed by hand. Every peer that pinned the old
host keeps working, because the identity did not change — only which file holds it.

Hard: the private key now exists in two files on such a machine, both `0600`. That is a real
widening of the blast radius of a filesystem mistake, and it is why §8.3's permission refusal
(issue #34) applies to whichever file is actually opened rather than to the canonical name. An
operator who wants one file deletes `host_key.pem` after the migration; v0.4.1 will not do it for
them, because §8.2 rule 4 says so and because deleting a private key is not a thing a shell
should do on its own initiative.

Also hard: after the copy the two files can be edited apart. A symlink would have made that
impossible — and would also have made it impossible to give the listening agent and the client
separate identities later, which §9 and H2 will very plausibly want on a host that both listens
and links out. The copy keeps that option open; the divergence it permits is visible in two
fingerprints an operator can print.

Encoded by: `crates/ono-cli/tests/peer_identity.rs::should_reuse_a_legacy_host_key_rather_than_generate_a_second_unrelated_identity`,
`::should_prefer_an_existing_link_identity_over_the_legacy_file`,
`::should_generate_a_fresh_identity_when_the_legacy_file_does_not_parse`,
`::should_generate_one_identity_and_keep_it_across_calls`.

## Alternatives considered

**A symlink from `host_key.pem` to `link_identity.pem`.** One file, no divergence, no second copy
of the key. Rejected: it rewrites a file the operator created, it makes the listening identity and
the link identity the same object forever, and a symlink whose target is a private key is a
permission surface (`0777` on the link, whatever the target says) that §8.3 would then have to
reason about separately.

**A read fallback with no copy: try `link_identity.pem`, else read `host_key.pem` in place.** The
smallest change, and nothing is duplicated. Rejected because the resolution is then invisible: the
canonical file §8.1 names never appears, `--print-peer-key` reports a fingerprint whose source is
not on disk under the name the documentation gives, and every later feature that wants to write to
the identity file has to re-derive which file that is.

**Refuse to start when `host_key.pem` exists and ask the operator to migrate.** Explicit, and
wrong for §2.9 and §8.2: migration is specified as automatic and deterministic, and a security
default that stops the shell until a human runs a command is a security default that gets worked
around.
