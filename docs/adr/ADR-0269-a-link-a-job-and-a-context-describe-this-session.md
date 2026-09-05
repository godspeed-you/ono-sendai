# ADR-0269: A link, a job and a context describe this session

- Status: accepted
- Date: 2026-08-29
- Spec refs: §14.4, §14.5, §21
- Decided by: agent (autonomous, `close-spat`)

## Context

§14.4: "The active link frame determines where provider calls and external processes execute."
`link`, `job` and `host` are served by the session provider `ono.shell`, which is an ordinary
provider, so entering a link swapped them for the far side's along with everything else:

```text
ono -c 'link host testbox --transport local; enter link testbox; get link | count | to json'
[0]
```

That is the remote agent's empty link table. The consequences were not symmetric, because the
mutations were never provider calls: `eval.rs` intercepts `add`/`set`/`rename`/`remove`/`detach
link` and `connect`/`test host` before any registry lookup, so `detach link` acted on *this*
session while the `get link` that would feed it answered for the other one. `get link | detach
link` could not be spelled from inside the link it would detach — and §14.5 says "all operations
SHOULD remain expressible without entering context", which reads oddly when entering one takes an
operation away. `get context` was already local, because it is a meta command and not a provider
call at all.

ADR-0103 recorded the old behaviour deliberately ("Inside a link frame `get link` and `get host`
are answered by the remote's `ono.shell`, as every provider call is"). This ADR reverses that half
of it.

## Decision

**`link`, `job` and `host` describe the session, not the machine, and answer locally inside a link
frame.** The narrowed `SessionProvider::session_facts` claims exactly those three and is registered
into the link's registry *first*, so it wins over the mounted remote provider for them and nothing
else.

The line is where the question is about: a link is a relationship this shell holds, a job is a
pipeline this shell started, a host is an entry in the sources this shell reads. None of them is an
observation of a system, which is what §14.4's "provider calls" means and what everything else in
the registry is.

**Everything else `ono.shell` serves stays remote.** `plugin`, `capability`, `audit`, `assistant`,
`model` and `finding` are facts about the machine the packages are installed on and the runtime
that executed them, and inside a link frame that machine is the far side.

## Consequences

- `get link | detach link`, `get link | remove link` and `get job | kill job` compose from inside
  a frame, and each acts on the session the user is typing into.
- A user who wants the far side's link table asks the far side for it, which is what the far side's
  own `ono` session would answer.
- ADR-0103's Consequences item about `get link` inside a frame is superseded in that one respect;
  the rest of ADR-0103 stands.
- Encoded by `ono-cli/tests/remote.rs::should_answer_for_this_sessions_links_and_jobs_from_inside_a_link_frame`
  and `::should_detach_the_link_it_is_standing_in_when_the_link_table_feeds_the_mutation`, and by
  acceptance case `044-remote-links-as-objects`.

## Alternatives considered

- **Intercepting `get link` in the evaluator, as the mutations are** — would work and would put a
  second answer to "which provider serves this target" outside the registry, where nothing can see
  it. Registering a narrowed provider keeps one mechanism.
- **Making the far side answer with this session's links** — a link is not transferable data; the
  far side has never heard of them.
- **Leaving it as ADR-0103 had it and documenting the workaround** — the workaround is `leave`,
  which is exactly the "operation not expressible in context" §14.5 rules out.
