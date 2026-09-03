# ADR-0433: CI pins what it pulls and asks for the least token

- Status: accepted
- Date: 2026-09-02
- Spec refs: spec §43.1 (immutable action references), §43.3 (permissions), §43.4 (pull-request
  trust), §43.5 (concurrency), §44.1 (container digests), §62.1 (action pin scanner), §62.2
  (mutable image scanner), §5.2 attacker classes 9 and 10, §65.11 (mutable release inputs)
- Issues: #98, #99, #100
- Decided by: agent (autonomous)

## Context

Every input this repository's CI pulled from outside itself was named by something its publisher
can repoint.

Seven third-party actions across two workflows, all on floating references:
`actions/checkout@v4`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`,
`taiki-e/install-action@v2`, `actions/upload-artifact@v4`, `actions/download-artifact@v4`,
`softprops/action-gh-release@v2`. A tag is a pointer, and `@stable` is not even that — it is a
branch. Anyone who can move one of those seven names runs code inside a job that holds this
repository's token, and in the release workflow that token could write. That is attacker class 9
of spec §5.2, and it needs no compromise of this repository at all.

Four container images the same way: `rust:1.94-slim-bookworm` in both `docker/Dockerfile` and
`scripts/package.sh`, `debian:bookworm-slim` in the acceptance image, and
`debian:bookworm` plus `fedora:latest` in `scripts/package-check.sh`. Spec §44.1 names
`fedora:latest` and the undigested `rust:` tag specifically. The .deb "installed and proven to
work" in a package check today and the .deb published next month were validated against two
different operating systems that share a name — attacker class 10, and equally a reproducibility
defect (§65.11): the same commit does not build the same artifact twice.

And the permissions were the coarse default. `ci.yml` declared none at all, so every job took
whatever the repository hands out. `release.yml` declared `contents: write` at the top, which
gave it to the two `package` jobs — jobs that run `scripts/package.sh` and
`scripts/package-check.sh`, download crates, and build inside a container — for no reason beyond
the fact that one later job needs it. It had no concurrency guard, so two runs for one tag could
race to attach conflicting assets.

Each of these is a one-line edit. That is exactly the problem: they were one-line edits when the
workflows were written, and the unpinned form was the one that got written, because nothing
objected.

## Decision

**Every reference CI resolves is immutable, every job asks for the narrowest token that lets it
finish, and `xtask spec-check` refuses the alternative.**

Three rules, in `xtask/src/supply_chain.rs`, running in the gate beside the existing scans of
`xtask/src/scan.rs`:

### `check_action_pins` — a `uses:` names a commit

Every `uses:` in `.github/workflows/` and `.github/actions/` must end in a 40- or 64-character
hexadecimal object id. Repository-local actions (`./…`) are exempt, as spec §62.1 allows: they
are pinned by being the commit under test. The conventional trailing comment is accepted and is
what the repository writes, because a bare digest is unreviewable:

```yaml
- uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0
```

The seven references, resolved with `gh api repos/<owner>/<repo>/commits/<tag> --jq .sha` and
cross-checked against `repos/<owner>/<repo>/tags` so the comment names a tag that really points
at that commit:

| Action | Commit | Comment |
|---|---|---|
| `actions/checkout` | `11d5960a326750d5838078e36cf38b85af677262` | v4.4.0 |
| `dtolnay/rust-toolchain` | `4360b52568e2003a75bf9bc1d59f33a8e3fc893c` | stable branch, 2026-08-05 |
| `Swatinem/rust-cache` | `6323deb102c322ba6fcbdcafc7e3dddab59af2b6` | v2.9.2 |
| `taiki-e/install-action` | `e67fa11c4b9316fa714ddf0abed07a0c3143b95b` | v2.87.4 |
| `actions/upload-artifact` | `ea165f8d65b6e75b540449e92b4886f43607fa02` | v4.6.2 |
| `actions/download-artifact` | `d3f86a106a0bac45b974a628896c90dbdf5c8093` | v4.3.0 |
| `softprops/action-gh-release` | `3bb12739c298aeb8a4eeaf626c5b8d85266b0e65` | v2.6.2 |

`dtolnay/rust-toolchain` is the one with no tag to name: `stable` is a branch, and the repository
tags only `v1`, which is a different branch head. The commit pinned is the head of `stable` on
2026-08-05 — the code that was already running — so the pin changed nothing but its mutability.
The toolchain it installs is unaffected either way: both jobs pass `toolchain: "1.94"`
explicitly, and `rust-toolchain.toml` remains authoritative (spec §44.3).

### `check_image_digests` — a pull reference carries a digest

Every base image in a Dockerfile, every `*IMAGE*` variable in `scripts/`, and every `image:` or
`container:` in a workflow must carry `@sha256:<64-hex>`. The tag stays in front of it, because
`name:tag@digest` is a legal pull reference and the tag is the only thing that tells a reviewer
what the digest is meant to be:

```
FROM rust:1.94-slim-bookworm@sha256:cf9dd0ec73e75f827fe59123fff9dc65af1a1c8363c3c31ee8d7f8ad0b6a5fb2
```

The four, resolved from the registry's `Docker-Content-Digest` for the multi-architecture index —
the index, not a platform manifest, so `ubuntu-24.04-arm` still resolves:

| Image | Digest |
|---|---|
| `rust:1.94-slim-bookworm` | `sha256:cf9dd0ec73e75f827fe59123fff9dc65af1a1c8363c3c31ee8d7f8ad0b6a5fb2` |
| `debian:bookworm-slim` | `sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171` |
| `debian:bookworm` | `sha256:6ebd97fa83deb272194a2cf015b3d26a4d538e9ad3a7a79d544c8af5b0a01443` |
| `fedora:latest` | `sha256:43b29f65a41eb9c35e1cd5323e3bdf3b655c2357a9f4f1ff2f9c2798e5045d80` |

Three references are skipped, and none of them is an allowlist: a Dockerfile stage naming an
earlier stage, an image this repository builds itself (`ono-sendai:…`, which is never pulled from
anywhere), and a reference that is only a shell expansion, which is pinned wherever the variable
is set. Spec §62.2 permits an allowlist for test-only convenience images; this repository has
none that it pulls, so it has no allowlist. An unused exemption is a hole waiting for its first
user.

### `check_workflow_permissions` — least privilege, and untrusted runs see nothing

**Untrusted, here, means a run whose code came from outside the set of people who can push to
this repository** — in practice a `pull_request` opened from a fork. GitHub already gives such a
run a read-only token and withholds repository secrets. The rules keep that true after the next
edit rather than by luck:

- every workflow declares `permissions:`, so no job inherits the repository default;
- the workflow-level block grants no write scope. `contents: write` is declared on the `publish`
  job of `release.yml` and nowhere else;
- `pull_request_target` does not appear. Spec §43.4 forbids using it to run untrusted code with
  elevated permissions; running trusted code is the only other thing it is for, and this
  repository has no such need, so the rule is a flat refusal rather than a judgement about
  what a given workflow does with it;
- a workflow a pull request can start reads no secret but `GITHUB_TOKEN`, which is minted per run
  and bounded by the `permissions:` block above rather than stored;
- a workflow containing a write-granting job is not reachable from a pull request at all. That is
  what isolates the release path: `release.yml` triggers on `push: tags: v*` and
  `workflow_dispatch`, and proposing a change to it does not run it;
- a workflow containing a write-granting job declares `concurrency:`. `release.yml` now uses
  `group: release-${{ github.ref }}` with `cancel-in-progress: false`, so a second run for one
  tag waits instead of racing the first to attach a conflicting asset (spec §43.5).

`ci.yml` gets `permissions: contents: read` at the top and needs nothing else: it publishes
nothing and reads no secret, so a fork pull request already runs it with an empty hand.

## Consequences

Easy: a floating reference cannot be reintroduced quietly. The gate names the file, the line and
the reference, and says how to resolve the pin — the message carries the `gh api` and
`docker manifest inspect` command, because a rule that fails without saying what to type is a
rule people work around. The same three whole-repository tests that hold this repository to the
rules would have caught every one of the eleven references as they were written.

Hard, and accepted deliberately: **a pinned SHA has to be bumped by a human decision.** Nothing
here dependabots itself. A security fix in `actions/checkout` does not arrive; somebody resolves
the new tag and edits the pin, and the trailing comment is what makes that edit reviewable —
`# v4.4.0` → `# v4.5.0` beside a changed digest is a diff a reader can judge, where a bare digest
change is not. This is the trade the spec asks for: §65.11 calls a mutable release input a
failure mode, and the cost of immutability is that staying current becomes work somebody does on
purpose. The bump is one command per reference and belongs in its own `chore:` change.

Also hard: the pinned `fedora:latest` and `debian:bookworm` digests will age, and
`scripts/package-check.sh` will keep proving the packages install on an operating system that is
no longer current. That is the correct failure — a stale-but-known validation environment beats
an unknown one — but it is a reason to bump the four image digests when the release candidate is
cut, not a reason to leave them floating. The environment overrides
(`ONO_PACKAGE_CHECK_DEBIAN`, `ONO_PACKAGE_CHECK_FEDORA`) still let a developer test against
something newer without touching the file.

Left for the neighbouring phases: this scan says nothing about *which* dependencies are allowed
(spec §45, advisory and licence policy) and nothing about whether two builds of one commit are
byte-identical (spec §46). It only guarantees the inputs are the same ones. Those are separate
checks and separate ADRs.

Encoded by, in `xtask/tests/supply_chain.rs`:

- actions — `should_reject_an_action_referenced_by_a_floating_tag`,
  `should_reject_an_action_referenced_by_a_branch_name`,
  `should_accept_an_action_pinned_to_a_commit_sha_with_the_tag_in_a_trailing_comment`,
  `should_reject_a_forty_character_reference_that_is_not_a_commit_sha`,
  `should_accept_an_action_that_lives_in_this_repository`,
  `should_reject_an_unpinned_action_inside_a_composite_action`,
  `should_ignore_an_action_reference_that_is_only_written_in_a_comment`,
  `should_report_this_repository_as_pinning_every_action_it_uses`;
- images — `should_reject_a_build_image_pulled_by_tag_alone`,
  `should_accept_a_build_image_pinned_by_digest_with_the_tag_still_readable`,
  `should_accept_a_later_stage_that_builds_on_an_earlier_one`,
  `should_accept_an_image_this_repository_builds_itself`,
  `should_reject_a_package_validation_image_named_by_a_shell_variable_without_a_digest`,
  `should_accept_a_shell_variable_whose_default_carries_a_digest`,
  `should_ignore_a_flag_variable_whose_name_merely_mentions_an_image`,
  `should_reject_a_workflow_job_running_in_a_container_image_without_a_digest`,
  `should_report_this_repository_as_pinning_every_release_critical_image`;
- permissions — `should_reject_a_workflow_that_declares_no_permissions_at_all`,
  `should_reject_a_workflow_that_grants_write_access_to_every_job`,
  `should_reject_a_workflow_that_grants_every_scope_at_once`,
  `should_accept_write_access_granted_only_to_the_publishing_job`,
  `should_reject_a_workflow_triggered_by_pull_request_target`,
  `should_reject_a_secret_reachable_from_an_untrusted_pull_request`,
  `should_accept_the_automatic_token_in_a_workflow_a_pull_request_can_start`,
  `should_reject_a_publishing_job_a_pull_request_can_reach`,
  `should_reject_a_publishing_workflow_without_a_concurrency_guard`,
  `should_accept_a_read_only_workflow_a_pull_request_can_start`,
  `should_report_this_repository_as_granting_least_privilege_in_every_workflow`.

## Alternatives considered

**Pin the workflows and skip the scanner.** The eleven unpinned references were written by people
who knew better; what was missing was an objection. A pin without a scan is a pin that lasts
until the next hurried edit, and spec §62.1 and §62.2 ask for the scan by name.

**Parse the workflows as YAML for all three rules.** A YAML parser discards comments and line
numbers, and both are the point for the pin rules: the trailing comment is what makes a bump
reviewable, and `file:line` is what makes a red gate actionable. The permission rules do need
structure — inheritance from workflow to job is a shape, not a line — so they parse, and the two
pin rules read lines. Splitting on what each rule actually needs beats one mechanism that serves
neither well.

**Allow `pull_request_target` with a review rule.** Every safe use of it is a use where
`pull_request` would also have worked, and every unsafe use looks safe at the moment it is
written. A trigger nobody may use is a rule a scanner can hold; "used carefully" is not.

**Detect image references anywhere in a script, not only in `*IMAGE*` variables.** A scanner
guessing which word of a `docker run` line is an image would either miss the inlined ones or
report `--rm`. The convention this repository already follows — the reference lives in a named
variable at the top of the script — is readable, and holding scripts to it is a smaller rule
than teaching a scanner to parse shell. The limit is real and is stated in the module: a script
that inlined a reference would slip past.

**Give every job its own `permissions:` block.** Explicit beats inherited, but with a
workflow-level default of `contents: read` and a scan that refuses any write scope declared
there, a job with no block of its own is already provably read-only. The extra blocks would be
four copies of one line, and four places for a later edit to disagree with itself.
