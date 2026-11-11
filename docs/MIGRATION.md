# Migrating

One page per thing that could surprise somebody upgrading, added to as tranches land, newest
section last. **Two migrations on it are required, each for some people only**: a directly
listening agent over TCP needs one step (§2), and every compiled KUANG/11 component needs one
`kuang-compile` after an upgrade that changes the WebAssembly engine (§10) — v0.6.2 to v0.6.3 is
such an upgrade, and so is v0.6.3 to v0.6.4; v0.6.4 to v0.6.5 needs no recompile, because
wasmtime stays at 48.0.8. Everything else here is either automatic or a description of a refusal
you may now meet.

Sections 1–5 are v0.4.1's, and they are v0.4.1 §63's five in its order: a hardening release that
adds no language and changes no schema you can see, and changes what the shell refuses. Sections
6–8 are the KUANG/11 installation, permission and acquisition layers. Section 9 is v0.5, the
Temporal & Causal Systems Interface. v0.6 and v0.6.1 have no section because they need nothing: a
shell that never types `plan` behaves as v0.5 did, the new `change.*` and `recovery.*` settings
default to the specification's reference configuration, and nothing a v0.6.0 configuration,
script or plugin relies on changed in v0.6.1 (`docs/releases/v0.6.0.md`, `v0.6.1.md`, under
*Compatibility*). Section 10 is v0.6.2, the KUANG/11 compiler leaving the shell. Sections 11–13 are
v0.6.3: one setting name retired, `measure`'s median behind an option, and what a script can now
see differently; v0.6.3 also moves wasmtime from 47 to 48, so §10's recompile is due once per
component. Section 14 is v0.6.4: one more recompile, what a job and a plan mean by a path, and what
the system and spatial model answer differently. Section 15 is v0.6.5: no recompile, and what the
KUANG/11 extension model now holds a package to — a destructive contribution's `--confirm`, a
declared risk, the session's shared instances and the contributed places and relations.

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

**A distribution package is a source, not an install.** `apt install
./ono-plugin-kubernetes_<version>_amd64.deb` (or `dnf install` of the `.rpm`) places a versioned payload under the system root and nothing else: no `INSTALLED` state,
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

## 10. KUANG/11 components are compiled once, by `kuang-compile`

*(spec §31.10, §31.36; issue #126; ADR-0870, ADR-0915, ADR-0916, ADR-0917)*

**One command per component you placed by hand, and one per component after an upgrade that changes the engine.**
`ono` now carries the WebAssembly runtime and no compiler, which takes about a quarter off the
binary. A `wasm-component` package still ships `runtime/component.wasm`; what runs is the engine's
compiled form of it, written by the SDK's `kuang-compile` into `~/.cache/ono/kuang/compiled/`
(or `$XDG_CACHE_HOME/ono/kuang/compiled/`), or by an administrator into
`/usr/lib/ono-sendai/kuang-compiled/` with `--store`.

- `install plugin` runs `kuang-compile` for you, from beside `ono` or from `PATH`. A component
  package installed that way loads straight away. The `.deb` and `.rpm` install it as
  `/usr/bin/kuang-compile`, beside `/usr/bin/ono` (ADR-0905). If the tool is not there — a shell
  built from source without it — the install is refused and names it.
- A component copied into the plugin path by hand, and every component after a shell upgrade that
  changes the engine, is refused with `load.component_not_compiled` (`Ono-Sendai-K11105`). The
  message carries the exact command, for example
  `kuang-compile ~/.config/ono/plugins/dev.example.echo/runtime/echo.wasm`, and the error's
  `reason` says `missing` or `incompatible`. Running the command fixes both; running it twice
  is harmless.
- An artifact, its store, or any directory above them that another user could write is refused
  as `untrusted` and never loaded: what the shell loads is native code (ADR-0915). A root-owned
  sticky directory such as `/tmp` is fine, and so is a directory of yours whose group is your own
  private group (the `775` directories a umask of `002` makes). A symbolic link anywhere from the
  store down is refused too; links above it (`/home -> /var/home`) are followed. If your cache
  sits below a directory a shared group can write, the refusal names that directory — make it
  `g-w`, or point `XDG_CACHE_HOME` elsewhere. `kuang-compile` refuses to write where the shell
  would refuse to read, and creates what it makes owner-writable only, whatever your umask.
- An artifact the shell may not read (permission denied) is refused as `unreadable`, not as
  `incompatible`; `kuang-compile` does not fix that, and the message says what does.
- An artifact is keyed on the exact wasmtime release (ADR-0916): every wasmtime upgrade, patch
  releases included, refuses existing artifacts as `incompatible` once, naming the command. An
  Ono upgrade that keeps wasmtime keeps them. Artifacts written before this change carry only
  the major version and are refused once. **v0.6.3 moved from wasmtime 47.0.4 to 48.0.5**
  (8327c34f), so after upgrading from v0.6.2 each compiled component is refused once with
  `Ono-Sendai-K11105`, naming the `kuang-compile` command that fixes it. The release after v0.6.3
  moves to 48.0.8 (80499901, 5b920caf) and asks once more (§14).
- The store follows the shell session's environment (ADR-0917): after
  `set env XDG_CACHE_HOME = …`, `load plugin`, `inspect plugin` and `install plugin` use the
  store it names, and `install plugin` runs `kuang-compile` with the session's environment and
  `PATH` (absolute entries only; an executable file only). When the store searched first is not
  the one `kuang-compile` chooses by itself, the command in the refusal carries
  `--store <store>`, and a `missing` refusal names the stores it looked in. The refusal an
  install raises when compiling fails carries `reason` `tool_unavailable` or `compile_failed`,
  and the component, command and engine.

Native-process packages are unaffected.

## 11. `history.result_cache` is a retired name for `limits.history_bytes_total`

*(spec §30, v0.4.1 §55.1, v0.6.1 §32; issue #175; ADR-0456, ADR-0933)*

**Nothing breaks; one line is worth rewriting.** Two keys used to name the ceiling on how many
bytes the result history may hold: `history.result_cache`, from spec §30's example, and
`limits.history_bytes_total`, v0.4.1's name for it. Only the second was ever enforced — the first
was declared and read by nothing, so setting it changed nothing. From v0.6.3 one key names the
ceiling, and the old name is read as that key:

- `set config history.result_cache = 64MiB` in `config.ono` or at the prompt sets
  `limits.history_bytes_total`, range-checked like any other assignment of it. Where the old key
  used to be silently ignored, the figure you wrote now takes effect.
- `ONO_HISTORY_RESULT_CACHE` is read as `ONO_LIMITS_HISTORY_BYTES_TOTAL`; when both are set, the
  new variable wins.
- `get config history.result_cache` answers the `limits.history_bytes_total` row, and `get config`
  lists only that key.
- The first time a session meets the old name the shell says so once, on standard error:
  `` `history.result_cache` is a retired name for `limits.history_bytes_total`, which holds its value
  now; write `limits.history_bytes_total` instead (ADR-0933) ``. Rewriting the line silences it. The notice is
  not a configuration problem: `get config --problems` does not list it.

## 12. `measure` reports the median when it is asked for

*(v0.2 §19, v0.4.1 §22.1; issue #174; ADR-0953)*

**One field moved behind an option.** `measure` used to hold every value it read in order to
report a median, which made it a global materialization bounded by `limits.materialize_items`.
From v0.6.3 a plain `measure` keeps constant state — `count`, `sum`, `mean`, `min`, `max` and
`stddev` — holds nothing, and answers on an unbounded stream after every value. Its `median` and
`percentiles` fields are `null` unless you ask:

- `… | measure memory --median` reports the median;
- `… | measure memory --percentiles [90, 99]` reports those percentiles, and the median with them.

Asking holds the distribution again, under the same materialization budget as before. A script
that read `median` from a plain `measure` reads `null` now; add `--median`.

## 13. What a script can see differently after v0.6.3

*(GitHub milestone `v0.6.3`; ADR-0929)*

**Nothing a correct script relied on was taken away, but some answers are now stricter or richer.**
Each item below is a contract the release made true; check a script that parses the old shape.

- **A wrongly typed evaluated argument is refused** (#168, ADR-0937). `get command --verb ["get"]`
  used to drop the filter and answer the whole registry; it now fails with
  `Ono-Sendai-E0201 type.mismatch` naming the option, the declared and the received type. A value
  that fits — `--verb ("get")`, `--verb $verb` holding a string — behaves as the word would.
- **Refusals print the fields that decided them** (#180, ADR-0938). After the message and before
  the help, a refusal shows `key: value` lines such as `stage: sort`, `limit: 2`, `consumed: 3`,
  `setting: limits.materialize_items`. A script that read a fixed number of stderr lines
  (`2>&1 | head -2`) reads a different part of it now; catch the error (`catch e { $e.name }`)
  instead of parsing the text.
- **A budget of zero is refused as `resource.materialization_limit`** (Ono-Sendai-E1103, #183,
  ADR-0934) before the stage reads, where it used to fail on the first value with
  `resource.item_limit` (E1101).
- **`type`, `inspect`, `inspect limits` and `get context` answer records** — `ono.type-info/1`,
  `ono.inspection/1`, `ono.limit/1`, `ono.context/1` — as their contracts always declared, instead
  of plain maps (#149, ADR-0935). `| to json` carries every declared field, so a field a map used to
  omit may now appear as `null`.
- **`explain` with a quoted, braced or `$variable` subject is a producer** (#173, ADR-0942):
  `explain "get process | sort pid" | to json` serialises the plan. The unquoted whole-line form is
  unchanged. A subject that would have to run code to be explained — `$( … )` in a quoted subject —
  is refused, and a prefix-assignment value that could run code is shown as written rather than
  evaluated (ADR-0939). `get process | explain "…"` and the library's `ono:explain` stage are gone:
  `explain` is the shell's.
- **`to`'s formats are a closed list**, `jsonl` among them (#214, ADR-0954); a misspelt format is
  refused, naming the nearest format.
- **A background job may not change directory** (ADR-0955): `cd`, `enter <dir>` and a restoring
  `leave` inside `… &` are refused with `type.mismatch`, because a job shares the shell's one
  working directory. Change directory before backgrounding, or pass absolute paths.
- **A redirected stream whose reader leaves ends that line with status 141** and the shell carries
  on; the shell itself exits with 141 only when its own standard output is gone.
- **`ono --complete '<line>' [--cursor N]`** is new (#176, ADR-0945): one `ono.completion/1` JSON
  document of what Tab would offer, with `complete: false` when the set was cut short.

## 14. What changes with v0.6.4

*(issues #301–#303 and the milestone `v0.6.4`; ADR-0957 … ADR-0959, ADR-0961 … ADR-0987;
`docs/releases/v0.6.4.md`)*

What v0.6.4 changes for someone upgrading from v0.6.3. The first seven entries come from the
maintenance prelude between the two releases, the rest from the milestone:

- **Recompile compiled KUANG/11 components once more.** wasmtime moves from 48.0.5 to 48.0.8 for
  eighteen advisories against it and wasmtime-wasi (RUSTSEC-2026-0352 … -0359, -0361 … -0364,
  -0378, -0379, -0420, -0422 … -0424), rustls to 0.23.46 for four more, tokio, rustls-webpki
  and webpki-roots for three (RUSTSEC-2026-0381, -0382, -0386), and rusqlite to 0.41.1 for
  RUSTSEC-2026-0459.
  An artifact is keyed on the exact wasmtime release (§10, ADR-0916), so each compiled component is
  refused once with `Ono-Sendai-K11105`, naming the `kuang-compile` command. Native-process
  packages are unaffected.
- **A job's relative paths mean the directory it was started in** (#302, ADR-0957). Inside `… &`
  every relative path — an argument, a glob, a redirection, a piped record's path, a program's
  working directory, a plugin reference — is anchored on the job's start directory, whatever the
  foreground does with `cd` afterwards.
- **A plan acts on the absolute path it froze** (ADR-0957). `plan remove file a.o` in one
  directory, `cd` elsewhere, `apply` removes the planned file, where it used to remove the namesake
  in the new directory. A plan stored by v0.6.3 with a relative operand is applied only where that
  operand still resolves to the target the plan froze; elsewhere `apply` refuses with
  `change.target_unresolved` and changes nothing.
- **Every backgrounded line is a job with an evaluator of its own** (#301, ADR-0958), native-only
  lines included: its scope, redirections, failures and status are the foreground's. `fg %N` waits
  for the job and shows its values once, with the status the line ended with, with or without a
  terminal; a native job's binding errors are reported when it is collected rather than before
  `[%N]` is printed.
- **What a job keeps, and every collection of a program's output, is bounded by
  `limits.command_capture_bytes`** (ADR-0958, 256 MiB by default). A line whose program writes more
  than that between two stages, or into a capture, is now refused with `resource.byte_limit`
  where it used to grow the shell; raise the setting deliberately for such a line.
- **Every job ends with the shell** (#303, ADR-0959). Leaving the shell — `exit`, end of input,
  `SIGHUP`, `SIGTERM` — sends each owned process group `SIGTERM`, then `SIGKILL` after one bounded
  grace period; there is no `disown`. A program meant to outlive the shell belongs to a service
  manager (`systemd-run --user …`).
- **A decimal number in JSON reads as the nearest `f64`** (ADR-0961). Where a provider, a remote
  peer or `from json` sent a number such as `1.04858e132`, v0.6.3 could hold it one unit in the
  last place away (`1.0485799999999999e132`) and log and forward that; it now holds the number sent.
- **`enter <unit>.service` means the service, and a control group is named by its path**
  (ADR-0971). `cgroup`, `namespace` and `address` are targets now, so `get cgroup`, `get namespace`
  and `get address` answer, and a spatial exit that read `unsupported` — a process's `cgroup` and
  `namespaces`, an interface's `addresses` and `routes`, a process's `endpoints`, a file's `owner`
  — leads somewhere. A script that tested for `unsupported` on one of them sees a place.
- **A redirected view follows a stated `COLUMNS`** (ADR-0979, superseding part of ADR-0166).
  `COLUMNS=80 ono -c 'look' > file` draws 80 columns, where it drew the redirected default. Unset
  `COLUMNS` for the old width.
- **`look` at the local root prints the links** (ADR-0985), each with its round trip and when it
  was last seen, when the session holds one. A session without links prints what it printed.
- **An object place can carry scope markers** (ADR-0983). A process inside a container is at
  `local container:<id> /process/<name>`, and one in another pid namespace at
  `… ns:pid/<inode> …`; `follow` across such a boundary prints `crossing scope: A -> B` on the
  diagnostic stream. The `scope` of such a place names the container, where it named the host.
- **The startup horizon waits 150 ms** (ADR-0982). A count whose provider has not answered by then
  reads `unknown — still being read` and is there on the next view. A `-c` script and a redirected
  `look` wait for the full answer as before.
- **`map --cluster user|service|container`** is new (ADR-0981), and the map of a directory draws
  its entries and clusters the rest by kind, `/etc (files)` (ADR-0976), where it drew one node.
- **Schemas gained fields, and none lost one.** `ono.kernel/1` is new (`get kernel`) and carries
  `memory_total`; `ono.process/1` gains `start_ticks`; `ono.service/1` gains `restarts`;
  `ono.container/1` gains `mounts`; `ono.link-place/1` gains `round_trip`, `last_seen` and
  `same_host_as`. A consumer that rejects unknown fields needs them added.
- **A `where` directly after a producer may be answered at the source** (ADR-0980). The answer is
  the same set of records with one stated exception: `get journal | where priority <= 3` asks
  `journalctl --priority`, so an entry without a canonical `PRIORITY` is left out, where it used to
  end the command with `Ono-Sendai-E0908`. `providers.pushdown = false` restores the old path, and
  `explain` names what is handed over.
- **A `high_memory` landmark names the host's memory as its budget**: its evidence reads
  `memory 6.00 GiB, 38% of host memory 16.00 GiB`, where it read `… 38% of 16.00 GiB`. A
  cgroup's memory limit is not read (#307). A script matching the old text needs the two words.
- **A journal entry whose `PRIORITY` is not one digit `0`–`7`** — `03`, `+2`, `-1`, `8` — is
  reported as `Ono-Sendai-E0908` by `get journal` and `get log`, where v0.6.3 read it as the
  number it spells (ADR-0988). journald writes those digits itself; only a client of its native
  protocol can store anything else.
- **`ono.link-place/1.same_host_as` is always present**, an empty list where no other link
  reaches the same machine. A program that writes `ono.link-place/1` records itself has to supply
  it; Ono's own link map is the only producer the shell has.
- **A file held open by a writer is `Ono-Sendai-E0305 io.busy`** (ADR-0962), where running it
  answered "found and not executable". The status stays 126.
- **`get filesystem` answers once per superblock** (#167): two tmpfs mounts are two rows, where
  one row stood for both.
- **A provider written against `ono-provider-api`** sees two additions it may ignore:
  `Provider::pushdown()` with `Query::pushed()`, and `Query::sought()` (ADR-0980, ADR-0987). Both
  are advice; a provider that reads neither answers as before.

## 15. What changes with v0.6.5

*(the milestone `v0.6.5`; ADR-0990 … ADR-0992, ADR-0995 … ADR-0998, ADR-1000 … ADR-1002,
ADR-1005 … ADR-1016, ADR-1020 … ADR-1024, ADR-1026 … ADR-1028, ADR-1030, ADR-1031;
`docs/releases/v0.6.5.md`)*

What v0.6.5 changes for someone upgrading from v0.6.4. There is **no recompile** this time:
wasmtime stays at 48.0.8, so a component `kuang-compile` compiled for v0.6.4 loads unchanged
(§10). The first four entries can make something that ran before refuse; the rest describe
answers that are richer or arrive differently.

- **A contributed command declared `risk: destructive` needs `--confirm` on every run** (#156,
  ADR-1001). In a script, in a background job and at a terminal alike, an unconfirmed run is
  refused with `safety.confirmation_required` before the package is loaded or sent anything —
  `` `<spelling>` is destructive and was not confirmed ``, with the help "nothing was changed.
  Write `<spelling> --confirm` to act". Nothing prompts, exactly as for `protect` or
  `stop socket`; `help` and completion offer `--confirm`. The flag counts wherever it is written,
  and takes the next word only when that word is `true` or `false`: `remove echo-record --confirm
  r1` confirms and binds `r1` to the selector (19670ee0). The Kubernetes provider's
  `remove k8s-resource` is `risk: destructive`, so its default server dry run needs the flag too:

  ```text
  remove k8s-resource --context prod --kind ConfigMap --name stale --confirm                   # server dry run
  remove k8s-resource --context prod --kind ConfigMap --name stale --dry_run false --confirm   # deletes
  ```

  The provider's own declared examples and two of its live-cluster tests invoke it without
  `--confirm` and refuse under v0.6.5 until the provider's repository adds it. `set k8s-resource`
  (`risk: mutate`) asks for nothing, as before.
- **`get plugin` answers `ono.plugin/2`, without `isolation`** (#199, ADR-0996). `execution_tier`
  is the one field naming what an instance runs in; the manifest's own word is
  `inspect plugin <id> | select manifest` (`runtime.kind`). A script that selects `isolation`
  gets a missing field. Every place that named `ono.plugin/1` — help, `get command`, the reference
  pages, the `ref<…>` fields of the KUANG/11 schemas — names `ono.plugin/2`; the identity
  `[id, version]` is unchanged, and the records that refer to a package keep their own versions.
  `ono.plugin-runtime/1` keeps both fields. The record also changed what it reports:
  `memory` is the instance's measured allocation where it was always null, `cpu_time` is new
  (null for a component and for a package with no instance), and `jobs` counts the invocations
  running in the instance where it was always 0 (#222, ADR-0997, ADR-0998).
- **A command that needs a `mutate` or `destructive` capability declares its risk, never below
  the capability's** (#156, ADR-1000, ADR-1002, ADR-1022). The `mutate` families include
  `network.connect`, `process.exec`, `process.signal`, `provider.mutate`, `state.persist`,
  `secret.use`, `relation.write` and `history.write`; `filesystem.write` is `destructive`
  (`docs/contracts/capabilities.yaml` → `kuang_capabilities`). A package whose command declares
  one of them without a `risk:` line, or with a lower risk, installed before: under v0.6.5 the
  on-disk declaration is refused at registration (`package.invalid`, reported by `get plugin`,
  absent from `get command`), and a hello declaration is refused the same way when the command is
  invoked. The fix is one `risk:` line. A command that only reads over `network.connect` has to
  say `risk: mutate`, which asks for no confirmation; a read that should read as a read is
  contributed as a target. Where a command is declared on disk and in the hello, the stronger
  declaration decides its confirmation.
- **A destructive host call is the declaring command's** (ADR-1022). The broker refuses a host
  call needing a `destructive` family — `recovery.restore` today — with `capability.denied`,
  `reason: undeclared-by-command`, unless every invocation open in the instance is a command that
  declared the capability. A target never declares one, and a call made with no invocation open
  is refused as well. An honest restore running alone is unaffected; one running beside another
  invocation of the same instance that did not declare it is refused, because a host call does not
  say which invocation made it.
- **A background job runs packages in the session's instances** (#304, ADR-0995). A package a job
  uses is the session's instance, listed by `get plugin`; the audit trail holds one
  `lifecycle.load` and one `lifecycle.unload` (capability `plugin.unload`, invocation `host`) per
  instance, and `unload`, `disable`, `remove`, a reload and the session's end now record the
  unload. A package's `context.get` inside a job answers where the session stands now, which can
  differ from the job's start directory. Leaving the shell with loaded packages can take up to one
  more grace period (2 s) for an instance that does not acknowledge its shutdown.
- **`kill %N` and Ctrl-C cancel a package invocation** (ADR-0998). A job blocked on a contributed
  command or target ends on `kill %N`, and Ctrl-C ends a contributed command read in the
  foreground; the line ends as interrupted (130) and the instance stays loaded, where `kill %N`
  used to report the job as not stopped yet.
- **An invocation beyond a package's concurrency ceiling waits for a slot** (ADR-1027), where it
  was refused at once. It runs when an invocation of the instance ends; after ten seconds without
  a free slot it is refused with `runtime.concurrency_limit`, naming the invocations that hold the
  slots, and Ctrl-C ends the wait. The bound is not a setting. A script that relied on the
  immediate refusal now waits up to the bound; `take`, `let xs = (get …)` or a higher
  `max_concurrent_invocations` in the package's manifest avoid the wait.
- **A package is loaded once when a job and the shell reach it together** (ADR-1026). Where a
  race used to leave two loads, one unload and `Ono-Sendai-K11201 runtime.trap the instance was
  shut down` on one side, both lines now run in one instance.
- **A view names its invocation** (#157, ADR-0991, ADR-1028). Packages built with the
  `ono-kuang-sdk` of this release send it, and nothing changes for them. A package built with an
  earlier SDK opens views without it; that is accepted while one invocation is open in the
  instance and refused with `view.protocol_error` while several are — which a background job or a
  waiting invocation now makes common. The refusal names both remedies: rebuild with
  `ono-kuang-sdk` 0.6.5, or declare `runtime.max_concurrent_invocations: 1` in the manifest. A
  package that speaks the protocol without the SDK names `invocation` in `views.open`.
- **A package command that stops short fails** (#154, ADR-0992). A contributed command or target
  whose package answers `cancelled` without having been cancelled now ends with
  `stream.cancelled` and status 1 after the values it sent, where it succeeded with a truncated
  answer. A refusal from an unbounded target is reported once, where it was printed twice. A
  package that speaks the protocol without the SDK should know that a clean `streams.close` no
  longer ends the consumer's stream before the invocation's result arrives.
- **A bounded contributed target streams** (#224, ADR-1011). `get <target> | take N` returns on
  the Nth value and stops the package; a failure after some values fails the command, with the
  values that arrived standing beside the error.
- **`watch <contributed target>` refuses** with `provider.unsupported` and names `get <target>`
  of an unbounded target as the live read a package has (ADR-1013). It used to run the `watch`
  program on `PATH`, which re-ran a command of that name every two seconds.
- **A relation shape may carry a name and a cost** (#224, ADR-1006):
  `<from>-><to>[; name=<word>][; cost=<cheap|normal|expensive|privileged|remote>]`. A package that
  uses either needs v0.6.5: an older host reads `b; name=x` as an endpoint that names no kind of
  place and refuses the package at load. A shape naming another package's schema, refused before,
  now loads and waits for that kind, and `follow` loads the installed package that contributes
  it (ADR-1008). A package's `spatial-relation` command is invoked
  with a `relations` argument naming the relations wanted, and a package that ignores it is
  still answered correctly (ADR-1020). Unloading a package withdraws its relations, and a reload
  declares them anew (0bdf84a5).
- **A schema may declare `scope`** (#224, ADR-1009). A schema in a package's
  `contributions.schemas` document may state `scope: [<field>, …]`; its places then report a
  `plugin:` scope — `plugin:cluster=kind-ono/namespace=shop` — where they reported the host, and
  movement between two such scopes records `scope_crossing`. Identity and place path are
  unchanged. A package declares scopes only for schemas in its own namespace, read from the
  directory the session loaded it from; a declaration for another package's schema is ignored
  (ADR-1030).
- **Spatial words can load an installed package** (#129, ADR-1005, ADR-1021). `find place
  --role`, `find place --type`, `jump <kind>/<key>` and `jump @<pin>` load the package whose
  on-disk declarations name that role or kind, so a `lifecycle.load` can appear in the trail where
  only a contributed command started one before. A package that names its schemas only in its
  handshake is not started for a kind word. A pin whose kind no installed package contributes no
  longer breaks the pin store: `jump @<pin>` reports it and `unpin <name>` removes it. `pins.json`
  stays at version 1.
- **`find place` can say it stopped at its bound**, on the diagnostic stream, when it read 1024
  objects from one contributed target (ADR-1007). Standard output is unchanged.
- **Three host services answer** (#203). `relations.contribute` keeps a package's edges for the
  session along its own declared relations, at most 1024 per package and at most 4096 bytes per
  edge, measured as the JSON the package sent; a call carrying a larger edge is
  `resource.item_limit` and nothing of that call is kept (ADR-1031). `relations.query` reads the
  edges as `inferred` (ADR-1014). `history.append` keeps a package's entry in
  `<state>/ono/history-packages/<package-id>.jsonl`, apart from `history.jsonl`: the prompt never
  recalls it, and `history.query` returns every entry with `package` (null for the operator's)
  and `cwd` (null for a package's) (ADR-1015). The operator's history file reads exactly as
  before. `secrets.request` still answers `provider.unavailable` and issues no handle; its message
  now reads "this shell keeps no secret store: the operator has nowhere to put a secret, the
  environment is not one, and no host call this build serves would consume a handle, so none is
  issued (ADR-1016)". The shell's environment is never read for a secret.
- **Who is asked a just-in-time permission is the invoking evaluator's** (ADR-1023). A background
  job asks nobody and gets `permission.required` with the remedy, whoever loaded the instance; the
  foreground at a terminal is asked, also in an instance a job loaded. A foreground question that
  arises while a job's invocation is open in the same instance is answered
  `permission.required`, and one reached only through the provider registry — resolving or acting
  on a contributed object, the edges of a contributed relation — is no longer asked at a terminal.
  An invocation that waited for a slot keeps its own evaluator's consent source (f426679e). An
  answer of `session` or `always` given once covers later calls.
- **An operator catalog may name `certificate_authorities`** (#153, ADR-1010): PEM files, absolute
  or relative to the catalog file, against which that catalog's HTTPS artifacts are verified in
  place of the public roots. The built-in catalog and a catalog of unknown origin that name one
  are refused as `package.invalid`; a named file that cannot be read refuses the fetch with
  `plugin.catalog_unavailable`.
- **A plugin launch refused before its first confinement control names the cause** (#197,
  ADR-0990): `… was not started because the spawn failed before <control> could be installed:
  Permission denied (os error 13)`. The error code and the metadata keys are unchanged.
- **A Rust consumer of `ono-kuang-supervisor`** sees `Sandbox` renamed `ExecutionEnvelope` and
  `LoadedPlugin::sandbox()` renamed `envelope()` (#198), and three additions it may ignore:
  `LoadedPlugin::running_invocations()` (ADR-0998) and `invoke_with_consent` /
  `query_with_consent`, which carry the invoking evaluator's consent source (ADR-1023); `invoke`
  and `query` keep their signatures.

## What to read next

- [`SECURITY.md`](../SECURITY.md) — the trust boundaries and how to report a vulnerability.
- [`docs/reference/remote-trust.md`](reference/remote-trust.md) — the six things a remote
  link keeps apart, including why authenticating is not being authorized.
- [`docs/reference/release-verification.md`](reference/release-verification.md) — checking
  the release you are about to install.
