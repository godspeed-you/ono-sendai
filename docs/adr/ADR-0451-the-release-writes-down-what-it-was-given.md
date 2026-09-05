# ADR-0451: The release writes down what it was given

- Status: accepted
- Date: 2026-09-02
- Spec refs: spec Appendix H (release input manifest), §43.2 (first-party scripts for critical
  logic), §44.1–§44.4 (pinned inputs), §46.1 (reproducible build definition), §47 (checksums,
  signatures and provenance), §57 (H10), §62.5 (provenance verification)
- Issues: #103 — consumed by #30 (baseline), #105/#106 (H11)
- Decided by: agent (autonomous)

## Context

ADR-0433 made the inputs immutable and ADR-0450 made them exact. Both are properties of the
repository, checked at the moment the gate runs. Neither travels with the artifact.

Appendix H asks for the thing that does travel:

> What exactly did we trust to produce these bytes?

Nothing answered it. A `.deb` attached to a GitHub release carried a version number and nothing
else — not the commit, not the image the binary was compiled in, not which `cargo-deb` laid it
out, not the hash of the dependency graph it was built against. A maintainer investigating that
package in two years would have had to reconstruct all of it from the workflow file *as it is
today*, which is exactly the file that will have changed.

Two neighbours are waiting on this. H11 builds every artifact twice and compares (§46.5), which
needs a statement of what "the same inputs" means; and H11's provenance (§47, §62.5) needs
something to attest to. Issue #30 — the frozen v0.4.1 baseline — wants today's action refs, image
refs and tool versions recorded as the starting point. **#30 has not landed**: there is no
baseline file in the repository, so this manifest derives its inputs from the working tree rather
than consuming one. When #30 lands, its baseline should be a captured manifest rather than a
second, hand-written list of the same facts.

## Decision

**`cargo xtask build-manifest` writes `dist/build-inputs.json`, the release workflow runs it
before it publishes anything, and the file is published beside the packages.**

### What it carries

Appendix H's list, in a stable shape, `ono.build-inputs.v1`:

```json
{
  "schema": "ono.build-inputs.v1",
  "source":    { "commit": "<40-hex>", "tag": "v0.4.1" | null, "version": "0.4.0" },
  "toolchain": { "file": "rust-toolchain.toml", "channel": "1.94",
                 "components": ["rustfmt", "clippy"], "profile": "minimal" },
  "lockfile":  { "path": "Cargo.lock", "sha256": "<64-hex>" },
  "containers": {
    "build":        [{ "file": "docker/Dockerfile",        "reference": "rust:…@sha256:…" }],
    "package_test": [{ "file": "scripts/package-check.sh", "reference": "fedora:latest@sha256:…" }]
  },
  "actions": [{ "file": ".github/workflows/release.yml",
                "uses": "actions/checkout@11d5960…", "version": "v4.4.0" }],
  "tools":   { "cargo-deb": "3.7.0", "cargo-generate-rpm": "0.21.0", "cross": "0.2.5",
               "cargo-deny": "0.20.2" },
  "source_date_epoch": "1788338112",
  "run": { "workflow": "release", "repository": "…", "id": "…", "attempt": "…",
           "ref": "refs/tags/v0.4.1", "runner": { "os": "Linux", "arch": "X64" } }
}
```

Every field is read, never typed: the commit and the epoch from git, the toolchain from
`rust-toolchain.toml`, the lockfile hash from `sha256sum`, the tool versions from
`[workspace.metadata.release-tools]` (ADR-0450), the run identity from the workflow's own
environment.

### It reads the same files the gate reads

The action commits and the image digests come from `supply_chain::action_references` and
`supply_chain::image_references` — the two functions `check_action_pins` and
`check_image_digests` are themselves written in terms of. This is the whole design, and the
reason those two scanners were refactored to expose their collectors rather than being read a
second time: **a manifest assembled by a second reading can disagree with the gate that approved
the commit, and then neither is evidence.** The exit condition of #103 — the manifest's contents
match the pin scanners — holds by construction rather than by a test that compares two lists.

### Unknown is null

A manifest generated on a developer machine has no workflow run, and usually no tag. It says
`null`, which spec §35.3 requires and which a consumer can act on. It does not fall back to a
plausible value, and it does not omit the key: a missing key is a question nobody asked, and a
fabricated one is worse than both.

### It is first-party, and it runs before the artifacts exist

Spec §43.2 asks that critical release logic live in repository-owned code with Actions
orchestrating it. `build-manifest` is a task in `xtask`, 250 lines a reviewer can read, invoked by
a workflow step. The `inputs` job runs in parallel with `package`, checks out with full history
so the tag is visible to `git describe`, writes the manifest, prints it into the log, and uploads
it; `publish` downloads it beside the packages and attaches it to the release.

It runs *before* anything is published on purpose. This is a record of what the build was given,
not a summary of what it produced. What it produced — checksums, signatures, attestations — is
§47 and belongs to H11, and the manifest is the input those take as given.

### What H11 is promised

- the schema name changes when a field's meaning changes; new fields may appear under the same
  name, so read by key and ignore what you do not know;
- `null` means unknown, and only that;
- one manifest per release run, deterministic given the commit and the run environment: the same
  commit built twice in the same run yields identical bytes, which is what §46.5's comparison
  needs to be able to assume about its own inputs;
- `source.commit`, `lockfile.sha256` and `source_date_epoch` are the three fields a rebuild has to
  reproduce exactly; the containers, actions and tools are the environment it has to reproduce
  the build *in*.

## Consequences

Easy: `dist/build-inputs.json` answers Appendix H's question from inside the release. #30 can
capture one instead of writing a list. H11 has a defined input rather than a convention.

Hard: **the manifest describes the repository, not the machine.** It records the digest
`scripts/package-check.sh` will pull, not the digest the runner actually pulled; it records the
`cargo-deb` version the workflow installs, not the one that ran. Those are the same thing while
the pins hold, and the pins are what the gate enforces — but the manifest is a statement of
intent, and an attestation of what physically executed is §47's job, not this file's. Saying so
here is better than a reader assuming otherwise.

Also accepted: `actions` lists every third-party action in the repository, not only the release
workflow's, each tagged with the file it appears in. The extra entries are what verified the
commit that was released, which is part of the same answer, and filtering them out would make the
manifest disagree with the scanner it is derived from.

Encoded by `xtask/tests/provenance.rs`:

- `should_emit_a_build_input_manifest_carrying_every_field_appendix_h_requires` — drives the real
  `xtask build-manifest`, requires every Appendix H key to be present, requires the four fields
  that can never be unknown to be non-empty, and requires every recorded image to carry a digest
  and every recorded action a commit;
- `should_bind_the_build_input_manifest_to_the_release_it_describes` — the commit is `HEAD`, the
  lockfile hash is `sha256sum Cargo.lock`, the tag and run identity are the run's own, a manifest
  generated outside a release run says `null` rather than inventing one, and the release workflow
  is what emits it.

## Alternatives considered

**Write it as a shell script.** §43.2 says "repository-owned scripts where practical", and a
shell script that parses Dockerfiles and workflow YAML for pinned references is neither small nor
auditable — it would be a second, worse implementation of what `supply_chain.rs` already does
correctly and is tested doing. `xtask` is first-party in every sense §43.2 means; what the rule
forbids is an *external action* being the only implementation.

**Emit one manifest per architecture, inside the `package` jobs.** The inputs do not differ by
architecture — the same commit, lockfile, images, tools and tag — so two files would be two
copies of one fact, and merging them into the release assets would need a name that says which is
which for no reason. The runner identity that *does* differ belongs to §47's attestation of each
build, not to the record of what they were both given.

**Record resolved facts from the runner: the digest actually pulled, `cargo deb --version` as it
ran.** Strictly more truthful about that one run, and it makes the manifest a *result* rather than
an *input*. §46.5 needs to compare two builds against one statement of inputs; if the statement is
itself an output of the build, there is nothing left to compare against. The resolved facts belong
in provenance (§47).

**Put the manifest in the provenance attestation and nowhere else.** Provenance is H11 and does
not exist yet, and Appendix H says the manifest "MAY be embedded in or referenced by provenance" —
may, not must. A file that stands on its own can be published today and referenced later.
