# ADR-1042: A listening agent reads the configuration the shell reads, and refuses what it cannot

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.4.1 §2.3, §11.2, §12.1–§12.4, §52.2, §55.1, §55.2, §59.5; ADR-0010, ADR-0094,
  ADR-0456, ADR-0461, ADR-0504
- Issues: #190
- Decided by: agent (autonomous)

## Context

`limits.remote_connections`, `limits.remote_pending_handshakes`,
`limits.remote_connections_per_client` and `limits.remote_handshake_timeout_ms` are ordinary
settings: declared in the catalogue, range-checked by `Settings::assign`, listed by `get config`
and reported by `inspect limits`. ADR-0504 §5 had `ono --agent --listen` read them from the
**environment layer only**, because loading `config.ono` needed a `Session` and agent mode had
never built one. Its Consequences named the result: an operator who set a ceiling in
`config.ono` got Appendix A's figure from the listener while `inspect limits` printed theirs. The
catalogue rows still described the four keys as "declared and validated; enforcement is phase
H3's", although `ono-remote` has enforced them since H3.

ADR-0504 also kept the environment layer's rule for a value that does not parse or is out of
range: reported, not stored, the earlier layer's value stays. For the shell that is ADR-0010's
promise that a bad setting never stops it from starting. For a listening agent the earlier value
is Appendix A's, which is *wider* than whatever the operator meant to write — the one outcome
§55.2 forbids ("a security-sensitive agent limit MUST NOT silently become unlimited because a value
failed to parse"), and §2.3's "failure to apply that control MUST prevent the operation from
starting".

## Decision

**1. A listening agent reads every configuration layer the shell reads, in the shell's order.**
Before it binds, `ono --agent --listen` evaluates the configuration files exactly as an
interactive shell does — `/etc/ono/config.ono` (system), then the user's `config.ono`, or the one
file `--config` / `ONO_CONFIG` names (ADR-0094) — in the same restricted configuration mode, then
applies the `ONO_*` environment layer. `--no-config` before `--agent` reads none of them. The
loader is one function, `ono_cli::config::load_settings`, shared with the shell's own start-up, so
the two cannot drift.

**2. Precedence is ADR-0010's: environment over the user file over the system file over the
built-in default.** No agent-specific rule: an operator who knows how `get config` resolves a key
knows how the agent does, and `get config limits.remote_connections --overridden` shows both
layers.

**3. A configuration the agent could not read in full refuses the listener.** Any problem met
while loading — a file that cannot be read or does not parse, a statement in it that fails, a
value out of range or of the wrong type in a file or a variable — is printed and the agent exits
with a failure status *before* it opens a socket. The rule is "any problem" rather than "a problem
with a `limits.remote_*` key" on purpose: a file that does not parse may well have been the file
that set the ceilings, and the agent cannot know what it did not read. The refusal says how to
proceed deliberately: fix the named setting, or start with `--no-config` and accept the defaults
knowingly.

This replaces, for `--listen` only, ADR-0504 §5's "reported and not stored" for the environment
layer. The shell, `--agent` over stdio (which enforces no listener ceilings) and every other
setting keep ADR-0010's reported-and-continue rule.

**4. The descriptions say what is true.** The four catalogue rows now say the listening agent
enforces them and that the agent reads them from `config.ono` and the environment.

## Consequences

The figure in `config.ono`, the figure `inspect limits` reports, the figure the startup summary
prints and the figure the listener enforces are one number from one loader (§12.4, §52.2).

An agent started in an environment with an unrelated broken `ONO_*` variable, or a broken
`config.ono`, no longer starts listening. That is the intended direction (§2.3) and it is said
on standard error with the variable or file named; `--no-config` is the explicit way past for an
operator who wants Appendix A.

**Compatibility.** Two observable changes for someone upgrading: a listening agent now honours
`limits.remote_*` from `config.ono` (an agent that ran with Appendix A because of this bug now
runs with the operator's figures), and a listening agent whose configuration has a problem refuses
to start where it used to warn and continue.

Encoded by `crates/ono-cli/tests/agent_configuration.rs`:
`should_enforce_a_remote_ceiling_written_in_the_configuration_file` (the exit test of #190: a
pending-handshake ceiling from `config.ono` closes the second connection),
`should_let_the_environment_override_the_configuration_file_as_it_does_for_the_shell`,
`should_read_no_configuration_file_when_told_not_to`,
`should_refuse_to_listen_when_the_configuration_file_sets_a_ceiling_out_of_range`,
`should_refuse_to_listen_when_the_environment_sets_a_ceiling_it_cannot_read`,
`should_refuse_to_listen_when_the_configuration_file_cannot_be_read`,
`should_describe_the_remote_ceilings_as_enforced_by_the_listening_agent`; and acceptance case
`400-a-listening-agent-reads-its-ceilings-from-the-configuration-file`.

**Amended in the v0.6.6 remediation (review COR-R1).** Rule 1's "`--no-config` before `--agent`
reads none of them" made `--no-config` drop the `ONO_LIMITS_REMOTE_*` variables too — and v0.6.5's
listener applied them unconditionally (ADR-0504: the environment was the only layer it read),
while rule 3's own advice sends an operator to `--no-config`. An operator whose tighter ceiling
lived in the environment was silently given Appendix A's wider figures. Now, for `--listen`,
`--no-config` leaves the configuration *files* out and still applies the environment layer
(`ono_cli::config::load_environment`), with rule 3's refusal for a variable it cannot read; the
advice says so. The shell's own `--no-config`, which reads no layer at all, is unchanged: it is
not a security ceiling, and widening nothing there needs no exception. Encoded by
`crates/ono-cli/tests/agent_configuration.rs::should_keep_the_environments_ceilings_when_told_to_read_no_configuration_file`.

## Alternatives considered

**Read only the `limits.remote_*` lines out of `config.ono`.** A small parser for
`set config limits.remote_* = <int>` avoids building a session. It is a second reading of the
configuration language that disagrees with the first the moment someone writes the value through
a variable, an expression or a retired name, which is how `inspect limits` and the listener came
apart in the first place.

**File over environment for the agent.** The file is the operator's considered configuration and
the environment is often inherited. But every other consumer of these keys resolves environment
over file, and an agent that did the opposite would print, in `get config`, a layer it then
ignores.

**Keep reported-and-continue, and print the figure in force.** It is ADR-0010's rule and it keeps
an agent up. It also starts a network listener on a ceiling wider than the operator wrote, with
the explanation in a log line, which is §55.2's failure stated precisely.

**Refuse only on problems that name a `limits.remote_*` key.** Narrower, and blind to the file
that did not parse at all — the case where the agent knows least about what it was told.
