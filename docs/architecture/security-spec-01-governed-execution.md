---
title: "ONO-SENDAI Security Specification 01"
subtitle: "Governed Execution, Sandboxes & Credential Mediation"
author: "Project Specification"
date: "2026-11-15"
geometry: "paperwidth=157mm,paperheight=210mm,left=13mm,right=13mm,top=14mm,bottom=15mm"
fontsize: 10pt
mainfont: "DejaVu Sans"
monofont: "DejaVu Sans Mono"
colorlinks: true
linkcolor: blue
urlcolor: blue
toc: true
toc-depth: 3
numbersections: false
header-includes:
  - |
    ```{=latex}
    \usepackage{microtype}
    \usepackage{enumitem}
    \setlist{nosep,leftmargin=*}
    \usepackage{fvextra}
    \DefineVerbatimEnvironment{Highlighting}{Verbatim}{breaklines=true,breakanywhere=true,fontsize=\small,commandchars=\\\{\}}
    \setlength{\parskip}{0.45em}
    \setlength{\parindent}{0pt}
    ```
---

# ONO-SENDAI Security Specification 01
## Governed Execution, Sandboxes & Credential Mediation

**Status:** Normative cross-release architecture specification, adopted into the repository on 2026-11-15; requires an ADR before implementation where an inherited contract is ambiguous.

**Applies to:** Ono Core, ordinary shell processes, sandboxed subshells, automation and scripted invocations, KUANG/11 plugins and providers, the native AI assistant ([v0.10](../specs/ono_sendai_shell_spec_v0.10_native_ai_assistance.md) §23.6, §24.5), external-agent processes, mediated local and remote operations.

**Release policy:** This is **not** a new feature-release number. It defines reusable infrastructure and the v0.10 security acceptance dependency. Implementation can be divided across branches/releases, but v0.10 MUST NOT claim protected agent tool execution until the relevant gates in this document pass. No retrospective editing of v0.2-v0.9 specifications.

**Normative terms:** MUST, MUST NOT, SHOULD, SHOULD NOT, MAY.

> **An agent can request an operation; it cannot grant itself authority. A credential can be used for an authorized operation without becoming available to the agent.**

---

# 0. Executive Decision

Ono MUST implement three composable, provider-neutral security mechanisms in Core:

1. **Execution Sandbox:** limits which resources a process can reach using operating-system enforcement and strictly mediated services.
2. **Credential Broker:** mediates narrow operations using secrets or signing identities without disclosing the secret to a model or untrusted agent/tool process.
3. **Operation Policy:** evaluates subject, target, operation, data flow and approval before every consequential operation, including those executed by a trusted deputy.

They are complementary, not substitutes. All existing `secret.use`, KUANG/11 capabilities, typed command registries, v0.4 remote-scope, v0.6 `ChangePlan`, v0.10 context/model egress, history and audit contracts remain authoritative. **This specification adds enforcement and execution semantics rather than a second shell language, plan model or plugin framework.**

The standard security guarantee for a protected session is that the sandbox cannot bypass a denied file/network/process access by launching another program; a broker cannot be induced to use a secret for an unauthorized operation; and no model receives the underlying credential. No claim is made that permitted code or permitted output is harmless, nor that an unrestricted native plugin is isolated merely because its broker calls are checked.

# 1. Goals, User Stories and Non-Goals

## 1.1 Required user stories

- An operator starts a native assistant against one or more explicitly permitted directories, with a separate read-only directory, and no other project or home-directory access.
- An operator starts an ordinary Ono subshell under the same policy; children inherit effective limits.
- An operator runs an external coding agent as an untrusted subprocess, while allowing it to inspect and edit only a workspace and to use narrowly scoped Ono-mediated capabilities.
- An assistant diagnoses a remote service using an SSH identity without access to a private key, an agent socket, or an arbitrary authenticated SSH session.
- A Git-host plugin creates a single permitted issue using a token that never enters the model or sandboxed process.
- An assistant temporarily requests one elevated operation; a trusted human approves exactly that operation outside the assistant execution context.
- A security reviewer can explain the compiled policy, inspect what the runtime actually enforces, and audit each denied or approved action.
- A platform that cannot enforce the requested scope declines protected execution with a clear error rather than quietly dropping restrictions.

## 1.2 Non-goals

- Replacing Docker, OCI, Kubernetes, Firejail, the host LSM, or enterprise identity systems.
- Guaranteeing safety against a kernel compromise, a compromised trusted broker, malicious kernel modules, or a hostile physical administrator.
- Automatically making arbitrary third-party executables safe without checking their access to sockets, environment variables, network and privileged helpers.
- Guaranteeing that a permitted file modification is correct or harmless.
- Enabling unrestricted AI-controlled SSH shells, arbitrary command execution with privileged credentials, or general token issuance in the v0.10 protected path.
- Treating a prompt, model safety instruction, workflow description or plugin metadata as a security boundary.
- Defining a second command grammar or a new AI-specific change-plan or history subsystem.
- Pretending that access to arbitrary remote filesystems can be constrained by a local filesystem-only sandbox.

## 1.3 Scope of guarantees

A protected operation has three separately reported properties:

1. **Resource isolation:** the actual kernel-enforced process envelope.
2. **Broker mediation:** every use of a sensitive identity or delegated capability passes through a trusted, scoped executor.
3. **Data egress governance:** results and context cross a model or remote-network boundary only under approved classification and destination policy.

A user interface MUST NOT conflate these three into one boolean labeled "secure". A deployment MUST publish a capability report declaring which exact guarantees can be enforced for its host and adapter set.

# 2. Existing Architecture and Ownership

## 2.1 Canonical ownership matrix

| Concern | Owner | Requirement |
|---|---|---|
| Shell grammar and verbs | Ono v0.2 | Reuse existing registered `enter`, `inspect`, `get`, `set`, `explain`, `ask`, `apply` semantics; sample syntax is provisional. |
| Sandbox policy representation | Ono Core | One profile schema, effective-scope compiler, validation, policy lineage. |
| Kernel process confinement | Core launcher/adapters | Linux backend with verifiable enforcement; no model-owned policy. |
| Typed tools and capabilities | Ono + KUANG/11 | Registry discovery is not authority; evaluate every invocation. |
| Model, context and egress | v0.10 Core | No raw secrets/context leaks; separate model transport plugin. |
| Credential handles | Existing KUANG/11 `secret.use` | Compatible opaque references, no parallel secret namespace. |
| Credential use mediation | Ono Core trusted broker | Resolve identities outside sandbox; operation-bound authentication. |
| Concrete secret backends | Trusted Core adapter / permitted integration | SSH agent, hardware keys, vaults, keyrings or cloud identities; adapters are not authority. |
| Resource adapter | KUANG/11 typed operation / trusted executor | Enforce operation-specific constraints before/after credential use. |
| Mutations, protection, rollback | v0.6 | Existing `ChangePlan`; separate explicit verification/recovery. |
| Remote identity/scope | v0.4 | Enforce again at remote boundary, not by local pathname tests. |
| Evidence, results and audit | v0.2/v0.5/v0.10 | Correlate, redact and retain provenance; no second history system. |

## 2.2 Trust zones

```text
Operator / trusted approval UI
             |
     Core Policy Evaluator <---- machine-enforced system policy
      /      |       \
     v       v        v
Sandboxed  Credential  Trusted typed
process    Broker       Executor
  |           |            |
  |           +---- Secret backend / SSH agent / HSM
  |                        |
  +-- narrow request API --+---- approved remote target / API

Native Assistant in Core ---- model/context broker ---- KUANG/11 model provider
     (model requests are untrusted intents, not commands)
```

The privileged broker/executor is not in the model's or subprocess's address space. A shared-UID, unrestricted background helper is not automatically a secure boundary; socket authentication, OS process isolation, credential-store access and same-user attacks MUST be covered by the threat model. Core runtime logic exposed to lower-trust subprocesses MUST use distinct authenticated endpoints and least-privilege service identities when the required guarantee demands it.

## 2.3 Policy vs isolation vs implementation

- **Policy** decides whether an action is allowed; a check in an RPC handler is policy enforcement for that RPC only.
- **Kernel isolation** prevents direct syscalls or access to resources outside the published envelope.
- **Trusted execution** holds credentials and performs a *validated* operation; giving a sandbox a raw credential, a raw authenticated socket or unfiltered shell command defeats this arrangement.
- An unrestricted in-process or native-process plugin that can call host syscalls outside a restricted child does not inherit the child's isolation. The runtime MUST reject protected-session use of such a tool unless a suitable isolated adapter can actually enforce the scope.

# 3. Threat Model and Security Assumptions

## 3.1 Adversaries

1. Model output containing hostile or malformed tool intents.
2. Malicious instructions hidden in a log, file, source code, document, web response or plugin-provided domain hint.
3. A compromised external agent process or its child process.
4. A malicious or vulnerable KUANG/11 plugin, native executable, model provider transport or credential adapter.
5. A compromised dependency reachable within the allowed workspace.
6. A confused deputy: a trusted helper induced to fetch credentials or access out-of-scope data.
7. A local same-user process able to probe sockets, environments or secret-store paths.
8. TOCTOU races, symlink/hardlink paths, bind mounts, pre-opened handles and file-descriptor transfer.
9. Data exfiltration through model prompts, network, DNS, Git operations, logs, terminal or approved tool responses.
10. A remote endpoint or SSH service that provides broader permissions than the intended Ono operations.

## 3.2 Assets

- Private keys, API tokens, vault contents, temporary credentials, hardware signing capabilities.
- Workspaces, unrelated personal files, configuration, system files and remote resources.
- Policy documents, approvals, audit integrity, host/connection identities and trusted process interfaces.
- Sensitive data in command output, model context, memory, history, prompt diagnostics and provider transport.

## 3.3 Attacker capabilities and limits

Assume the model or sandboxed process can choose arbitrary input, invoke any tool it discovers, submit malicious paths, spawn children (if execution is allowed), open available sockets, produce symlinks, race filesystem operations and request repeated approvals. Do not assume that the model obeys its instructions. The kernel, correctly configured isolation primitives, authenticated broker boundary, trusted executable images and approval channel are assumed uncompromised for the documented guarantee; if any of those assumptions fails, the guarantee changes and MUST be disclosed.

## 3.4 Explicit residual risks

- A read/write workspace grant permits deletion or corruption within that workspace unless per-operation controls and backup/protection address it.
- A credential that grants a full remote login cannot, by itself, be constrained to a read-only service inspection.
- Any allowed remote endpoint, model provider or export path may leak permitted data; classification and output policy must still apply.
- A plugin running with unrestricted ambient user privileges can evade a broker policy if its runtime is not independently confined.
- A malicious trusted backend, process with equivalent OS rights or compromised kernel can bypass these boundaries.
- External AI clients that require unrestricted internet or raw provider tokens cannot claim Ono's strictly mediated egress guarantee.

# 4. Core Security Invariants

1. Every governed session MUST have an effective scope, a stable session ID, an owner, a policy revision and an enforcement report.
2. Every operation in a governed session - whether an operator command, a subshell or child process, a KUANG/11 plugin call, an automation step or an AI tool call - MUST execute with the intersection of operator, administrator, parent, session, plugin, target, credential and operation authority.
3. Denial in any required layer MUST deny the operation; approvals MUST NOT silently widen unrelated dimensions.
4. Agent-produced text, files and tool results MUST NOT edit system policy, attach credentials or grant capabilities.
5. Profile compilation MUST occur in a trusted process; user-controlled paths or templates MUST NOT become shell command text.
6. All protected child processes MUST inherit or further restrict the parent's actual operating-system confinement.
7. A sandbox MUST NOT receive raw secrets or unrestricted existing secret sockets, including `SSH_AUTH_SOCK`, cloud credential sockets and desktop keyring APIs.
8. **Secret use MUST be operation-bound, not a generic sign/decrypt/token-return service offered to the agent.**
9. Every broker request MUST authenticate the calling session and be authorized for that specific target, operation, parameters and current policy revision.
10. Every significant operation MUST be revalidated just before execution; permission or target drift MUST invalidate a stale grant.
11. Remote access MUST use target-appropriate enforcement; a local sandbox alone is insufficient.
12. Mutation MUST use inherited v0.6 planning/apply/verification/rollback rules where v0.6 applies; agent approval is not plan approval.
13. Model egress and tool-result filtering MUST remain independently enforced.
14. Unsupported enforcement MUST fail closed for protected sessions. A weaker mode MAY be available only through an explicit, conspicuous operator choice, without protected claims.
15. Policy introspection MUST distinguish declared rules, compiled rules, and verified runtime enforcement.
16. Broker audit MUST describe allowed/denied intent and identity references, but MUST NOT include credential material, authentication headers or sensitive request bodies by default.
17. Revocation MUST stop future authorizations immediately and attempt cancellation of in-flight work; irreversible already-completed effects are not claimed to be undone.
18. Plugin installation, model selection and assistant prompt content MUST NOT silently broaden any scope.
19. No implicit credential inheritance to a newly spawned session, tool or child policy unless the explicit effective-scope rule allows it.
20. Network access to models is a separate approved egress path; it MUST NOT be opened for a sandboxed agent solely because the Core assistant has model access.

# 5. Threat-to-Control Traceability

| Threat | Primary control | Secondary control | Must-test |
|---|---|---|---|
| Read `~/.ssh/id_ed25519` | Kernel filesystem boundary | No secret path exposure | G-FS-01/03 |
| Symlink from workspace to outside | Kernel path/FD confinement | Canonicalized trusted broker access | G-FS-04 |
| Inherited pre-opened secret FD | Launcher FD scrubbing | Protected `/proc`, IPC restrictions | G-FS-06 |
| Child exec escapes filesystem | Inherited kernel policy | Scope lineage and tests | G-EX-02 |
| Socket accesses host credential service | IPC isolation and socket filtering | Broker endpoint authentication | G-IPC-01 |
| Arbitrary SSH use of valid key | No raw agent socket; trusted executor | Remote forced command/account policy | G-CRED-03 |
| Broker confused deputy | Typed authorization plus target binding | Narrow prevalidated adapter | G-OP-02 |
| Exfiltrate via HTTP/DNS | Network isolation / egress mediation | Data classification and approval | G-NET-01/03 |
| Cross-scope policy escalation | Intersection and revision binding | Independent trusted approval | G-POL-03 |
| Prompt injection | Typed intent validation, no model authority | Tool budget + mutation plan | G-AI-02 |
| Remote scope bypass | Remote enforcement/attestation | Deny unsafe remote adapter | G-REM-01 |
| Revoked grant reused | One-shot grant, server-side nonce | Revalidation + audit | G-APP-03 |

# 6. Security Profiles and Effective Scope

## 6.1 Common schema

All examples are **proposed contract syntax**, not claims that v0.10 already implements these commands or schema fields. Exact verb/target registration requires a compatibility ADR consistent with the canonical language.

```yaml
apiVersion: ono.security/v1
kind: SecurityProfile
metadata:
  name: project-work
spec:
  filesystem:
    default: deny
    allow:
      - path: "${WORKSPACE}"
        rights: [read, write, create, delete]
      - path: "${SPECS_DIR}"
        rights: [read]
  process:
    exec: allow-listed
    executables: ["/usr/bin/git", "/usr/bin/rg"]
    descendants: inherit
    resourceLimits:
      maxProcesses: 32
      maxRuntimeSeconds: 900
  network:
    direct: deny
    mediatedDestinations: []
  ipc:
    inheritedSockets: deny
    brokerEndpoint: allow
  credentials:
    grants:
      - ref: "credential://ssh/production-readonly"
        destination: "ssh-host://production-web-01"
        operations: ["service.inspect", "log.read"]
        requiresApproval: false
  model:
    egress: mediated-only
    dataPolicy: "system-default"
  mutation:
    requireChangePlan: true
```

The variables above are **resolved by the trusted launcher before enforcement**, must come from explicitly permitted configuration and cannot be expanded by an agent. `~`, globbing and environment substitutions in user-created profiles MUST be normalized or rejected deterministically; path aliases MUST NOT be interpreted by a shell.

## 6.2 Rights are explicit, not just `read-write`

Directory `read` is not identical to listing directory entries, file `read`, metadata inspection or execution. `write` does not imply rename, deletion, truncation, execute or cross-directory movement. The portable policy schema SHOULD enumerate `read`, `list`, `create`, `write`, `truncate`, `rename`, `delete`, `execute` and `metadata` where the backend can distinguish them. For user convenience a profile MAY use `read-only`/`workspace`, but the compiler MUST expand aliases into explicit platform rights and disclose any unenforceable distinction.

`metadata` may not be fully denyable via some Linux filesystem mechanisms. A strict profile claiming metadata secrecy MUST use a stronger view-isolation mechanism or fail. Permission semantics MUST be tested against directory trees, hard links, relative traversal and file creation.

## 6.3 Scope intersection, never union

Let `A` be the operator's existing permissions, `S` the system/organization policy, `P` a parent session, `R` the requested profile, `K` KUANG/11 capabilities, `T` target policy, and `C` a credential's allowed operations. For an operation `o`:

```text
allowed(o) = A(o) AND S(o) AND P(o) AND R(o)
             AND K(o) AND T(o) AND C(o)  [if credential required]
             AND approval_condition(o)
```

Absent layers that are irrelevant to an operation are not implicit denials, but **unknown required permissions are denial**. A child's profile is a restriction of the parent, never a replacement. Multiple allow-lists from independently restrictive authorities are intersected; they MUST NOT be naively appended/unioned to create extra access. Network, remote and credential dimensions are likewise separately intersected.

## 6.4 Profile inheritance and lifecycle

- A child MAY specify a narrower file list, operation list, model policy, endpoint list and TTL.
- Widening requires creating a new trusted session after an independent authorization step; a running confined process cannot lift kernel Landlock restrictions.
- A profile has an immutable ID/version or content digest for the lifetime of a session.
- Policy updates MUST invalidate authorizations not covered by the new policy. Existing running processes that cannot be tightened MUST be terminated and recreated if the policy demands a stricter boundary.
- All session handles, jobs, grants and tool calls MUST include the effective policy digest and owner/session binding.
- Expiration is checked at the broker; kernel restrictions persist even when a metadata deadline expires.

## 6.5 Named defaults

Ono SHOULD expose a minimal set of comprehensible profiles, not an entire policy-language framework:

- `read-only`: trusted launcher restricts files to selected readable paths, no writes or direct network by default; mediated model access only if explicitly configured.
- `workspace`: selected project trees readable/writable as listed, read-only toolchain, no other home-directory view, no direct credential access.
- `custom`: explicit allow-list of files, processes, networks, resources and broker operations.

`none` or `unrestricted` MAY exist for intentionally unconfined local usage, but MUST never be selected as an invisible fallback from a protected profile. A user's current working directory is not automatically a complete write grant unless an explicitly selected profile says so.

## 6.6 Multi-directory rule

An operator MUST be able to grant multiple directories with distinct rights and an independent ephemeral scratch directory. The implementation MUST preserve each path's effective rights even in the presence of nested paths, aliases or hard links. An allowed parent directory that includes a denied sensitive subdirectory MUST be rejected or mounted with a demonstrably effective exception; simple prefix matching is forbidden.

# 7. Sandbox Enforcement

## 7.1 Design principle

Use existing, maintained kernel facilities and simple launchers. Do **not** implement a general-purpose container orchestrator. The security backend is replaceable only behind one canonical scope and enforceability contract. Backend feature detection is mandatory; no implicit weakening.

## 7.2 Linux backend selection

Recommended initial Linux implementation:

1. **Bubblewrap or equivalent mount/user namespaces** for a minimal visible filesystem tree, read-only runtime paths, private `/tmp`, private `/proc` as appropriate, and explicit workspace mounts.
2. **Landlock** for irrevocable additional filesystem restrictions and, if supported and adequate, network restrictions; never claim it restricts hostnames/URLs merely because it can restrict certain port-level operations.
3. **`no_new_privs`, seccomp, reduced capabilities, controlled IPC and process limits** where supported and required by the selected profile.
4. **Dedicated broker/executor process boundaries** for credential and authorized operations; never rely on the sandbox alone for operation policy.

The compiler MUST map the requested rights to the available backend features *before* launch. A backend can be used alone only when the exact policy and guarantee require nothing it cannot enforce. No `--*-try` style option may silently degrade a required security property; all configured mounts/namespace/FD steps must succeed atomically or abort.

## 7.3 Linux-specific limitations that must be considered

- Landlock is an additional access-control layer, not a private view of the filesystem. On its own it does not hide file names and metadata in all cases and cannot retroactively restrict already-open FDs.
- Landlock availability and rights vary by kernel ABI; for example `REFER`/cross-directory rename and `TRUNCATE` arrived after initial filesystem support. Rules for device ioctl, Unix sockets and network operations also vary by ABI. Test the running kernel, not the distro name.
- Landlock network capabilities differ from a policy for exact remote host identities. Exact hostname/IP ownership, DNS re-resolution, redirect handling and TLS peer identity require a mediated network stack or no direct network.
- Mount namespace isolation is only as strong as the mounts and inherited devices/sockets. Binding `/run/user`, host D-Bus, host Docker socket, unrestricted procfs or shared SSH agent into a sandbox can defeat its aims.
- Namespaces and Landlock do not automatically protect an unrestricted Core process or native plugin that remains outside the sandbox.
- Restriction of ptrace, process visibility, signals, shared memory and terminal input must be evaluated; `--new-session` and appropriately restricted PTYs are relevant for terminal-connected programs.

See primary Linux kernel and bubblewrap sources in Section 25. These are backend constraints, not optional implementation notes.

## 7.4 Launch sequence

A trusted launcher MUST do the following, failing before external code executes on any required step:

1. Authenticate owner and load system, user and requested policies.
2. Resolve profile variables, canonical resource identities and available host features.
3. Compile explicit effective scope and strict enforcement plan; compute digest.
4. Validate referenced paths using trusted directory handles and resistant resolution (e.g. `openat2` with appropriate resolution constraints where needed), not agent-controlled string comparison.
5. Prepare a minimal runtime filesystem and required executable/library files, without exposing home secrets or host sockets.
6. Close all unnecessary inherited FDs; sanitize environment, credentials, working directory, PATH, inherited handles and process-group settings.
7. Create isolation boundaries and drop ambient privileges; install mandatory kernel policy before executing untrusted code.
8. Bind only a narrow, authenticated, operation-oriented broker endpoint (if requested); never expose raw credential-manager or generic host-control sockets.
9. Apply resource limits and set process/cancellation ownership.
10. Verify actual runtime enforcement and record a capabilities attestation/report; if verification is impossible for a required claim, abort protected launch.
11. Execute the requested program **without invoking a shell parser implicitly**.
12. Maintain lifecycle, cancellation, cleanup, denial audit and policy-revision checks.

Untrusted executable path resolution MUST happen under a trusted handle, and shell-like command text MUST NOT be inserted into a launcher argument. Commands that intentionally invoke a shell are a separate, explicitly permitted operation, still within enforced sandbox limits.

## 7.5 Paths, symlinks, hard links and TOCTOU

- The policy compiler MUST NOT trust text path prefixes, `realpath` alone, or a one-time `lstat` of an attacker-controlled path as a complete security boundary.
- Kernel-enforced view limits filesystem reachability; broker operations using host privileges MUST independently resolve target identity and scope from trusted handles at the point of use.
- A symlink within an allowed workspace targeting a forbidden path MUST fail to access the target. `..`, mounted aliases, proc-fd traversal, hardlink behavior and rename races need explicit tests.
- Write constraints MUST include truncate, link/rename, create, overwrite, delete and directory metadata side effects as relevant.
- The launcher MUST close privileged pre-opened handles; the broker MUST NOT pass arbitrary host FDs back to the agent.
- If a workspace path can be replaced after grant, identity binding MUST detect that change or ensure the compiled mount/FD remains scoped to the intended object; otherwise reject.

## 7.6 Runtime dependencies and execution

A useful sandbox often needs `/usr/bin`, shared libraries, CA roots, locale files or a temporary directory. The system MUST create an explicitly documented **read-only runtime baseline** that does not grant ambient access to host secrets. Writable runtime files MUST be confined to scratch or declared workspace. Executable allow-lists MUST consider dynamic loaders, interpreters, shebangs, symlinks, user plugins and helper binaries, not only the first `argv[0]` string. If a kernel-level executable whitelist cannot be enforced on the selected backend, the UI MUST NOT claim that it can.

## 7.7 Process isolation, network and IPC

- `exec`, `fork`, new namespaces, ptrace, process signaling and resource limits MUST be mapped to enforceable primitives; unimplementable constraints are errors in protected mode.
- No raw outbound networking by default in `workspace` and `read-only`. If a third-party agent needs an LLM service, it MUST use an explicit trusted proxy/egress rule with precise endpoints and a separate data-policy decision; generic network enablement forfeits the restricted-egress claim.
- DNS is also a network/data channel. An allow-list of HTTPS destinations is not enforced by allowing DNS/ports alone.
- Local sockets (D-Bus, Docker, Podman, credential agents, service managers, systemd, display) are resources and MAY provide escape or delegated authority; default deny unless separately mediated.
- Parent and child process limits MUST persist across exec; termination and cancellation must reap descendants and release grants.

## 7.8 Remote resources are not local directories

A local protected filesystem is not sufficient to restrict a remote SSH session. Local file scoping and v0.4 remote resource scoping are different dimensions. For any remote action the trusted executor MUST enforce destination identity, remote principal, operation/arguments, and remote authorization. If remote enforcement cannot guarantee an operation restriction (for example a full shell under an admin identity), the corresponding `service.inspect`-style scope MUST NOT be claimed.

## 7.9 Platform support policy

The normative first implementation target is Linux. macOS/Windows MAY be supported by platform-specific adapters only after equivalent guarantees are precisely characterized. An unsupported backend MUST yield a structured `SandboxUnsupported` or `SandboxCannotEnforce` error for protected workflows. It MUST NOT silently run unrestricted because Linux is unavailable.

# 8. Credential Broker: Core Contract

## 8.1 Purpose and trust boundary

The Credential Broker is a trusted Core facility, not an assistant/plugin-owned secret store and not a pass-through signing proxy. It resolves existing KUANG/11 opaque `secret.use` references outside the agent sandbox and makes approved *operations* possible through constrained adapters.

The model and agent MAY know a **stable non-secret alias**, such as `credential://ssh/production-readonly`, if its disclosure is permitted. That alias is not the private key, a bearer token, a file path, or a reusable authorization token. **Knowing a secret reference MUST NOT be sufficient to authorize use.**

## 8.2 Basic operation protocol

```text
Agent or tool intent
 -> typed operation ID + canonical target + typed arguments
 -> Core capability and effective-scope policy
 -> bind credential reference to target/operation/session
 -> approval check, if required
 -> trusted executor performs authenticated action
 -> typed result + evidence + redaction + audit
 -> model egress check, if any
```

No API exposed to an untrusted process MAY return a raw secret value, SSH agent FD, unrestricted signing primitive, unrestricted token, generic cookie jar, private-key handle enabling arbitrary signature requests, or authenticated shell handle in the protected profile.

## 8.3 Credential descriptor

```yaml
apiVersion: ono.security/v1
kind: CredentialBinding
metadata:
  name: production-readonly
spec:
  ref: "credential://ssh/production-readonly"
  backend: "ssh-agent"
  permittedSubjects: ["profile:server-diagnostics"]
  destination:
    type: "ssh-host"
    id: "ssh-host://production-web-01"
    hostIdentity: "pinned-host-key-reference"
    principal: "observer"
  allowedOperations:
    - "service.inspect"
    - "log.read"
  approval:
    mode: "none"
  limits:
    requestTimeoutSeconds: 30
    maxUsesPerSession: 40
  audit:
    includeArguments: "metadata-only"
```

The descriptor is a **policy binding**, not a replacement for the existing credential store. The `backend` is resolved by a privileged adapter and need not expose an SDK to arbitrary plugin code. Identity references and registered operation IDs MUST be exact and machine-checkable; no anonymous `*` grants in protected default profiles.

## 8.4 Supported secret types

The core contract MUST accommodate:

- SSH private-key use through a trusted `ssh-agent` or hardware-backed signer.
- Keyring or vault records resolved only inside the trusted executor.
- Short-lived cloud credentials issued for a particular role/action by an approved integration.
- HTTP bearer/API keys injected only into trusted transport requests, never returned to the agent.
- Database credentials used by a trusted query adapter with restricted query capabilities.
- mTLS client identities and hardware-backed signing identities where a backend can enforce the operation/destination boundary.

The contract **does not** require supporting every backend in v0.10. An adapter MUST state its exact enforcement capabilities. If an operation cannot be scoped as declared, authorization fails.

## 8.5 Backend rules

- Backend credentials MUST be stored using already specified secret handling or supported host secret providers, never in a plaintext policy file, terminal history or model messages.
- Backends SHOULD avoid retaining secret material in Core heap beyond required operation time; use opaque handles/OS-backed facilities where possible. Memory zeroization is defense in depth, not a blanket guarantee against process compromise.
- Secret material MUST NOT be written to environment variables, command-line arguments, on-disk temporary files, error strings or audit events for untrusted executors.
- The trusted executor MAY receive a temporary bearer token in its own protected process memory when unavoidable; it MUST NOT share its address space or general IPC endpoint with the sandbox.
- A credential backend returning a raw token to a trusted transport is not the same as returning it to the agent. Contract interfaces MUST preserve that distinction.

## 8.6 Agent-socket and forwarding restrictions

- The original `SSH_AUTH_SOCK` MUST NOT be exposed to a protected agent or generic sandboxed plugin.
- Even when a key is non-exportable, an unrestricted signing service may permit unauthorized logins. Broker requests MUST bind the intended destination and allowed remote operation; `ssh-add -h` destination constraints and `ssh-add -c` confirmation MAY be used as defense in depth but not a replacement for policy.
- Agent forwarding MUST be disabled by default for protected SSH sessions. Port, X11 and other forwarding paths MUST be disabled unless explicitly authorized in a wider profile with truthful impact labeling.
- SSH host identity MUST be validated with trusted host-key configuration/certificates; accepting an unknown host key automatically is forbidden for privileged use.
- A dedicated remote identity with server-side forced commands/restricted service endpoints is strongly preferred over broad login credentials. A forced command that itself accepts unchecked shell text is not an adequate constraint.

## 8.7 Credentials as capabilities, not identity of the model

Permissions attach to the *operator/session/operation*, not to a model brand or conversation. Switching from a local to a remote model MUST re-evaluate data egress and tool policy, but MUST NOT silently grant or alter credential use. A plugin may offer a credential-backed typed operation; it does not gain general credential access because it is installed or because an assistant discovered it.

# 9. Operation Policy and Trusted Executors

## 9.1 Typed operation boundary

Every brokered operation MUST use an operation ID registered in existing typed metadata and a schema-validated argument object; no stringly-typed fallback to `sh -c`, `ssh host "..."`, `curl -H Authorization: ...` or arbitrary local command construction. A generic `ssh.exec`, `credential.sign`, `http.request-with-any-token` or unrestricted `process.run` MUST NOT be offered to a protected agent as a narrow capability.

## 9.2 Decision inputs

A policy decision MUST include at minimum:

```text
subject: session + invoking agent/tool identity + caller UID/OS identity
origin: operator / model / plugin / subprocess
scope: effective profile + parent scope + administrator policy
operation: registered ID + schema/version + classified side effects
target: stable identity + destination + remote principal
arguments: normalized and validated typed values
credential: optional opaque ref + binding constraints
context: autonomy level, ChangePlan identity, model/egress policy
approval: required? bound grant? expires? reused?
revocation: policy, provider, plugin and credential current state
```

A denied decision MUST identify which rule/required capability failed without revealing secret internals. A positive decision MUST be bound to these inputs, not cacheable as a general permission for subsequent arbitrary requests.

## 9.3 Time of check vs time of use

The broker MUST re-check target identity, binding, grant lifetime, policy revision, plugin capability state and applicable ChangePlan immediately before sensitive execution. Data-driven or malicious replacement of arguments between preview and apply MUST invalidate approval. Long-running operations SHOULD periodically re-check revocation where feasible and MUST never reinterpret a changed target as the original approved target.

## 9.4 Trusted executor contract

A trusted executor:

1. Authenticates the broker-issued request and checks replay protection.
2. Resolves the registered typed operation from trusted metadata rather than model text.
3. Validates arguments and target identity before acquiring a credential.
4. Enforces endpoint/remote principal/namespace and operation-specific bounds.
5. Uses the credential only through its approved backend.
6. Applies timeout, size limits, cancellation and output filtering.
7. Produces a typed result (success, partial, error) with provenance and audit correlation.
8. Drops/forgets temporary credential data immediately after its legitimate use.

A trusted executor MUST NOT execute workspace-provided helper scripts, import untrusted workspace code, follow attacker-controlled redirects or call privileged hooks unless they are separately verified and permitted by the operation contract. Plugin-provided executors must run in a trust tier genuinely sufficient for the declared operation; otherwise protected execution is not offered.

## 9.5 Example: SSH service inspection

```yaml
request:
  session: "session://example-123"
  operation: "service.inspect"
  target: "ssh-host://production-web-01/service/nginx"
  arguments: {includeRecentErrors: true}
  credentialRef: "credential://ssh/production-readonly"
```

Expected execution: trusted SSH adapter verifies the server's host key and `observer` identity, invokes a predetermined remotely restricted inspect endpoint or server-side forced command, and returns a typed status/error result. No remote user-supplied shell string is generated and no unrestricted SSH login session is handed to the agent. If the remote account can run arbitrary commands, the adapter MUST NOT claim it only exposes `service.inspect` without an additional enforcement boundary.

## 9.6 Example: Git host issue creation

```yaml
request:
  operation: "issue.create"
  target: "repository://org/project"
  arguments:
    title: "Document service restart behavior"
    bodyRef: "value://sanitized-draft"
  credentialRef: "credential://git/issue-writer"
```

The trusted Git-host adapter MUST enforce allowed repository, operation, argument bounds, token scope and destination TLS identity. It injects the token into its own API transport only. The agent may see the issue identifier and an approved response, never the token or authentication header. A broad token remains dangerous if the adapter is compromised; prefer narrowly scoped, short-lived tokens where available.

## 9.7 Database and cloud operations

Database and cloud plugins MUST implement the same boundary: typed resources, explicit permitted actions, resource scope and credential binding. A read-only database grant MUST be enforced by database server role as well as Ono policy. A cloud role with broad authority MUST NOT be represented as read-only solely because Ono's tool description says so. Least privilege on the remote service is a mandatory independent layer for strong claims.

# 10. Human Approval and Temporary Elevation

## 10.1 No consent by model

A model may request approval, but no model output, plugin-returned text, code review comment or sandbox-provided web UI may grant it. Approval MUST originate from an authenticated trusted operator channel outside the lower-trust execution context.

## 10.2 Approval object

```yaml
apiVersion: ono.security/v1
kind: OperationGrant
metadata:
  id: "approval://one-shot-example"
  session: "session://example-123"
  policyDigest: "sha256:example-policy-digest"
spec:
  operation: "service.restart"
  target: "ssh-host://production-web-01/service/nginx"
  planDigest: "sha256:sealed-changeplan-digest"
  credentialRef: "credential://ssh/production-operator"
  argumentsDigest: "sha256:normalized-argument-digest"
  maxUses: 1
  expiresAt: "2026-11-15T16:35:00Z"
  approvedBy: "operator://authenticated-human"
```

This is illustrative. In production, the grant MUST be nonforgeable, server-side stored or cryptographically authenticated, bound to owner/session/policy/operation/target/arguments/plan, single-use where appropriate, time-limited, revocable and independently validated. Do not use approval bytes that the model can simply replay or edit as a bearer credential. The user interface MUST describe actual destination, principal, side effect, data/export consequence and scope expansion.

## 10.3 Elevation is not an in-place sandbox escape

An already confined process cannot elevate its own filesystem/kernel sandbox. A one-shot elevation is executed by a trusted, separate executor after approval, with specific operation binding; alternatively a **new session** is established after authenticated operator action with an explicit new profile. There is no `sudo`-like bypass inside a running agent's process merely because a UI button was pressed.

## 10.4 Approval with v0.6 ChangePlan

Mutation subject to v0.6 MUST produce and validate the existing `ChangePlan`. An approval is bound to its sealed plan digest, target identities, protection/impact expectations and relevant credential/operation. Any plan revision, stale target or widened effect invalidates the grant and requires a fresh review. The v0.6 recovery sequence remains independently authorized and is not auto-granted by approving apply.

## 10.5 Approval fatigue and budgets

Protected sessions SHOULD have configurable caps on approvals per unit time, retry loops, total broker calls, duration and background jobs. Repeated denied requests MUST not spam the operator into accidental consent. Batch approvals MAY group a finite explicit set of independently auditable operation instances; never an open-ended wildcard.

# 11. Model Context, Data Egress and Results

## 11.1 Separation of secret access and model access

The Core native assistant may be allowed to reason about a remote server's health without learning the SSH credential. Model egress remains governed by v0.10 ModelPolicy and classification. A confidential server log is not safe to upload merely because the broker retrieved it using a protected key.

## 11.2 Allowed result exposure

Trusted executors MUST classify and filter typed results before returning them to a model-visible context or untrusted plugin. The broker may return an opaque `ResultRef` or evidence reference when the content is not authorized for direct disclosure. Nested, untyped and mixed-sensitivity results MUST be recursively reviewed. Error messages, HTTP headers and failed authentication diagnostics are subject to the same filtering.

## 11.3 Channel separation

A protected agent's arbitrary network access must not be enabled for the sake of model inference. Native v0.10 Core can use its own governed model broker and provider-plugin transport through a separate trusted channel. An external agent that requires a vendor API MAY be integrated via a restricted, authenticated, model-policy-aware egress proxy. If its protocol cannot be safely mediated, protected operation MUST be denied or explicitly presented as a weaker, unprotected mode.

## 11.4 No context laundering through other plugins

A plugin must not return an unauthorized file as an apparently benign typed value, copy denied data into allowed notes, or use an authorized credential to fetch unrelated data. Both source authorization and output classification are enforced. A remote fetch performed by a trusted executor is still constrained by its target/operation/data-policy scope.

# 12. Shells, External Agents and Plugins

## 12.1 Native assistant

The generic assistant remains in Ono Core. It receives an effective scope for each turn/session and may invoke only eligible registered typed operations with valid authority. No special "assistant plugin" is required. The model is never given a direct credential handle, host filesystem path containing secrets or raw broker authorization token.

## 12.2 Sandboxed subshell

Conceptual UX (subject to registry/verb ADR):

```text
enter sandbox project-work
inspect sandbox
explain sandbox
leave sandbox
```

A subshell MUST inherit the same kernel restrictions, profile digest and denied-resource guarantees as other child processes. A command that opens an unrestricted `ssh`, `curl`, `docker`, `systemctl --user`, editor extension or other helper MUST be denied unless the profile separately allows the resource and all indirect authority. An apparent directory-limited shell without actual OS restriction MUST NOT be labeled a sandbox.

## 12.3 External agent

Conceptual UX:

```text
enter sandbox project-work
run process -- executable=codex
```

The example is indicative, not a promised current command. The external agent runs as an untrusted process. Its local file access is kernel-restricted; the only approved privileged operation path is a narrow Ono broker endpoint, if the client supports it. Its own network, model credentials, terminal, environment and subprocess behavior must be assessed separately. A third-party process not designed for mediated credential use cannot transparently receive all developer credentials while preserving the guarantee; Ono SHOULD report incompatible capabilities instead of injecting secrets.

## 12.4 KUANG/11 plugins

Existing KUANG/11 typed capabilities MAY extend the assistant. Each plugin capability declaration must distinguish:

- **pure typed data** with no external side effects;
- **sandboxed local execution** under the caller's scope;
- **trusted mediated resource operation** with narrow adapter permissions;
- **unrestricted native-process behavior**, which is not eligible for protected scope unless separately confined.

Core MUST evaluate isolation truth, not manifest promises alone. A plugin's signature proves origin/integrity, not that its runtime is safe. Plugin upgrades invalidate cached eligibility/privilege decisions. Plugin removal or grant revocation cancels future calls and invalidates stale tool refs.

## 12.5 Parent/child and detached jobs

Session scope MUST follow detached jobs, long-running views and child processes. A tool cannot become unrestricted by daemonizing, disowning, sending work to a host service, spawning a nested shell or handing a task to another plugin. Delegated work requires a new explicit broker authorization and a policy-bound job identity. If a host service cannot enforce that delegation, it cannot be used as an escape path for protected execution.

# 13. Network and Remote Security

## 13.1 Network classes

Define separately:

- `direct`: kernel-permitted ordinary outbound/inbound networking from the sandbox (default deny).
- `mediated`: broker-controlled destination and operation with network transport in a trusted executor.
- `model-egress`: v0.10 governed provider transport for prompt/context data.
- `local-ipc`: allowed broker endpoint and explicitly permitted local-only interfaces.

A profile MUST make these distinguishable. A permitted TCP port is not proof of a permitted TLS host. DNS, proxies, IP literal aliases, redirects, Unix sockets, loopback, VPN routes, IPv6 and alternate connection paths must be tested according to each claimed guarantee.

## 13.2 Remote identities

v0.4 place identities are canonical. An operation may reference a host name for usability, but the trusted executor MUST verify the resolved remote identity at connect time (SSH host key/cert, TLS identity, cluster identity, etc.). DNS resolution changes, redirect chains and proxy routing MUST NOT change the authorized destination without reauthorization.

## 13.3 Preventing authenticated tunnel escape

An authenticated remote connection may itself provide a network tunnel, forwarded socket, remote port or command channel. The protected SSH adapter MUST deny forwarding by default and restrict requested remote operations. SSH agent forwarding and raw PTY login are distinct high-risk capabilities; neither is granted by `ssh.authenticate` or `service.inspect`. Broader use requires an explicit non-default scope with honest warnings and, where necessary, remote-side enforcement.

## 13.4 Remote Ono executor

If an Ono remote agent exists, the remote peer MUST independently validate caller/session/operation/target/credential permissions and attest its enforcement capability. Local approval is necessary but not sufficient. Where remote enforcement cannot be confirmed, only operations whose trusted remote API intrinsically enforces the restriction may be used; others fail closed in protected mode.

# 14. Audit, Explainability and Observability

## 14.1 Mandatory correlation

Events SHOULD use existing Ono event types/history and carry correlation IDs covering:

```text
session -> scope compile -> sandbox launch
        -> assistant turn/model decision -> tool intent
        -> operation policy -> credential selection/use
        -> trusted executor -> result classification
        -> ChangePlan/approval/apply/verification (if any)
        -> completion/revocation/cancellation
```

No new competing audit log semantic model is introduced. Sensitive values are represented as opaque references or redacted metadata, not logged in full.

## 14.2 Inspectable enforcement report

A user MUST be able to inspect at least:

| Field | Example meaning |
|---|---|
| Profile | requested `workspace`, parent `read-only` |
| Effective file access | two allowed trees, actual rights, runtime baseline |
| Effective network | direct denied, model broker allowed, no raw proxy |
| Brokered operations | `service.inspect` for one verified remote identity |
| Credential aliases | names only; no key material or socket path |
| Runtime backend | Landlock ABI, namespace setup, feature checks |
| Boundaries actually verified | filesystem yes, host identity yes, remote command restriction yes/no |
| Unenforced/unknown | cannot be hidden under an overall "sandboxed" label |
| Policy lineage | system -> user -> parent -> session + digest |
| Approval state | requested, granted once, expired, revoked |

The report MUST distinguish **declared**, **compiled** and **observed** guarantees. An inspect command MUST NOT include secrets or private authorization tokens.

## 14.3 Denial behavior and errors

Structured error categories SHOULD include:

- `SandboxUnsupported`
- `SandboxCannotEnforce`
- `SandboxLaunchFailed`
- `ResourceOutsideScope`
- `OperationNotPermitted`
- `CredentialNotPermitted`
- `CredentialBackendUnavailable`
- `RemoteEnforcementUnavailable`
- `ApprovalRequired`
- `ApprovalExpired`
- `GrantRevoked`
- `PolicyChanged`
- `TargetIdentityChanged`
- `DataEgressDenied`

Use the existing Ono error representation. Denials MUST NOT automatically trigger retries via a less restricted path. All errors have stable machine-readable codes, human-readable explanations and safe remediation hints.

## 14.4 Privacy of audit data

Audits often contain sensitive target and user information. Access to logs MUST be governed; retention SHOULD be configurable and proportionate. A security failure must be provable without storing raw prompts, HTTP authorization headers, private keys, whole command environments or raw database rows. Audit storage failures MUST have a defined policy: high-risk protected operations SHOULD fail closed if mandated evidence cannot be recorded; lower-risk operations MAY use a documented buffered/limited fallback.

# 15. Policy Persistence, Security and Supply Chain

## 15.1 Profile sources and precedence

System/organization policy constrains user policy; user policy constrains session choice; session choice constrains child scopes. None can override a stronger parent. Configuration provenance MUST be inspectable. Profile files are **data**, never executable code, macros or user-supplied shell expansions.

## 15.2 Protecting the broker

The broker service MUST authenticate callers using OS-enforced peer identity plus session-bound authorization; a UNIX socket's path alone is not authentication. It MUST reject arbitrary FD passing, bogus session IDs, replayed approvals, requests from unbound processes and arbitrary endpoint redirection. The broker MUST not load shared libraries or executables from writable workspace paths, nor read policy from paths editable by the confined agent.

A same-UID attacker may be able to inspect another same-UID process or influence shared environment unless hardening is adequate. The implementation MUST substantiate its protection using separate process privileges, kernel policy, credential access separation or explicitly documented limitations; it MUST NOT claim a stronger isolation tier than is enforced.

## 15.3 Secret storage

Existing `secret.use` stores and KUANG/11 backend semantics remain canonical. Do not create a second vault merely for this feature. System keyrings, SSH agent, hardware tokens, approved vault providers and short-lived cloud identity systems can be integrated by trusted adapters under the same broker rules. Backends that require exposing raw secrets to sandboxed code are unsuitable for strong protected mode.

## 15.4 Policy changes and signed plugins

Updates MUST be atomic, versioned and logged. Signed plugins still require least-privilege execution; signature is not proof of safety. Cache keys for operations must include plugin schema identity, policy digest, target identity, backend capability version and credential binding revision where relevant.

# 16. UX and Command Design

## 16.1 Command integration principles

Reuse existing verbs and typed targets. The following is a **proposed UX contract for implementation design**, not finalized command syntax:

```text
get sandbox
inspect sandbox project-work
explain sandbox project-work
enter sandbox project-work
leave sandbox

get credential-binding
inspect credential-binding production-readonly

ask assistant "Inspect the failing service"
```

No new top-level `sandbox ...` or `secret ...` mini-shell should be introduced solely for security. The target registry, completion, `help` and introspection MUST derive from machine-readable schemas. A credential alias may be shown in explanation, but raw secret values never are.

## 16.2 Preview before use

Before a protected session starts or a broader operation is approved, the UI SHOULD summarize file paths, read/write distinctions, network exposure, processes, credentials, remote targets and known limitations in a compact view. A detailed `inspect` path MUST always be available. Non-interactive invocations MUST not prompt unexpectedly; missing grants/approval produce structured failure.

## 16.3 Permission requests

Permission prompts MUST be concrete: "Read logs on production-web-01 with observer identity" rather than "Allow SSH"; "Write files in ~/repo/foo" rather than "Grant filesystem access". A request to change a credential target, writable directory, model egress boundary or remote principal MUST be displayed as a meaningful scope increase, never disguised as routine continuation.

## 16.4 Explicit weaker mode

A deliberate `unrestricted` mode for experienced users MAY exist, but it MUST NOT look identical to protected mode, be triggered silently by backend incompatibility, or be selectable by the model. A test suite MUST verify that all downgrade and fallback paths require explicit trusted user action.

# 17. Detailed Reference Flows

## 17.1 Workspace-only source review

1. User selects `/work/ono-sendai` read-write and `/work/specs` read-only.
2. Launcher compiles profile and creates a fresh, minimal filesystem view; other home paths are absent.
3. Core assistant asks for typed repository information or an external `rg` tool, both within scope.
4. If the agent tries `/home/user/.ssh`, the kernel denies direct access.
5. If it asks a plugin to read that file, the broker denies the out-of-scope typed operation.
6. If a model request is permitted, only context passed through the egress policy reaches the model.
7. `inspect` shows effective mounts, backend features and denied operations.

## 17.2 Read-only server diagnosis with SSH identity

1. User chooses `server-diagnostics` and `credential://ssh/production-readonly`.
2. Agent requests `service.inspect` on registered remote target.
3. Core checks session, target, operation, credential binding and server host identity.
4. Trusted executor uses a credential backend without exposing a signing socket or key to sandbox.
5. Remote endpoint or forced command enforces the exact operation; untrusted SSH text never becomes arbitrary command content.
6. Result is classified, bounded and returned with evidence. Model egress applies separately.
7. Attempt to run arbitrary `ssh` or `scp` is denied unless the operator explicitly authorized a different, broader mode.

## 17.3 Git issue with credential

1. Agent drafts issue title and description; no token is available to its process.
2. Core checks `issue.create`, approved repository, arguments, request size and account binding.
3. If approval is required, the human reviews the exact repository and payload digest.
4. Trusted HTTP adapter sends the authenticated request, subject to destination/TLS constraints.
5. The result returns issue number and safe metadata, not the bearer token.
6. Retrying with a different repository or altered body invalidates a one-shot approval.

## 17.4 Temporary service restart

1. Investigation recommends a restart but the current session is read-only.
2. Core creates the normal v0.6 `ChangePlan`; mutation is not yet executed.
3. User reviews verified target, risk, protection, exact operation and credential identity.
4. Trusted UI issues a single-use, plan-digest-bound authorization.
5. Trusted executor revalidates policy, target and plan and applies only the approved restart.
6. Verification produces evidence; recovery requires its own authorized plan where applicable.
7. Agent's originally sandboxed process never gains admin shell rights.

## 17.5 Sandbox backend unavailable

1. User requests strict filesystem isolation.
2. Backend preflight determines required kernel feature is unavailable or user namespaces are disabled.
3. Ono reports exact missing enforcement and refuses protected launch.
4. It does not silently run the same agent unrestricted, remove disallowed directories from a report while permitting them, or replace kernel isolation with prompt instructions.
5. A human MAY start a separately labeled weaker/unprotected session after explicit action, where supported by policy.

# 18. External-Agent Compatibility Rules

## 18.1 Standard integration levels

- **Level A: Confined standalone process.** Filesystem/process/network restrictions enforced; no credential broker use unless explicit adapter support.
- **Level B: Confined process with typed broker tools.** The agent can request approved operations through a narrow IPC/API bridge; secrets remain unavailable.
- **Level C: Full governed model egress.** Agent model traffic also crosses a compatible, tested Ono context/egress proxy with enforceable remote destinations and classification.

These levels are independent of KUANG/11 isolation-tier labels. They MUST describe actual enforcement, not an implied marketing hierarchy. A client may be compatible with Level A but not Level C. Ono MUST present that distinction before execution.

## 18.2 No transparent arbitrary-SSH promise

An existing external agent that runs arbitrary `ssh` commands cannot automatically obtain the same guarantees as typed broker operations. Providing `SSH_AUTH_SOCK` or a transparent unrestricted SSH proxy would weaken the boundary. A deliberately limited SSH wrapper MAY be supported only if it can validate every operation and the remote side enforces constraints. Compatibility must be demonstrated with adversarial tests, not inferred from common CLI usage.

# 19. Performance, Resource and Reliability Requirements

- Scope compilation MUST be bounded by configured maximum path/rule counts. Path traversal and symlink resolution must have limits.
- An ordinary denied filesystem access SHOULD be resolved by kernel policy without a userspace RPC per byte or per syscall.
- Broker operations MUST support cancellation, deadline, output size limits and a finite per-session request budget.
- Broker authorization decisions SHOULD be fast enough for interactive tools, but performance MUST NOT justify bypassing identity, policy revision or credential-binding checks.
- No default global lock may serialize unrelated sessions if a narrower lock can protect the relevant credential backend safely.
- Cleanup MUST close credential handles, wipe temporary payloads where feasible, kill/reap terminated descendants and revoke session grants.
- Broker outage MUST deny new credential operations; it MUST NOT cause raw secret fallback or unrestricted local tool execution.
- Deterministic test fixtures SHOULD cover nested profiles, revoked grants and hundreds of small tool requests without unbounded state growth.

# 20. Implementation Architecture

## 20.1 Suggested Core modules

```text
ono-core security/
  policy_schema             (data types and validation)
  effective_scope           (intersection and digests)
  enforcement_capabilities  (host feature probing)
  sandbox_launcher          (trusted process + backend abstraction)
  credential_broker         (authorization + opaque reference resolution)
  operation_policy          (typed operation decisions)
  approval_grants           (trusted human authorization)
  trusted_executor          (bounded, audited operation execution)
  audit_bridge              (existing Ono history/audit integration)

platform/
  linux_landlock            (optional additional restriction)
  linux_namespaces          (e.g. bubblewrap-backed launcher)
  linux_process_controls    (no_new_privs, seccomp, limits)

kuang11/
  existing typed plugin contracts + isolation tiers
  approved backend/executor contributions
```

These are suggested module boundaries, **not** permission to create duplicate command registries, a second vault or a second audit system. Exact crate ownership requires an ADR and repository architecture review before coding.

## 20.2 Boundary APIs

A minimal internal contract SHOULD resemble:

```rust
// Pseudocode, not a finalized Rust API.
struct EffectiveScope { id: ScopeId, digest: PolicyDigest, /* ... */ }
struct EnforcementReport { requested: Vec<Rule>, enforced: Vec<Rule>,
                           unsupported: Vec<Rule>, /* ... */ }
struct OperationIntent { session: SessionId, operation: OperationId,
                         target: TargetRef, arguments: TypedValue,
                         credential: Option<CredentialRef>, /* ... */ }
struct PolicyDecision { permitted: bool, reason: DecisionCode,
                        required_approval: Option<ApprovalSpec>, /* ... */ }

trait SandboxBackend {
    fn probe(&self) -> BackendCapabilities;
    fn compile(&self, scope: &EffectiveScope) -> Result<EnforcementPlan>;
    fn launch(&self, plan: EnforcementPlan, executable: TrustedExecutable)
        -> Result<ConfinedSession>;
}

trait CredentialExecutor {
    fn invoke(&self, approved: AuthorizedOperation)
        -> Result<TypedExecutionResult>;
}
```

A successful compile MUST NOT imply a successful launch; actual enforcement must be verified. `AuthorizedOperation` must be unforgeable from untrusted inputs, tightly bound to the current policy and valid only for the intended executor and invocation. A typed `Value` returned by an executor must carry data classification and provenance.

## 20.3 ADRs required

At minimum, document:

- ADR-S1: Scope schema and canonical ownership relative to KUANG/11 capabilities.
- ADR-S2: Linux enforcement backend(s), minimum supported kernel features and backend guarantee matrix.
- ADR-S3: Broker privilege boundary, same-user process threat model and authenticated IPC.
- ADR-S4: Credential binding protocol and trusted executor eligibility; why raw signing and raw SSH are excluded.
- ADR-S5: Human approvals, policy revision, ChangePlan binding and audit integration.
- ADR-S6: v0.10 assistant/typed-tool integration and backward-compatible `secret.use` semantics.

No renumbering of historical ADRs is implied; these are temporary identifiers pending normal repo allocation.

# 21. Phased Delivery and Dependency Plan

## Phase S0 - Contract audit and formal threat model

- Inspect existing v0.2 KUANG/11 permission/secret types, v0.4 remote object identities, v0.6 planning, v0.10 assistant bridge and current plugin isolation tiers.
- Resolve incompatible claims by ADR; document exactly what existing native plugins can bypass.
- Decide which protected guarantees are v0.10 release blockers and which broader adapter types are later work.
- Produce a source-to-test traceability matrix and prototype attestation schema.

**Exit:** architecture ADRs approved, trust boundaries and supported guarantee matrix documented.

## Phase S1 - Policy compiler and introspection

- Implement immutable profiles, strict schema/version validation, inheritance/intersection, digests, expiry and structured errors.
- Integrate canonical typed operations/capabilities and target identities.
- Expose inspect/explain from the current command registry with no parallel parser.

**Exit:** deterministic policy evaluation, denial provenance and no widening of parent policy.

## Phase S2 - Strict Linux sandbox

- Feature-detect Linux backend capabilities, implement one enforceable workspace/read-only profile and a minimal trusted launcher.
- Sanitize FD/env/IPC/process state; confine child processes; control network; expose honest runtime attestation.
- Integrate cancellation, resource bounds and hostile path tests.

**Exit:** mandatory filesystem/network/child isolation gates pass on a documented supported Linux configuration; unsupported hosts fail closed.

## Phase S3 - Broker and credentials

- Reuse existing secret refs; add authenticated broker IPC, bindings, approvals and trusted executor interface.
- Implement one narrow SSH-backed read-only operation or equivalent credential-backed reference adapter with independent remote-side restriction.
- Verify no secret/socket/FD transfer to untrusted process and complete audit correlation.

**Exit:** end-to-end credential-mediated operation with no raw credential exposure and no arbitrary command capability.

## Phase S4 - Native assistant and KUANG/11 mediation

- Bind every v0.10 Assistant request to an effective scope and tool grants.
- Exclude unconfined plugins from strict sessions; enforce tool-result classification, egress and ChangePlan lifecycle.
- Add safe broker discovery and optional narrow capability contributions from normal KUANG/11 plugins.

**Exit:** an adversarial model/tool cannot bypass profile via plugin, model provider or a trusted deputy.

## Phase S5 - External process integration

- Add launch of ordinary subshell and one external agent/client in Level A, with Level B only when protocol integration supports controlled operations.
- Assess model network protocol compatibility separately; do not claim Level C by default.

**Exit:** repeatable on-host external-process tests, truthful compatibility reports and no secret socket exposure.

## Phase S6 - Hardening and documentation

- Fuzz input/policy/IPC parsers; test kernel/backend compatibility and plugin lifecycle revocation.
- Run malicious tool/plugin, symlink, FD/IPC, SSH forwarding, network exfiltration and stale-approval tests.
- Publish security operations manual, failure guides, threat model and supported-platform matrix.

**Exit:** mandatory gates green, known residual risks documented, reviewer can reproduce security claims.

# 22. Release-Blocking Acceptance Gates

Gates named here are **security-contract gates**. They do not replace v0.10's existing functional gates; v0.10 must reference the applicable gates and include reproducible evidence in its release check. The G-AI gates are the integration gates of an AI consumer of these mechanisms, today the v0.10 native assistant; every other gate is consumer-neutral and holds equally for shells, subshells, plugins and automation.

| Gate | Criterion |
|---|---|
| G-POL-01 | Profiles are data; malicious YAML/strings cannot cause command execution or policy alteration. |
| G-POL-02 | Two independent allow-lists intersect, never silently union. |
| G-POL-03 | Child cannot widen any required parent/system constraint. |
| G-POL-04 | Policy/target/plugin version change invalidates cached authorization. |
| G-FS-01 | Restricted process cannot read private key outside allowed trees. |
| G-FS-02 | Read-only tree denies write, truncate, rename, delete and create as requested. |
| G-FS-03 | Workspace process cannot read sibling project or unrelated home paths. |
| G-FS-04 | Symlink, `..`, bind alias and path race cannot escape restrictions. |
| G-FS-05 | Filesystem metadata and hardlink behavior match published guarantees. |
| G-FS-06 | Inherited FDs, `/proc` and FD passing do not expose disallowed files. |
| G-EX-01 | Backend feature gaps cause explicit failure before untrusted exec. |
| G-EX-02 | All children and subshells retain mandatory kernel restrictions. |
| G-EX-03 | No unrestricted host binary/helper/loader or shell fallback bypasses scope. |
| G-IPC-01 | Host credential/privileged sockets unavailable in protected session. |
| G-IPC-02 | Broker rejects spoofed peer/session, replay and unauthorized FD transfers. |
| G-NET-01 | Direct network/DNS denied in strict default profile. |
| G-NET-02 | Mediation enforces TLS/SSH destination identity across redirects/races. |
| G-NET-03 | Allowed model egress cannot be reused as arbitrary sandbox networking. |
| G-CRED-01 | Private key/token not present in model context, agent env, argv, FD, filesystem, stderr or audit. |
| G-CRED-02 | Credential reference alone grants no use; request is subject and scope bound. |
| G-CRED-03 | SSH key cannot be used for unauthorized destination, forwarding or raw shell. |
| G-CRED-04 | Trusted executor does not return token or unrestricted signer to agent. |
| G-CRED-05 | Credential backend outage fails closed; no raw-secret fallback. |
| G-OP-01 | Unknown operation ID, invalid args or untrusted shell text is rejected. |
| G-OP-02 | Trusted adapter cannot be induced to act outside target/operation constraints. |
| G-OP-03 | Read-only intent cannot trigger a mutation via hidden side effects. |
| G-APP-01 | Agent and plugin text cannot self-approve or alter system policy. |
| G-APP-02 | Mutation approval bound to sealed v0.6 ChangePlan and exact target. |
| G-APP-03 | One-shot, expired, revoked, modified or replayed grant is rejected. |
| G-APP-04 | Approval UI and grant issuance are inaccessible to the model, sandboxed child and untrusted plugin. |
| G-REM-01 | Remote restrictions enforced remotely or operation denied. |
| G-AI-01 | Built-in assistant exists independent of optional plugins and uses effective scope. |
| G-AI-02 | Prompt injection cannot add tool or secret capability. |
| G-AI-03 | Model egress and tool-result classification run after broker execution. |
| G-AI-04 | Unrestricted plugin cannot falsely participate in protected tool execution. |
| G-AUD-01 | Correlated audit identifies decisions without leaking credential material. |
| G-AUD-02 | Inspect/explain reports actual enforceability and unknowns honestly. |
| G-LIFE-01 | Cancellation/revocation stops future broker use and cleans descendants. |
| G-EXT-01 | External-agent compatibility level reflects real confinement, credential mediation and model-egress enforcement. |

## 22.1 Which gates block v0.10?

For v0.10's **native assistant with protected tool use**, S0-S4 and all gates applicable to the enabled v0.10 reference flows are blockers. In particular G-POL, G-FS, G-EX, G-IPC, G-NET, G-CRED, G-OP, G-APP, G-REM, G-AI and G-AUD cannot be waived where the feature claims those guarantees. To keep v0.10 feasible, the first reference scope MAY support only Linux strict workspaces and one proven credential-backed typed SSH/remote inspection path. Unsupported secret backends and arbitrary third-party agents MUST remain unavailable rather than faking enforcement.

S5 external-agent integration can ship separately, unless v0.10 explicitly markets protected external-agent execution. If it does, G-LIFE and all Level A/B claims become release blockers for that advertised mode. Level C governed external model egress is not a default v0.10 promise without a compatible tested proxy.

# 23. Test Matrix and Reproducible Evidence

## 23.1 Mandatory test layers

1. **Policy unit/property tests:** profile parsing, empty/unknown rights, intersections, revocation, stale digests, bounded rule counts, malformed op IDs and approval replay.
2. **Kernel integration tests:** temp directories, sibling home, symlinks, hard links, rename, creation, truncation, process exec, multiple nested scopes, FD passing and network/IPC.
3. **Trusted adapter tests:** SSH host identity mismatch, false remote principal, forwarding attempts, arbitrary command substitution, Git redirect/TLS mismatch, token leakage.
4. **End-to-end assistant tests:** native assistant tool intents, prompt injection in log text, model fallback, data class propagation, denial messaging and ChangePlan approval.
5. **Host compatibility tests:** at least one supported baseline kernel and one intentionally unsupported/missing-feature environment; unprivileged user namespace disabled case.
6. **Abuse tests:** attacker-controlled cwd, environment, PATH, `/proc`, D-Bus, containers, leftover processes, reconnect, detached jobs, broker restart and policy revocation.

## 23.2 Evidence requirements

Each gate MUST have an automated or explicitly reviewable reproduction command, expected denied/allowed operation, actual backend/ABI environment, result and associated build/commit/ADR. Tests claiming a kernel property MUST run with the actual security backend active; mocking policy evaluation alone does not establish sandbox enforcement. Test reports MUST label skipped tests and unsupported hosts rather than counting them as passes.

## 23.3 Mutation safety verification

For any writable workspace, demonstrate that denied paths remain inaccessible but permitted edits work; verify intended writes cannot change unrelated paths via symlink/hardlink or trusted executor. For remote mutations, validate ChangePlan identity, protection, approval and verification independently of the model's narrative. Recovery should be tested as a separate authorized sequence.

# 24. Decision Ledger and Open Architecture Choices

| Decision | Status | Rationale / bounded question |
|---|---|---|
| Native assistant remains in Core | Fixed | Generic semantics should not require assistant plugin. |
| Model implementations via KUANG/11 | Fixed | Avoid vendor coupling in Core. |
| Existing typed plugin operations may extend assistant | Fixed | Avoid a second AI tool framework. |
| Generic sandbox policy in Core | Fixed | Useful for shells, tools and external agents. |
| Credential references reuse `secret.use` | Fixed | No new vault/secret naming model. |
| Trusted broker never exports secret to agent | Fixed | Necessary for "use without knowledge" promise. |
| Raw authenticated shell from a narrow broker call | Rejected | Cannot enforce claimed operation boundary. |
| Linux first, fail closed on unsupported backend | Fixed | Truthful enforcement over platform parity. |
| Landlock plus namespace-based view as baseline | Proposed | Verify against supported kernels and security review. |
| Broker IPC privilege separation | ADR required | Shared-UID and native plugin attack surface must be proven. |
| Precise stable policy/approval schema fields | ADR required | Fit canonical Ono types. |
| One reference SSH operation in v0.10 | Proposed minimum | Demonstrate real credential mediation without giant adapter catalog. |
| External-agent Level C support in v0.10 | Deferred by default | Requires dedicated protocol-aware egress enforcement. |
| Hardware/vault/cloud backends | Incremental | Implement only with proven trusted adapters. |

## 24.1 Criteria for architecture acceptance

Do not approve a design merely because it runs a demo. A reviewer must be able to answer: who holds the key; who owns the signing socket; which process can access the underlying file; how an agent could route around a denied tool; which operations the remote service actually allows; how policy changes affect an already running process; whether approved output can leak data to the model; and which checks are performed by the kernel versus trusted Ono code.

# 25. Primary Technical Sources

The links below support platform assumptions and must be re-checked when deciding minimum supported kernels/versions. They do **not** replace implementation threat modeling or runtime tests.

1. Linux Kernel, **Landlock: unprivileged access control**, including current ABI rights, limits and pre-opened FD caveats: <https://www.kernel.org/doc/html/latest/userspace-api/landlock.html>.
2. Linux Kernel, **No New Privileges Flag**: <https://kernel.org/doc/html/v6.15/userspace-api/no_new_privs.html>.
3. Bubblewrap, **README: sandbox security, namespaces, mounts, IPC caveats**: <https://github.com/containers/bubblewrap/blob/main/README.md>.
4. OpenSSH, **ssh-add(1)**: destination constraints (`-h`), confirmations (`-c`), lifetimes (`-t`), and agent limitations: <https://man7.org/linux/man-pages/man1/ssh-add.1.html>.
5. OpenSSH, **sshd(8)**, including `authorized_keys` `restrict`, `command=` and forwarding options: <https://man7.org/linux/man-pages/man8/sshd.8.html>.
6. Linux Kernel, **Landlock security model and fd semantics**: <https://www.kernel.org/doc/html/latest/security/landlock.html>.

# 26. Definition of Done

This specification is ready for implementation when: (1) its ownership boundaries and ADRs are accepted; (2) the supported Linux baseline and exact guarantee matrix are frozen; (3) a strict sandbox runs a workspace-only subshell/agent with verified syscall-level restrictions; (4) a trusted broker performs a narrow credential-backed operation without handing agent/process/model the secret or signer; (5) typed operation permissions, v0.6 plan approval and v0.10 data egress are integrated; (6) denial/revocation/remote enforcement paths are evidenced; and (7) all advertised protected-mode gates pass without hidden skips.

**Product statement:** Ono does not merely ask an agent to respect a boundary. For its supported protected modes, Ono enforces the boundary and can demonstrate how.
