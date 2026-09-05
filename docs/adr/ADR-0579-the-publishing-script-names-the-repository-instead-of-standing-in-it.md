# ADR-0579: The publishing script names the repository instead of standing in it

- Status: accepted
- Date: 2026-09-04
- Spec refs: v0.4.1 §49.1, §49.4, §62.5, §47.3; ADR-0529, ADR-0532
- Issues: none (found by the first real `v*` tag)
- Decided by: agent (autonomous)

## Context

The first release run of `v0.4.1` got further than anything before it and then stopped one step
short. Everything the tranche built to be provable was proven:

```
== SHA256SUMS            ok  every artifact hashes to what SHA256SUMS records
== SHA256SUMS.sigstore.json
                         ok  SHA256SUMS was signed by this project's release workflow on a version tag
== build-provenance.json ok  every digest appears in both SHA256SUMS and build-provenance.json
verify-release: green

== drafting v0.4.1
failed to run git: fatal: not a git repository
```

`gh` reads which repository it acts on from the git remote of the directory it runs in.
`publish-release.sh` `cd`s into the artifact directory so it can name assets by their bare
filenames, and in the release workflow that directory is `dist/` at the workspace root while the
checkout is at `repository/` — `actions/checkout` was given `path: repository` so that the
downloaded artifacts and the source tree do not share a directory. So the script published from a
place that is not a repository, and `gh` had nothing to go on.

Nothing was published: §49.4's order — verify, draft, upload, check the inventory by digest, and
only then make it visible — meant the failure happened before anything existed, and no draft, no
release and no half-populated asset list was left behind. The rule did exactly what it is for.

This is the class of defect only a real tag can find. `sign-release.sh` refuses without an OIDC
token, so the gate cannot reach the step after it, and `docs/ACCEPTANCE.md` §4.8.11 and §4.8.12
both say in their own text that the first `v*` tag is what proves them. It did — including this.

## Decision

**The repository is named, not inferred from where the script happens to stand.** Before it
changes directory, `publish-release.sh` resolves `GH_REPO` once:

1. from `GITHUB_REPOSITORY`, which Actions sets to `owner/repo`;
2. otherwise from the `origin` remote of the checkout the script itself belongs to — `$repo` is
   already computed from `BASH_SOURCE`, so this is the repository the script is *part of* rather
   than whichever one the caller is standing in;
3. and if neither answers, it refuses. Guessing which repository to write a release to is not a
   guess this script may make.

An explicit `GH_REPO` in the environment still wins, because a caller who names one has said
something the script has no business overriding.

## Consequences

Easy: the release publishes from anywhere — the workflow's `dist/`, a maintainer's `/tmp`, a
directory that has never been a repository. The third rule also turns the confusing failure into a
readable one for the case nobody has hit yet.

Hard: the release harness now has a step whose environment is checked by a stand-in rather than by
the thing itself, and a stand-in can only model what somebody thought of. The two tests here own
one property — that every `gh` call knows its repository — and say so; they do not model asset
storage, so the runs they drive stop at the inventory check with the script's own diagnostic. The
end-to-end proof is still the tag.

Encoded by `xtask/tests/provenance.rs::should_name_the_repository_it_publishes_to_when_it_runs_from_outside_a_checkout`
and `::should_take_the_repository_from_its_own_checkout_when_the_environment_names_none`.

## Alternatives considered

**Run the publishing step with `working-directory: repository` in the workflow.** It fixes the
workflow and leaves the script wrong: a maintainer running it by hand from anywhere but the
checkout hits the same thing, and §43.2 asks that critical release logic be readable in the script
rather than assembled from where its caller stood.

**Stop `cd`-ing into the artifact directory.** The `cd` is what lets the script name assets by bare
filename for `gh release upload`, and unpicking it touches the inventory check that compares what
was attached against what is here — the part §62.5 is about. A directory change is not the defect;
inferring identity from a directory is.

**Pass `--repo` to each `gh` call.** Five call sites, each of which could be forgotten by the sixth.
One exported variable is the same instruction to every one of them.
