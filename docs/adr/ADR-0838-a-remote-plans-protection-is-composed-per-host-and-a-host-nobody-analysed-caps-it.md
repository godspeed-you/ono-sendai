# ADR-0838: A remote plan's protection is composed per host, and a host nobody analysed caps it

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §29.1, §29.2, §29.3, §10.2, §10.3, Appendix A.5, Appendix A.7, §55.10 case 44
- Decided by: agent (autonomous)

## Context

§29.2's worked example is a twenty-host plan: twelve hosts ZFS-protected, six Btrfs-protected, two
unprotected, and the plan-level status `PARTIALLY_PROTECTED` "unless policy excludes the
unprotected targets". The coverage algorithm of Appendix A analyses one host — one mount table,
one provider registry — and nothing composed the analyses of several. The recovery side already
plans per host (`ono_change_recovery::plan_hosts`, §29.4). This build has no remote change path:
no link carries a change provider, and case 307 proves that no plan here can be about another host.

## Decision

`ono_change_protection::hosts::compose(&[HostCoverage], &ProtectionPolicy) -> ProtectionSummary`.
Each host is analysed on its own terms and arrives as a `HostCoverage`: the host's own summary, or
the reason its protection could not be analysed. `compose` puts every host's rows into one matrix
and lets `ProtectionSummary::level` compute the plan-level word, so one composition rule serves one
host and twenty.

- A host the policy excludes (`ProtectionPolicy::excluding_host`) keeps its rows, marked
  declared-irrelevant, and gains a plan-level exclusion that names it (§10.3).
- A host whose protection could not be analysed — its link down at plan time — contributes a
  `remote-system` row with an unknown objective and unknown protection, so it caps the plan by
  Appendix A.7 and the reachable hosts never speak for it (§29.3).
- A host's plan-level exclusions travel through the new `ProtectionSummary::plan_exclusions`, and
  its rows bring their own, so every exclusion reaches the composed matrix once.

## Consequences

§29.2 holds wherever per-host analyses exist. `compose` has no caller in the shell until a remote
change path exists, which is the standing `plan_hosts` already has; that path calls it. Each host's
own word stays available through `HostCoverage::level`. Tests:
`crates/ono-change-protection/tests/remote.rs` (seven).

## Alternatives considered

A host field on `DomainCoverage`. Rejected: every place the matrix is keyed — the digest, the
renderer, the store — would need host-qualified rows, while `HostCoverage` keeps each host's
summary whole for a per-host view. Composing in the CLI. Rejected: the rule belongs beside the
single-host rule it reuses.
