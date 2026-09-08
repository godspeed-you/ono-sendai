# ADR-0603: Consent is asked at the broker when the need is concrete

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.2 §31.17, §31.18, §31.19, §31.37, §31.61, §31.63; v0.3 §1.22;
  `docs/specs/kuang11/kuang11-plugin-installation-permissions-spec.md` (K11P) §7.3, §14, §19.3, §20.3,
  §24.4, §33 Gates H–J, §34.7, §35.6; ADR-0264, ADR-0265, ADR-0600
- Decided by: agent (autonomous)

## Context

The supervisor's policy module says why it never asks: "this supervisor is a library with no
prompt to offer, so `ask` resolves to `deny`". That is right for a library and wrong for the
product once K11P §14 exists: a kubeconfig context that authenticates through `/usr/bin/aws` is
the moment at which the shell can say exactly which program, for exactly which reason, and
asking then is both safer and clearer than asking for generic `process.exec` at install
(K11P §35.6). The Kubernetes provider currently pre-checks the grant and refuses by name, telling
the user to re-load with `--grant process.exec` — a prompt would have been the answer.

## Decision

### 1. A consent seam in the supervisor, with a default that denies

`LoadConfig` gains `consent: Arc<dyn ConsentSource>`. The trait has one method:

```rust
fn consent(&self, request: &ConsentRequest) -> ConsentAnswer;
```

`ConsentRequest` carries the package id, the capability, the concrete scope values the call will
use (the resolved program and its arguments for `process.exec`, the host and port for
`network.connect`), the invocation label and the package's purpose text. `ConsentAnswer` is
`Deny` or `Allow { duration: Once | Session | Always, scope }`. The default, `NoConsent`, answers
`Deny`; the test host's `ScriptedConsent` answers what a test wrote down and records what it was
asked. The seam is called from `broker_check` only when the evaluation is `Denied(Default)` — a
system or operator deny is never re-asked — and only for a capability whose permission phase in
the package's descriptor set is `jit` (ADR-0600). A class B capability the package declared with
phase `jit` prompts the same way; an `explicit` one never does.

The call runs on the blocking pool, so a prompt that waits for a person does not stall the
runtime's other tasks; the plugin itself is waiting for the reply, which is what a request is.

### 2. The prompt, and what each answer does

The shell's `ConsentSource` renders K11P §14.2 on stderr and reads stdin:

```text
Kubernetes needs to run:
  /usr/bin/aws eks get-token --cluster-name prod
Reason:
  authenticate to Kubernetes context "prod"
Allow this helper?
  [o] once  [s] this session  [a] always for this program  [n] deny  [d] details
```

| answer | effect |
|---|---|
| once | this call proceeds; nothing is stored; the audit carries `permission.allow` with duration `once` |
| session | a session grant scoped to the exact program (`programs: [<resolved path>]`), added to the live policy and to the host's grant table as `source: prompt`, `permission: <id>` |
| always | the same grant with duration `always`, written to `policy.yaml`, and a decision record in `permissions.yaml` (ADR-0604) |
| deny | `permission.denied` (K11305) to the package; the denial is remembered for the session for that exact scope, so a pipeline is not asked twice; `set permission … --decision ask` clears it |
| details | the exact capability, scope keys, enforcement and the command line, then the question again |

`always` is never unrestricted `process.exec`: the grant carries the narrowest enforceable scope —
package, capability, the resolved executable path — and nothing a package writes in its descriptor
can widen it (K11P §14.3, §34.7, Gate I). `once` is enforced by the broker because the answer
authorises one evaluation and is not recorded as a grant at all — the objection ADR-0264 raised
to a `once` *grant* does not apply to a `once` *answer*.

### 3. Non-interactive: no prompt, a structured requirement

Where the session is not interactive, the source answers `Deny` with `permission.required`
(K11304) instead of `permission.denied`, carrying `plugin`, `permission`, `capability`,
`requested_scope` (`program`, `arguments`), `reason`, and a `remedy` that is a command line —
`set permission <name> <permission> --decision allow --scope programs=<path>` — and the
`grant capability` equivalent (K11P §20.3, Gate J). The package sees a wire error that leads with
the human permission and the reason, with the capability in the metadata (K11P §24.4).

### 4. `capabilities.check` answers `ask`

A check for a family the policy does not grant, whose permission is `jit` and whose session holds
no remembered denial, answers `CheckAnswer::Ask` rather than `Denied`. A package that reads
`Granted` or `Ask` as "proceed and let the host decide" gets the prompt; one that reads only
`Granted` keeps refusing itself, which is still correct. The Kubernetes provider is changed to
proceed on `Ask`.

### 5. What is *not* asked

Install-time permissions are decided at install. A capability the package never declared is
denied and audited without a prompt, as today. A `runtime_requested` request without an action
context is denied without a prompt, as today. Nothing here changes load-time negotiation: a denied
required capability still refuses before the binary starts.

## Consequences

- `Supervisor` audits `permission.ask`, `permission.allow` and `permission.deny` beside the
  capability event, correlated by the request id (ADR-0604 §4).
- The Kubernetes context that uses a token or a client certificate never meets the prompt
  (Gate H); the `exec` context meets it at first use and is asked about that exact helper.
- Encoded by `ono-kuang-sdk/tests/conformance.rs` (once, session, always, deny, non-interactive,
  `ask` on check), `ono-cli/tests/permissions_jit.rs` (the prompt through a pty-less stdin) and
  acceptance case `222-kuang-jit-permission`.

## Alternatives considered

- **Prompting from the SDK side, in the package.** A package cannot take the terminal (§31.27)
  and must not be trusted to describe its own request; the host owns both.
- **A pre-declared `programs` scope at install.** The program is whatever the operator's
  kubeconfig names; a scope written into the package would be wrong for their cloud or wide
  enough to mean nothing (the Kubernetes manifest already says so).
- **Persisting a `once` answer as a one-use lease.** The host only sees a use after the fact;
  answering one evaluation is the honest form of "once".
