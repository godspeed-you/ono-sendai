# ADR-0836: Appendix H's profiles are selectable, and they only tighten

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 Appendix H, §17, §19.4, §40.3, §53; ADR-0810, ADR-0815
- Decided by: agent (autonomous)

## Context

`ono_change_protection::policy::Profile` expanded Appendix H's four presets into values, and
nothing could select one: the profiles were display text, which the user's rules forbid.

## Decision

`change.profile` (an extension beside §53's keys) selects `interactive`, `cautious`, `fleet` or
`scripted`; the default is `none`. `ChangeSettings::read` applies the chosen profile last and only
tightens (ADR-0810): the stricter protection mode, the longer retention, the stricter free-space
floor, opaque actions off, and both risk acknowledgements required — every profile's risk gate is at
least `high+`, so a profile brings back an acknowledgement configuration had switched off.
`cautious`'s `moderate+` gate is enforced at `apply` with `--accept-risk`. `scripted` never prompts:
every gate needs its flag (§40.3, H.4). An unknown name is reported like any unreadable key, and no
profile applies. Fleet's strategy is the default a plan takes when neither `--strategy` nor a written
`change.default_strategy` says otherwise. `ChangeSettings::profile_expansion()` is the inspectable
expansion (H's opening line), shown by `get config --profile`.

ADR-0815's narrowing is reported only when the mode in force does not contain what the plan asked
for (`maximize` under `require`); a stricter mode than requested is reported as raised.

## Consequences

Tests: `crates/ono-cli/tests/change_profiles.rs`, `crates/ono-change-protection/tests/settings.rs`.
Under `cautious` a plan of UNKNOWN risk — which ADR-0806 ranks above MODERATE — needs
`--accept-risk`, which is what the profile asks for.

## Alternatives considered

A profile as a mode of its own. Rejected by Appendix H's first sentence: profiles are presets over
explicit policy and must not hide semantics.
