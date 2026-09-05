# Migrating to v0.4.1

v0.4.1 is a hardening release. It adds no language and changes no schema you can see; what it
changes is what the shell refuses. **One migration is required, and only for people running a
directly listening agent over TCP.** Everything else on this page is either automatic or a
description of a refusal you may now meet.

The five paths below are v0.4.1 §63's five, in its order.

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

## What to read next

- [`SECURITY.md`](SECURITY.md) — the trust boundaries and how to report a vulnerability.
- [`docs/reference/remote-trust.md`](docs/reference/remote-trust.md) — the six things a remote
  link keeps apart, including why authenticating is not being authorized.
- [`docs/reference/release-verification.md`](docs/reference/release-verification.md) — checking
  the release you are about to install.
