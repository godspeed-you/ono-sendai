# ADR-1023: Who may be asked is the invoking evaluator's, not the instance's

- Status: accepted
- Date: 2026-11-10
- Spec refs: §18.4, §31.10, §31.37; K11P §14, §20.3; `docs/contracts/kuang/permissions.v1.yaml` →
  `jit.rules`; ADR-0603, ADR-0995, ADR-0998; issue #304 (review finding S-F2, a regression of
  ADR-0995)
- Decided by: agent (autonomous)

## Context

A just-in-time permission (K11P §14) is answered by a `ConsentSource` the shell hands the
supervisor. The shell built it once, at load, as `ShellConsent::new(…, session.is_interactive(),
…)`: the instance asked a person if the evaluator that loaded it was an interactive foreground,
and nobody otherwise.

That was right while each evaluator had instances of its own. ADR-0995 made a background job run
packages in the session's one instance, and its consequences said the source stays the loader's
"as before". The security review showed what that means:

- A package the foreground loaded, invoked from a job, printed its JIT question at the shell's
  prompt and read the keystrokes the person was typing for the shell — an `a` became a persisted
  `always` grant.
- A package a job loaded refused the foreground's JIT call with `permission.required … nobody can
  be asked`, at a terminal with a person in front of it.

Both pty tests below reproduce it. K11P §20.3 says a script — and a job is one — never waits for a
prompt and never assumes consent; a person at the terminal is asked.

The protocol's host calls do not name the invocation they belong to, and an instance may have
several invocations open from the foreground and from jobs at once.

## Decision

**Who may be asked is decided per invocation, by the evaluator that runs it. The instance itself
asks nobody.**

1. The supervisor takes an optional consent source with each invocation
   (`LoadedPlugin::invoke_with_consent`, `query_with_consent`; `invoke`/`query` pass none) and
   keeps it for as long as the invocation is open.
2. A just-in-time question is answered by the open invocation's own source while it is the only
   invocation open in the instance — the attribution rule the audit trail's label already follows.
   With none or several open, or for an invocation started without a source, the source the
   instance was loaded with answers.
3. The shell loads every instance with a source that asks nobody (`permission.required` with the
   remedy), whoever loads it, and passes each contributed command and each typed target query the
   source of the evaluator running it: interactive for the foreground at a terminal, asking nobody
   in a script or a job.
4. Calls through the provider registry (`PluginProvider::snapshot`, `act`, spatial edges) are made
   on behalf of no evaluator the registry can name, and pass none: nobody is asked.

Every ambiguity therefore resolves to *nobody is asked*: a refusal with a remedy, never a question
at the wrong terminal and never consent assumed.

## Consequences

- A job never prints a question or reads the terminal; a line typed for the shell while a job
  hits a JIT permission runs in the shell, and the job's call is `permission.required`. The
  foreground is asked in an instance a job loaded.
- A foreground JIT call made while a job's invocation is open in the same instance is refused
  rather than asked: the broker cannot tell whose call it is. The remedy line decides it, and an
  answer of `session` or `always` given once covers later calls. Giving host calls an
  `invocation` field (as `views.open` has, ADR-0991) would lift this; it is a protocol change.
- A JIT question reached only through the provider registry (resolving or acting on a contributed
  object, the edges of a contributed relation) is no longer asked at a terminal even in the
  foreground; before ADR-0995 it was, for a foreground-loaded instance. Typed `get <target>` and
  contributed commands — the paths a person meets a JIT question on first — still ask.
- The shell resolves the installed package once per contributed invocation to build the source.
- ADR-0995's consequence "answered by the consent source of the evaluator that loaded the
  instance, as before" no longer holds.
- Tests: `crates/ono-cli/tests/permissions.rs` —
  `should_never_ask_from_a_background_job_in_an_instance_the_foreground_loaded` (RED: the job's
  question appeared at the prompt),
  `should_ask_at_the_terminal_in_an_instance_a_background_job_loaded` (RED: the foreground got
  `permission.required … nobody can be asked`); unchanged and green:
  `should_ask_at_first_use_and_keep_an_always_answer_for_that_program_at_a_terminal`. No
  acceptance case: both halves need a person at a pseudo-terminal with a JIT-governed package
  installed from a catalog and keystrokes timed against a job, which the container's piped `pty:`
  cases cannot time; the non-interactive half (`permission.required` in a script) is already case
  material of K11P Gate J.

## Alternatives considered

- **Keep one source per instance and make it ask only when no job is running.** The shell cannot
  tell, from inside the source, which evaluator made the call; it would guess from the job table
  and ask the foreground about a job's call.
- **A source per instance, switched by the shell before each invocation.** Two evaluators invoking
  concurrently would race over one switch.
- **Ask whenever any open invocation is the foreground's.** The job's call would be asked at the
  terminal, which is the defect.
