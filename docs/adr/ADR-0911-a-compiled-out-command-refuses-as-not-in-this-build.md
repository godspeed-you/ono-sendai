# ADR-0911: A compiled-out command refuses as `resolve.not_in_build`; a compiled-out provider answers as unavailable

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.2 §15.4, §16.1, §43 (error taxonomy), §50 (discoverability); ADR-0006, ADR-0008,
  ADR-0011 (resolution order), ADR-0910
- Issues: #127
- Decided by: agent (autonomous)

## Context

The core build of ADR-0910 compiles nine tiers out, and the shell's vocabulary still names every
one of them. Without a rule, each kind of name would reach a different failure:

- `map` would go to `PATH`, and then fail with `command not found` or run whatever program is
  called `map`.
- `get service` would fail as `resolve.target_not_found` ("no provider answers `service`").
- `at -1h` would be claimed by nothing.
- `help map` would describe a command that cannot run.

The issue requires two answers. A target whose provider is compiled out takes "the same
`provider unavailable` path a container without D-Bus already takes". A command of a
compiled-out tier gets a structured error that says the capability is not in this build. Neither
may be a parse error or a panic, and discoverability must not advertise what is absent.

## Decision

1. **A new code, `Ono-Sendai-E0104 resolve.not_in_build`, kind `resolution`.** The taxonomy is
   closed and additive (ADR-0006), so the code takes the next free number in the resolution
   family. It is not `command_not_found`, which would send a user looking for a typo or a
   missing program. It is not `provider.unavailable`, which blames the host: the same command
   runs on the same host in the full build. The message names the spelling and the tier. The
   error carries `tier` and `profile` metadata, and its help names `ono --version` and
   `exec:<name>`:

   ```text
   ono: Ono-Sendai-E0104 resolve.not_in_build `map` belongs to the spatial systems interface
   (spec v0.4), which this core build of ono does not include
   ```

2. **Exit status 126.** POSIX reserves 127 for a command that was not found and 126 for one that
   was found and cannot be executed. A name the build knows and cannot run is the second case.
   `status_for` maps the code to it, beside ADR-0008's 127 for `command_not_found`.

3. **Where it is decided.** `absent::claims` runs on every stage of a stage list after user
   functions and aliases, which win over every name (ADR-0011). It runs before anything else
   claims the stage: the shell's own commands, the registry and `PATH`. A stage belongs to a
   compiled-out tier when:
   - the full embedded registry resolves it to a command of the `spatial`, `temporal`, `change`,
     `kuang` or `remote` family, or of the verb `trace`;
   - its head is `adapt`;
   - its head is qualified by a package namespace (`echo:emit`), which only KUANG/11 fills.

   A compiled-out name is Ono's vocabulary, so it is never handed to a program that shares it.
   `exec:<name>` still reaches that program. `--agent` and `--print-peer-key` refuse the same
   way. `--at` on any command refuses as the temporal tier's. The core session is always in the
   present, and answering today's state for a past instant is what v0.5 §55.2 forbids. The full
   build refuses nothing here and does not consult the registry for it.

4. **Compiled-out providers answer as unavailable.** `absent::AbsentProvider` is registered in
   place of each provider the build leaves out, under the full build's id and with its targets:
   - `systemd`: `service`
   - `systemd-logind`: `session`
   - `systemd-journal`: `journal`, `log`
   - `container-engine`: `container`, `image`

   Its availability is `Unavailable("this core build of ono does not include …")`, and it
   advertises the capabilities the command contracts declare for its targets. The commands of
   those targets therefore resolve and bind exactly as in the full build: `get`, `watch`, and the
   mutations bound by capability (ADR-0068). All of them then meet the registry's own refusal:

   ```text
   ono: Ono-Sendai-E0401 provider.unavailable `service` cannot be answered here — systemd: <reason>
     the provider exists but the system it reads is not present. …
   ```

   Code, name, kind, message shape and help are the ones a full build gives on a host without
   systemd. Only the reason after the provider's id differs, and each build states its own truth:
   the full build names the missing bus socket, the core build names itself. Acceptance case 357
   runs in both profiles and holds them to the same shape.

5. **Discoverability carries only what the build carries.**
   - The command registry the shell advertises and runs (`help`, completion, `get command`,
     `find command`, the type check) is the embedded registry narrowed with
     `CommandRegistry::retaining`. That removes the command families of the compiled-out tiers,
     the `trace` commands, and every verb and target that only those used.
   - A `help` or `explain` topic that names a compiled-out command answers with the refusal the
     command gets, and so does `help here`.
   - `get config` and `inspect limits` omit the settings of compiled-out tiers (`spatial.*`,
     `temporal.*`, `change.*`, `recovery.*`, `limits.orientation_*`, `limits.remote_*`,
     `safety.confirm.remote_*`), and `set config` refuses them with `resolve.not_in_build`. An
     `ONO_*` variable for one of them is not read, because an environment may have been set up
     for the full build.
   - The usage text does not offer `--print-peer-key`.

   This matches how the shell already treats an unavailable provider: the provider's commands
   stay visible and answer as unavailable, while commands that cannot exist in this binary are
   not listed.

6. **`ono --version` names the profile.** The first line stays `ono <version>` in both builds, so
   a script that reads it reads what it always read. The core build adds a second line:
   `build: core (without adapter, change, container, graph, kuang, remote, spatial, systemd,
   temporal)`. The full build prints the one line it always has.

7. **A missing object in `enter` keeps the v0.2 answer.** Without the spatial tier there is no
   tombstone and no `spatial.not_found`, so `enter process 99999` answers
   `resolve.target_not_found`, as spec v0.2 §14.3 has it.

## Consequences

- A script can branch on `$e.name == "resolve.not_in_build"` or on status 126 and tell "this
  binary lacks it" apart from "no such thing".
- `crates/ono-cli/tests/core_build.rs` (core-profile only) and acceptance cases 355–359 encode
  every rule above.
- A new command family has to be mapped in `absent::tier_of`. A new provider of a tier that can
  be compiled out needs an `AbsentProvider` entry, or its targets would answer
  `resolve.target_not_found` in the core build.

## Alternatives considered

**Reusing `resolve.command_not_found`.** This was rejected: it says the name does not exist, and
it would send a user looking for a typo. It also lets a program on `PATH` answer instead.

**Reusing `provider.unavailable` for commands too.** This was rejected: `map` is not answered by
a provider that is absent from the host, and the help text of that code would mislead.

**Leaving the compiled-out commands in `help`, marked unavailable.** This was rejected:
completion and `get command` would offer them, and every caller of the registry would have to
learn a second notion of availability. A narrowed registry changes nothing for its callers.
