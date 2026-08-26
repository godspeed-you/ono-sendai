# ADR-0010: Configuration, startup sequence and the config file language

- Status: accepted
- Date: 2026-08-26
- Spec refs: §4.1, §30, §34, §38, §49
- Decided by: agent (autonomous)

## Context

Spec §30 gives configuration as a sketch: four "potential locations", a list of domains, and an
example that spells settings as `set config prompt.path = "smart"` in a file named `config.ono`.
It does not say what that file's language is, how layers combine, what happens when a setting is
wrong, or what may run at startup. Spec §4.1 requires a nearly silent startup that "reaches the
prompt immediately", §34 caps cold start at 100 ms with a 50 ms target, and
`docs/ACCEPTANCE.md` §4.5 requires that startup load no plugin eagerly and query no
network-backed configuration.

## Decision

### The config file is Ono source, executed in a restricted mode

`config.ono` is what its extension says: an Ono script. It runs through the ordinary parser and
evaluator, so `set config …`, `let`, `fn` and alias definitions are the same constructs the user
already knows, and `explain` works on them. It runs in **config mode**, in which the evaluator
refuses:

- external command execution;
- any provider that performs I/O beyond reading the config tree;
- network access of any kind;
- plugin load (§31.8 distinguishes install, enable, load and run — startup does none of them).

A refusal is `safety.policy_denied` (E0702) naming the file and line. Config mode is a property
of the evaluation context, not of the parser, so nothing about the language changes.

### Layers, in order, later overriding earlier

```text
1. built-in defaults                       provenance: default
2. /etc/ono/config.ono                      provenance: system
3. $XDG_CONFIG_HOME/ono/config.ono          provenance: user   (default ~/.config/ono)
4. ONO_* environment variables              provenance: environment
5. command-line options to `ono`            provenance: invocation
```

`ONO_CONFIG` names a single file and replaces layers 2 and 3 entirely, which is what a test or a
container needs. `ONO_CONFIG_DIR` relocates the whole user tree. `--no-config` skips layers 2–4,
which is how a case proves behaviour without a config, and how a user recovers from a config that
breaks their shell.

Environment mapping is mechanical: `ONO_RENDER_TABLE_MAX_ROWS` sets `render.table.max_rows`.
Nothing is magic and nothing needs a table.

Directories follow the XDG base directory specification, honouring `XDG_CONFIG_HOME`,
`XDG_DATA_HOME` and `XDG_STATE_HOME`: config in `…/ono/`, history and result cache in the state
directory, KUANG/11 packages in the data directory as §30 shows.

### Every setting carries provenance

A resolved setting is a value plus the layer, file and line that set it, so `get config` answers
spec §30's requirement — "so the user can see which file or environment variable set each value"
— from data rather than from a guess. Provenance uses the same `ono_value::Provenance` every
other value carries.

### A bad setting never stops the shell from starting

An unknown key, a value of the wrong type, or a config file with a parse error produces a
structured diagnostic on stderr and startup continues with that setting at its previous layer's
value. A shell that refuses to start because one line of its config is wrong has removed the
tool its user needs to repair the config. The diagnostics remain available as structured values
via `get config --problems`, so nothing is merely printed and forgotten.

A config file that cannot be *read* — as opposed to parsed — is silently skipped when it does
not exist, and reported when it exists but is unreadable.

### The startup sequence

```text
1. read argv                      no I/O
2. resolve directories            environment only
3. load config layers             file reads, bounded, no network, no plugins
4. build the context              cwd, environment record, link = local
5. open history lazily            the file is not read until the first recall
6. print the identity line        one line, or nothing when disabled or non-interactive
7. prompt
```

Nothing else is eager. Providers are constructed on first use; the command registry is a static
table built at compile time; completion metadata is read from the registry already in memory.
The identity line of §4.1 (`ONO/7  local  linux/amd64`) is printed only when stdin and stdout are
both terminals, and is suppressed by `prompt.identity = false`. It is never printed for `-c`, for
a script, or when output is redirected, which is what makes redirected output deterministic
(§4.6).

### Non-interactive invocation

```text
ono                       interactive REPL
ono -c '<source>'         run source, then exit with its status
ono <file> [args…]        run a script file, `$args` bound to the remainder
ono -                     read a script from stdin
```

Script and `-c` modes are non-interactive: no identity line, no history recording by default, no
interactive confirmation prompt ever (§17.4 — a destructive operation that would need
confirmation fails with `safety.confirmation_required` instead of waiting for a TTY that a cron
job does not have).

## Consequences

Easy: one language to learn; `explain` and completion work inside the config file; provenance
answers "why is this set" without a debugger; a container can pin an exact configuration with one
environment variable; a user with a broken config still gets a shell.

Hard: config mode is a second evaluation mode that every new command must respect. The mitigation
is that it is a capability check in one place — the evaluation context — rather than a flag each
command inspects.

Must be revisited in phase I: KUANG/11 packages contribute settings and their own defaults
(§31.31), which adds a layer between built-in defaults and the system file.

Encoded by: the config tests in `crates/ono-cli`, and the acceptance cases
`027-startup-is-quiet` and the config-provenance case.

## Alternatives considered

- **TOML or YAML config** — rejected: it would mean two languages, two completion systems and two
  ways to express an alias, and spec §30 already names the file `config.ono`. Themes stay TOML,
  as §30 shows, because a theme is pure data with no behaviour.
- **Failing startup on a bad config** — rejected: it takes away the only tool that can fix it.
- **Eagerly loading plugins declared in config** — rejected outright: `docs/ACCEPTANCE.md` §4.5
  and spec §31.8 both forbid it, and it is the classic way a shell's startup budget dies.
- **Reading history eagerly** — rejected: a large history file would dominate the 50 ms target
  for a benefit no user sees before their first recall.
