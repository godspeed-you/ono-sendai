# ADR-0848: A plan inside a link freezes through the link's providers

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §7.1, §14.4 (v0.2), §29.1–§29.4, §34.2, §55.10 cases 44–46, §63 item 15,
  Appendix B, Appendix F.2, Appendix I.3; supersedes ADR-0844
- Decided by: agent (autonomous)

## Context

ADR-0844 refused `plan` inside an entered link because nothing in the change path carried a host.
`scripts/release-check.sh` admits no open box, and five §4.12 boxes need §29 through the shell.

The v0.4 link is an `ono --agent` process — spawned over a pipe pair by `--transport local`, reached
over TLS by `--transport tcp` — serving its own provider registry. The protocol carries provider
queries, subscriptions and single actions (`StartQuery`, `StartSubscribe`, `Act`); it runs no
pipeline and no command on the far side. Records crossing it are re-tagged with the host
(`ono-remote/src/retag.rs`). Inside `enter link H` the session routes provider calls to H's
registry (`Session::pipeline_context`), which `apply` already used while `plan` read the local one.
The change layer's other inputs are this machine's: canonical paths and the mount table
(Appendix B), and the three recovery providers.

## Decision

1. **`plan` inside `enter link H` freezes through H's providers.** Every subject is resolved
   through the registry the link frame routes to, and every frozen target records `host = H`
   (§7.1). A plan in this build addresses one host. File and directory targets are refused inside
   a link with `change.action_not_plannable`: a canonical path and its persistence domain are
   resolved against this machine's filesystem and mount table (Appendix B), and the link carries no
   filesystem resolution. Services, packages, processes, interfaces, routes and every other object
   a provider names plan as they do here.
2. **Protection is per host (§29.2).** The coverage analysis of a remote plan asks no local
   recovery provider about a remote target; the host's rows are composed by
   `ono_change_protection::hosts::compose`, and the matrix names the host. No recovery provider of
   this build runs on the far side of a link, so a persistent domain there is unprotected and says
   why; a runtime domain keeps the compensation its provider declares, which runs over the same
   link.
3. **A remote plan runs where its host is.** `apply`, `protect`, `verify`, `resume` and `recover` of
   a plan whose targets carry host H run only while the session stands inside `enter link H` with
   the link connected, because that frame is what routes provider calls there (§14.4). Elsewhere
   they refuse by name and change nothing. A plan whose targets carry no host refuses inside a link:
   its actions would otherwise run on the linked host.
4. **A dropped link leaves the outcome unknown (§29.3, Appendix F.2).** An action on a remote target
   whose transport fails settles `unknown`; the executor records `remote-disconnect`, the plan stays
   `APPLYING`, and `resume` asks again once the link is back.
5. **Recovery is planned per host (§29.4).** `recover` of a remote plan answers through
   `ono_change_recovery::plan_hosts`: the host holds no recovery asset of this build, so its
   fragment says recovery cannot proceed there and why, and a host whose action outcome is unknown
   carries `change.remote_state_unknown`.
6. **The active link is an input to risk (§34.2, Appendix I.3).** Each connected network link of the
   session (`tcp`, `ssh`) is the `ActiveLink` of the risk assessment, with the interface the kernel
   routes its address through — `lo` for a loopback address — so a plan that stops or changes that
   interface or route is CRITICAL.

## Consequences

Cases 307 and 308 run with `--transport local` and loopback `--transport tcp` links inside the
container and lose their declared skips.

A plan addresses one host, because the shell has no path that brings objects of several links into
one pipeline; §29.2's twenty-host composition is `hosts::compose`'s, proven by
`crates/ono-change-protection/tests/remote.rs`, and it is the function a single-host plan's matrix
goes through too. The refusals reuse the registry: a remote plan outside its frame answers
`change.precondition_failed` — §7.2's `provider-available` precondition does not hold where the
host's providers are not the ones routed to — naming the fact `host`, what was expected and what
was found; a frame whose link is down answers `remote.unreachable`.

## Alternatives considered

Delegating the whole change command to the agent. Rejected: it needs a remote session with its own
plan store and settings, a statement-execution message and a new authorization model, and plans
stored over there would be invisible to this shell's `get plan` and ledger.
Keeping the refusal as a recorded exclusion. Rejected: the release gate admits no open box, and the
link layer carries everything §29 needs except remote recovery providers, whose absence the matrix
states.
