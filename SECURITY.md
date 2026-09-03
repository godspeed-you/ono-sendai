# Security Policy

Ono-Sendai runs commands, adapts external tools, links to remote machines and loads extensions.
This page is the front door: where to report something, what is supported, and what the product
does and does not claim to defend. **It is not the security model.** The model is
`docs/ono_sendai_shell_spec_v0.4.1_hardening_trust_release_integrity.md`, and the vocabulary it
uses is generated at [`docs/reference/terminology.md`](docs/reference/terminology.md).

## Reporting a vulnerability

**Report privately.** Use [GitHub's private vulnerability
reporting](https://github.com/godspeed-you/ono-sendai/security/advisories/new) on this
repository — *Security → Report a vulnerability*. It needs no account beyond the one you already
have and no third-party service.

There is no published security email address. If private reporting is unavailable to you, open a
public issue that describes **the impact and the affected component only** — no reproduction, no
working exploit, no proof-of-concept input — and say that you have details to share privately.

**A public issue is the wrong place for an unpatched exploitable vulnerability.** The issue
tracker is world-readable and indexed, so a working reproduction posted there is a working
reproduction handed to everyone who can reach a shell you have not patched yet. That holds even
when the report is well-intentioned and even when the bug looks minor.

### What to expect

| | |
|---|---|
| Acknowledgement | within 7 days of the report |
| First assessment — severity, affected versions, whether it is in scope | within 14 days |
| Fix or a stated plan with a date | within 90 days of the assessment |
| Credit | in the release notes, under whatever name you ask for, unless you would rather not be named |

The project is developed by a small number of people and these are the times it commits to, not
the times it hopes for. If a deadline is going to be missed you will be told before it passes,
with the reason.

Please give the project the 90 days before publishing. If you disagree with an assessment — that
something is out of scope, or lower severity than you judged it — say so; a disagreement recorded
in the advisory is a better outcome than a silent one.

## Supported versions

| Version | Supported |
|---|---|
| 0.4.x | yes — security fixes |
| 0.3.x | no |
| 0.2.x | no |
| `implementation` branch | no — it is a working branch and is rebuilt without notice |

Only the latest 0.4 patch release receives fixes. There is no long-term-support line: the project
is pre-1.0 and says so rather than implying a maintenance commitment it cannot keep.

## What is protected

The threat model (v0.4.1 §5.1) names nine protected assets:

- **confidentiality of system data** exposed by providers;
- **integrity of provider actions** and remote mutations;
- the **identity of remote systems and remote clients**;
- **local files** readable by the Ono process;
- **network access** available to the Ono process;
- user **credentials** present in the environment, in files, or in process metadata;
- **availability** of the shell under untrusted or pathological input;
- **integrity of published release artifacts**;
- **trustworthiness of test and release claims** — a green suite that skipped the case it claims
  to prove is a security problem, not a hygiene problem.

## The boundaries, in one page

Each of these is enforced by one owning component, and each has an automated test that proves the
forbidden thing is refused (v0.4.1 §6.2, §20). The words below mean exactly what
[`docs/reference/terminology.md`](docs/reference/terminology.md) says they mean.

| Boundary | What crosses it | What is enforced |
|---|---|---|
| Direct TCP transport | bytes from the network | TLS 1.3 with mutual peer proof; no mode accepts a client that presents no certificate |
| Direct TCP authorization | an authenticated peer | the agent's own policy store — authentication is never sufficient |
| SSH-carried transport | an OpenSSH channel | OpenSSH authenticated it; Ono reports that rather than claiming it |
| Protocol frames | peer bytes | size, depth and version limits before anything is decoded |
| Provider query and act | an authorized request | the capability contract, and risk and elevation checks for mutations |
| KUANG/11 native spawn | a package process | manifest validation and confinement that fails closed |
| KUANG/11 protocol | plugin bytes | frame, credit and schema limits |
| External adapters | another program's output | the adapter's decoder and schema validation; never a guess |
| Pipeline materialization | a value stream | a count budget and a byte budget, both enforced |
| Release build | CI inputs | immutable action and image references, locked dependencies |
| Release publish | artifacts | checksums, a signature and build provenance |

A refusal at any of these says which one decided, in the ordinary error a user sees — see
[`docs/spec/hardening/refusals.yaml`](docs/spec/hardening/refusals.yaml).

## What is not protected

**Ono-Sendai does not claim to defend an unprivileged Ono process against a fully compromised
kernel, against an attacker who is already root on the same host, or against malicious hardware or
firmware.** Anything with that much authority can read the shell's memory, its keys and its
configuration directly, and no boundary above is meaningful against it.

**A native KUANG/11 plugin is not fully isolated from your user account.** It executes as a
process of the Ono user. Ono limits the capabilities it brokers and applies process confinement —
resource ceilings, no-new-privileges, its own session, a sanitized environment, a private working
directory, each installed before the plugin's first instruction and each able to refuse the
launch. That is confinement, and confinement is not kernel isolation: native execution is not a
complete filesystem or network sandbox, so a plugin can reach whatever your account can reach
without asking Ono for it. **Install native plugins only from sources you are willing to run as
your user account.**

Two further things are deliberately out of scope, and are not vulnerabilities:

- **Configuration you wrote.** `~/.config/ono/config.ono` cannot run commands at startup, which
  is a boundary; what you deliberately put in it is your decision, not an escalation.
- **Commands you ran.** The shell executes what it is asked to execute. `explain` shows what a
  mutation would do before it does it, and that is the protection offered — not a refusal to obey
  its user.

## Cryptographic material on disk

| File | Contents |
|---|---|
| `~/.config/ono/link_identity.pem` | this installation's private link identity — mode `0600`, and the shell refuses to use it if the mode is wider |
| `~/.config/ono/hosts.json` | the host keys you pinned; public material |
| `~/.config/ono/authorized_clients` | the client fingerprints this host authorizes, and what each may do; public material |

A diagnostic never prints private key material. Fingerprints are printed in full, deliberately:
they are public identity material and they are what an operator pastes into `add client-key`.

## Verifying what you install

Every release publishes `SHA256SUMS`, a signature over it and signed build provenance. Verify
before you install: the copyable sequence lives with the installation instructions, in this
repository's [README](README.md) and in the Wiki's
[Install](https://github.com/godspeed-you/ono-sendai/wiki/Install) page. An artifact whose digest
is not in a signed manifest is an artifact nobody has vouched for, whatever it was downloaded
from.
