# Ono-Sendai

**A typed, structured Unix shell for people who like machines.**

> *The sky above the port was the color of television, tuned to a dead channel.*
> — William Gibson, *Neuromancer*, 1984

In Gibson's Sprawl, **Ono-Sendai** builds the cyberspace decks — the hardware a console
cowboy jacks into to see the system as it actually is, instead of squinting at a readout.
That is the whole idea here, minus the fiction:

> **Bash is a command interpreter. PowerShell is an object shell. Ono-Sendai is a systems interface.**

The command is `ono`. The deck is real this time.

> **Status: pre-implementation.** The specification, the agent instructions, the workspace and
> the containerised verification harness are in place and green. The interpreter is not — there
> is no shell to install yet. What follows describes what is being built and how it will behave.

---

## The problem

`ps`, `find`, `tar`, `dd`, `ip`, `systemctl`, `git`, `awk`, `grep` — individually excellent,
collectively a vocabulary you memorise rather than a language you understand. Each has its own
grammar, its own flags, its own idea of what a column is.

And all of them flatten. The kernel *knows* that a process is a process, a socket is a socket,
a service is a service. Then it prints characters, and you rebuild the structure that was
already there with `awk`, `cut`, `sed`, `jq` and regexes that break the day someone adds a
column. Decades of tooling exist to reconstruct information the machine never should have
thrown away.

PowerShell proved the fix — objects survive the pipe — and then wrapped it in a ceremony
Unix people justifiably bounced off. Ono-Sendai takes the idea seriously and reinterprets it
for terminal culture: terse, composable, and honest about the Unix underneath.

## What it feels like

Values keep their shape all the way down the pipe:

```text
local://~ > get process | where cpu > 20 | sort cpu desc

PID    PROCESS        CPU     MEM      USER
4419   rustc          96.1%   3.8 GiB  masl
812    postgres       24.8%   1.2 GiB  postgres
```

That table is only a *rendering*. What actually flows is `Stream<Process>` — so the next stage
doesn't parse columns, it addresses objects:

```text
local://~ > get process | where name == "postgres" | stop process
```

Unix stays underneath, unharmed and first-class:

```text
local://~ > git log --oneline | grep fix
```

Text from foreign programs enters as text. Ono-Sendai does not hallucinate schemas out of
someone's stdout — structure is native where Ono owns the command or where a program explicitly
speaks a structured protocol, and honest text everywhere else.

### Remote hosts become places, not subprocesses

```text
local://~ > link prod-db
link prod-db established  12 ms  linux/amd64

prod-db://~ > enter service postgres

prod-db://service/postgres > get socket

PROTO  LOCAL        REMOTE            STATE
tcp    :5432        10.4.3.17:55128   established
tcp    :5432        10.4.3.21:49312   established

prod-db://service/postgres > trace

postgres
+-- listens tcp/:5432
+-- reads /etc/postgresql/...
+-- writes /var/lib/postgresql/...
+-- clients
    +-- api-1  10.4.3.17
    +-- api-2  10.4.3.21
```

The prompt is a location URI because you are, in a real sense, somewhere. `link`, `enter`,
`trace`, `detach` are not costume jewellery — each names exactly what it does.

## Core ideas

- **Structured by default, text by choice.** Native commands emit typed values; rendering is a
  separate concern from data.
- **Unix remains underneath.** Running an arbitrary executable stays trivial. Always.
- **A predictable language, not a pile of syntax.** A small grammar and a controlled verb
  registry — `get`, `set`, `where`, `sort`, `enter`, `trace`, `watch`, `link` — instead of
  fifty bespoke command dialects.
- **Discoverability is speed.** Completion and help expose the *language*, not just filenames.
- **Objects are transparent.** Field names, types, origin and raw values are always inspectable.
- **Errors are values.** Structured, typed, inspectable — with a human rendering on top.
- **Local and remote share one mental model.** The active context is always visible.
- **Danger is visible before damage.** Privilege, remote targets and destructive scope announce
  themselves in the prompt, not in the post-mortem.
- **No fake intelligence.** The shell never guesses destructive intent from vague text.

## KUANG/11

The extension runtime is named after the Chinese military icebreaker Case rides through
Straylight's ICE — *Kuang Grade Mark Eleven*. It loads analysis programs, providers,
interactive lenses, automations and AI assistants into the shell under an explicit capability
and isolation model: manifests, declared capabilities, sandboxed execution, an audit trail, and
a deterministic test host every package must pass.

> **Ono is the deck. KUANG/11 is the software you load into it.**

Extensions contribute real objects and real relationships to the same typed pipeline as native
commands. A plugin that inspects Postgres internals produces `Stream<T>` you can filter, sort
and pipe like anything else — not a wall of text with a nicer banner.

## House rules for the aesthetic

**Allowed:** terse system vocabulary where the word is *accurate*; live tables whose movement
comes from real events; graph views of real relationships; latency indicators on real links; a
prompt that expresses real location; dark, restrained themes.

**Forbidden by default:** `ACCESS GRANTED` on a successful `ls`. Fake scanning progress. Matrix
rain. Random glitches. Boot animations. Hexadecimal noise. Artificial keystroke delay. Sound
effects. Failure messages written like a video game.

> **The machine is already strange enough. Reveal it. Do not decorate it.**

A live object stream looks alive because processes are dying in it. A dependency trace feels
like walking into a machine because the edges came from the kernel. The prompt creates a sense
of place because you actually are somewhere. Every effect in this shell is a side effect of
telling the truth about the system — which is also why none of it can be turned into a
screensaver.

## Where this is going

The build order is dependency-respecting, not MVP-then-maybe-finish. Every phase is meant to be
production infrastructure for the next one.

| Phase | Delivers |
|---|---|
| **A** | Language and Unix shell foundation — parser, execution, quoting, jobs, signals |
| **B** | Value system and native pipelines — the typed stream engine |
| **C** | Linux core providers — process, file, user, mount, interface, socket, service |
| **D** | Consistency and discoverability — registries, help, semantic completion, `explain` |
| **E** | Contextual systems interface — the context stack, `enter`/`leave` |
| **F** | Live system semantics — `watch`, events, native background jobs |
| **G** | Relationship graph — `trace`, graph values, provenance |
| **H** | Remote links — the protocol, the agent, capability negotiation |
| **I** | KUANG/11 extension runtime |
| **J** | Advanced TUI views — where the semantics actually justify them |

Written in Rust, because this needs low-level Unix integration, async I/O, real concurrency
safety, a recoverable parser, and PTY and job-control work. Latency is treated as a product
feature and specified like one: under 50 ms cold start, under 8 ms from keystroke to render,
first rows of `get process` inside 50 ms — measured against machines with tens of thousands of
processes, slow NSS and high-latency links.

## Repository

```
docs/ono_sendai_shell_spec_v0.2.md   the specification — product, language, architecture, KUANG/11
docs/ACCEPTANCE.md                   what "finished" means, in boxes a script can check
docs/STATE.md                        the work board: what is done, what is next
docs/decisions/                      architecture decision records
AGENTS.md                            operating instructions for autonomous agents
crates/, xtask/                      the workspace
docker/, scripts/                    the container and the three gates
docs/spec/                           machine-readable contracts — arrives with phase D
```

### Verifying it

```bash
scripts/gate.sh            # format, lint, test, contract check, docs
scripts/acceptance.sh      # build a container, run every case against the real `ono` binary
scripts/release-check.sh   # both, plus the release checklist
```

The middle one is the interesting one. It builds a clean Debian image, installs `ono` as the
login shell of an unprivileged user, cuts the network, and asks the binary to prove each
advertised capability against a process table nobody tuned for the test. A feature that has not
survived that is not a feature yet.

`scripts/release-check.sh` currently exits 1 and prints the boxes that are still open. It will
keep doing that until the shell is done.

The specification is the source of truth and is deliberately more detailed than a pitch:
command metadata, object schemas, error taxonomy, grammar and test matrices are meant to be
*derivable* from it rather than reinvented per feature.

Development is test-driven and largely agent-driven. `AGENTS.md` is the contract those agents
work under — tests are the referee for whether a goal is reached, and every decision the spec
does not fix is made autonomously and recorded as an ADR. If you are an agent reading this:
`AGENTS.md` is your entry point, not this file.

## License

MIT. See [LICENSE](LICENSE).

---

*A modern shell inspired by PowerShell and Neuromancer. The time has come to bring our fiction
into life.*
