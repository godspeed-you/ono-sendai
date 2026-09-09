# Migrating

One page per thing that could surprise somebody upgrading, added to as tranches land, newest
section last. **Exactly one migration on it is required**, and only for people running a directly
listening agent over TCP (§2). Everything else here is either automatic or a description of a
refusal you may now meet.

Sections 1–5 are v0.4.1's, and they are v0.4.1 §63's five in its order: a hardening release that
adds no language and changes no schema you can see, and changes what the shell refuses. Sections
6–8 are the KUANG/11 installation, permission and acquisition layers. Section 9 is v0.5, the
Temporal & Causal Systems Interface.

## 1. Ordinary local use — nothing to do

*(§63.1)*

No action is required for the local shell, for external command adapters, for spatial navigation
or for SSH-carried remote links. Configuration files continue to parse; new security settings are
additive and every one of them has a default that is the behaviour you already had, or a stricter
one that is stated below.

## 2. A directly listening agent — one required step

*(§63.2, and a release criterion under §66.8)*

**v0.4.1 stops accepting anonymous TLS clients.** In v0.4.0 a direct TCP agent authenticated
*itself* to the client and accepted whatever connected; in v0.4.1 both ends prove possession of a
key, and an authenticated client is still refused until an operator authorizes it. That is
deliberate and it is not configurable — there is no flag that turns client authentication off
(§7.4).

If you run `ono --agent --listen`, do this once per client, before upgrading the agent:

```bash
# On the client
ono --print-peer-key
```

That prints the client's own fingerprint, `sha256:` and sixty-four hex characters. **Verify it
through a channel you already trust** — read it aloud, compare it over an existing SSH session,
anything that is not the network you are about to authorize it on. Then, on the agent's host:

```bash
ono -c 'add client-key sha256:... --label my-laptop'
```

Adding a client grants observation and nothing else (§9.4). To let it perform one action, name
that action:

```bash
ono -c 'set client-key sha256:... --allow service.manage'
```

Grants name exact capability ids and never patterns (§9.5), so a wildcard is refused at the point
you type it rather than quietly widened later. `get client-key` lists what is authorized;
`remove client-key` revokes.

**What happens if you skip this.** The client connects, proves its key, and is refused with
`Ono-Sendai-E1202 remote.unauthorized`. The refusal carries the fingerprint to add, so the
migration is also recoverable from the error message:

```text
ono: Ono-Sendai-E1202 remote.unauthorized the client sha256:… is authenticated and is not
authorized on this host, where no client is authorized yet
```

**A v0.4.0 client cannot talk to a v0.4.1 agent** and fails safely rather than downgrading (§4.2).
Upgrade both ends, or reach the machine over `--transport ssh`, where OpenSSH does the
authenticating.

## 3. An existing host identity — automatic, and nothing is deleted

*(§63.3, §8.2)*

v0.4.0 kept a listening agent's key in `~/.config/ono/host_key.pem`. v0.4.1 calls the file
`~/.config/ono/link_identity.pem`, because both ends of a direct link now have an identity and it
is no longer something only a listening host has.

The move happens by itself, once, the first time the shell needs the identity:

1. if `link_identity.pem` exists, it is used;
2. otherwise, if `host_key.pem` exists and parses, it is **copied** across, mode `0600` preserved;
3. otherwise a new `link_identity.pem` is generated;
4. **the old file is never deleted.**

So a host that already ran an agent keeps the same key and the same fingerprint, and clients that
pinned it are not asked to re-trust anything — a host-key-change refusal for a rename would be a
refusal about nothing (§63.3). `ono --agent --host-key <path>` still names the old file.

One thing to check: the private key must not be readable by anyone else. If its mode is wider than
`0600` the shell refuses to use it and says so (§8.3). `chmod 600 ~/.config/ono/link_identity.pem`
is the fix.

## 4. Existing KUANG/11 plugins — no manifest change, and some may stop starting

*(§63.4, §16.2, §4.4)*

**No manifest migration is required.** Nothing in a package's declaration changes because
confinement failures became fatal.

What changes is what happens when a confinement control cannot be installed. In v0.4.0 a failed
`setrlimit` or `PR_SET_NO_NEW_PRIVS` could be ignored and the plugin ran anyway; in v0.4.1 it
refuses the launch, and the diagnostic names the control:

```text
ono: Ono-Sendai-K11803 plugin.no_new_privs_failed dev.example.thing was not started because
no_new_privs could not be installed: Operation not permitted
```

**If a plugin of yours stops starting, that is the intended change** (§4.4). It ran before only
because a control Ono claims to apply was not applied. The remedy is to fix the platform or
policy incompatibility the message names — a hard `RLIMIT_NOFILE` below what the tier configures,
a kernel without the option, a container policy that forbids it — and not to disable the check.
The package is *not* quarantined: it never started, so there is nothing to hold (§18.1).

`inspect plugin <id>` shows the controls actually in force for a package that did load.

## 5. Existing test infrastructure — silent skips become explicit ones

*(§63.5, §38.1, §65.10)*

This one is for contributors rather than users.

A test that returned early when a precondition was missing used to be counted as a pass. v0.4.1
requires three visible outcomes — PASS, FAIL and SKIP with a reason — so such a test is converted
rather than left:

```rust
// before: a silent early return, reported as a pass that asserted nothing
if !have_systemd() { return; }

// after: an announced skip with one of §38.4's six reasons
ono_testkit::require(
    have_systemd(),
    ono_testkit::SkipReason::ExternalToolUnavailable,
    "the fixture needs a running systemd",
)?;
```

Every test that can announce a skip is declared in
`docs/contracts/hardening/expected_test_skips.yaml`, and the gate compares that registry against the
tree in both directions: an undeclared skip fails, and a declared skip that stopped happening
fails too. A test name may be changed when the old name encodes semantics that no longer hold,
but intent and coverage are preserved.

## 6. KUANG/11 plugins after the permission layer — install by name, and what your grants become

*(`docs/specs/kuang11/kuang11-plugin-installation-permissions-spec.md` §28; ADR-0600 … ADR-0605)*

**No manifest migration is required.** A `kuang-package/1` manifest installs and loads exactly as
before; the host derives one human-readable permission per declared capability. A package that
wants its own wording declares a `permissions` section under `format: kuang-package/2`
(`docs/contracts/kuang/manifest.v1.yaml` → `permissions`).

**Existing grants are kept and projected, never rewritten.** Every grant in
`~/.config/ono/kuang/policy.yaml` stays what it is. `get permission <plugin>` shows it as
`allowed` where it matches a permission's mapping exactly, as `custom` where it is wider or
narrower or was made by hand, and as a `legacy` row where no permission maps its capability.
A broad `process.exec` grant reads `custom`; nothing narrows it for you.

**Two things change what a package holds by default.** A bounded extension-local capability —
`clock.read`, `state.persist`, `ui.view`, and `relation.write` scoped to the package's own
declared shapes — is included without a question, whichever way the package arrived. If you
relied on such a capability being *denied* by default, deny it deliberately:

```text
set permission <plugin> relation-write --decision deny
```

And installing a package now decides its recommended access. `install plugin <name>` (or
`install plugin path:<dir> --confirm`) grants the package's recommended, read-only profile; pass
`--access minimal` for the smallest one, and name a wider profile — `--access operate` — only on
purpose. Mutation is never part of recommended access, and an upgrade that widens authority asks
again (or, in a script, needs `--confirm` for a safe widening and refuses an explicit one).

The old sequence keeps working and is documented as the administrative surface:

```text
install plugin path:/srv/packages/dev.example.thing --confirm
grant capability network.connect --plugin dev.example.thing --duration always
revoke capability network.connect --plugin dev.example.thing
```

Revoking a grant that a permission minted records the permission as denied, so it does not come
back at the next load; `set permission <plugin> <permission> --decision ask` clears that.

## 7. KUANG/11 plugins from a system package — acquisition is not consent

*(`docs/specs/kuang11/kuang11-plugin-package-acquisition-system-distribution.md`; ADR-0606, ADR-0607)*

**Nothing you have installed changes.** A package installed from a catalog or a local path keeps
its lineage; a system package that appears later under `/usr/lib/ono-sendai/plugin-sources/` is a
candidate for `find plugin` and never the upgrade path of a package acquired another way, until
you say `install plugin <name> --source system`. `install plugin path:…` keeps working.

**A distribution package is a source, not an install.** `apt install ono-plugin-kubernetes` (or
`dnf`) places a versioned payload under the system root and nothing else: no `INSTALLED` state,
no enabled flag, no publisher trust, no permission, no grant. `install plugin kubernetes` then
takes it through the same verification, prompt and transaction as any other source, copying the
payload into `~/.config/ono/plugins/` — so `apt upgrade` under the root changes a candidate and
never the active package, and `remove plugin kubernetes` removes Ono's copy and tells you the
system source remains for the package manager to remove.

**Catalog artifacts are fetched now.** A catalog release naming an `https://` artifact is fetched
to staging, hashed against the catalog's digest, unpacked into the package cache and installed
from there; a release without a digest, or a plain `http://` artifact in a catalog that does not
declare `insecure_http: true`, is refused. A system copy is preferred over a fetch, and the same
version hashing differently in two places is `plugin.source_conflict`, which `--source` resolves.

**Two new environment variables** name the roots and nothing else: `ONO_PLUGIN_SYSTEM_SOURCES`
(the system roots; default `/usr/lib/ono-sendai/plugin-sources`) beside `ONO_PLUGIN_SOURCES`. A
system root writable by its group or by everyone is set aside with a warning rather than searched.

## 8. Signing a plugin without keeping a key

*(ADR-0609; `docs/specs/kuang11/kuang11-plugin-package-acquisition-system-distribution.md` §17.2)*

**Nothing you have changes.** A package signed with `kuang-sign` and a key verifies exactly as
before, and no installed package needs re-signing. What is new is a second way to sign, for
publishers who do not want to keep a private key — which is how Ono signs its own releases.

A package may now carry `signature.sigstore.json` beside `manifest.yaml`: a Sigstore bundle over
the same bytes the ed25519 signature covers, made by a workflow that holds no secret. Ono verifies
it offline, against certificate authorities and transparency-log keys it carries, and reports the
same four separate answers as before. A package may carry either signature or both; where both are
there, both must verify.

**Trusting one is enrolling an identity rather than a key.** `trust.yaml` gains a second list
beside `keys:`:

```yaml
format: kuang-trust/1
identities:
  - publisher: io.github.godspeed-you
    issuer: https://token.actions.githubusercontent.com
    identity: https://github.com/godspeed-you/ono-sendai-kubernetes/.github/workflows/release.yml@refs/tags/*
    trust: trusted
```

The subject may end in one `*`, so a repository's releases are enrolled once rather than once per
tag. `revoked` works there as it does for a key, and a revocation of either wins over a trust.

**Publishing one** needs `kuang-sign describe`, which writes the bytes a signature covers, and
`cosign sign-blob --bundle signature.sigstore.json <those bytes>` in a workflow with `id-token:
write` and no secret at all.

## 9. After the temporal tranche — nothing starts watching you, and two names differ from the spec

*(v0.5 §2 invariant 16, §10.2, §33, §34, §35; ADR-0610, ADR-0611, ADR-0775)*

**Nothing to do.** v0.5 adds time as a coordinate, and persistent recording is off until you ask
for it: §10.2 makes that a MUST, and §32.1 makes the store lazy, so a shell that was never asked
creates no database and reads none. `get recorder` on a fresh installation says `stopped`.

Three things are worth knowing before you meet them.

**Turning it on is bounded, and it says so.** `start recorder` begins retaining local history under
`~/.local/share/ono/temporal/`, directory `0700` and database `0600`, capped at 24 hours or
512 MiB — whichever removes data first. Process command lines are not persisted by default
(`temporal.record.process_argv`), secrets never enter the ledger, and `remove temporal-history`
clears it under the ordinary destructive-operation policy. `get recorder` states the policy in
force and how much is retained now.

**`look`'s recent-change section can now say `unknown`.** In v0.4 it reported what it had; in v0.5
it is backed by the temporal engine, and §24.3 forbids reporting an absence from a source that was
not watching. A session-only ledger is `partial` by §8.3 and can prove no absence at all, so where
v0.4 would have shown nothing you may now read `unknown` with no source named. That is the honest
answer rather than a regression: `empty` is reserved for a window something actually observed
(ADR-0775). Scripts that branched on an empty change list should branch on the state instead.

**Two identifiers differ from the specification text, deliberately.** If you are reading v0.5 §34
or §35 and matching on what the shell emits:

- the temporal error family is **E1301–E1314**, not §34's E1101–E1114. v0.4.1 §21.4 had already
  spent the first three of that block on the resource family. Every *name* §34 fixes is kept
  exactly — `temporal.not_recorded`, `temporal.read_only` and the rest — and only the numbers move
  (ADR-0610). Match on names.
- §35's `ono.evidence/1` is registered as **`ono.temporal-evidence/1`**, because v0.2 §31.24
  already holds `ono.evidence/1` for an unrelated record (ADR-0611).

## What to read next

- [`SECURITY.md`](../SECURITY.md) — the trust boundaries and how to report a vulnerability.
- [`docs/reference/remote-trust.md`](reference/remote-trust.md) — the six things a remote
  link keeps apart, including why authenticating is not being authorized.
- [`docs/reference/release-verification.md`](reference/release-verification.md) — checking
  the release you are about to install.
