# ADR-0021: How a typed command line becomes a provider call

- Status: accepted
- Date: 2026-08-26
- Spec refs: §6.1, §15, §26.2, §27, §42, §50, §52; ADR-0009, ADR-0011, ADR-0012
- Decided by: agent (autonomous)

## Context

ADR-0009 has the parser keep a words-mode argument as exact source text and reinterpret nothing,
so that an external command receives the bytes that were typed. Something has to do the
reinterpreting for a native command, using the types its contract declares. That is the command
layer, and building it forced several decisions the registry format does not fix.

## Decision

### 1. Selectors are alternatives, not positions

`get process` declares both `pid: int` and `name: string`. Spec §6.1 writes `get process 4419`
and §26.2 writes `get service nginx`, so both spellings must work and neither is "the first
selector".

A positional word therefore binds to the **first still-free selector whose declared type accepts
it**. `get process 4419` binds `pid`; `get process nginx` binds `name`. A word that no free
selector accepts is `type.mismatch` naming the word and the types that were available.

This is why the parser must not coerce: `4419` and `nginx` are the same kind of token until a
contract says otherwise.

### 2. The contract carries the argument mode, so binding needs no second opinion

`CommandContract::bind` takes only the arguments. The mode is a property of the command, and a
conformance test asserts that all 168 declared modes agree with `ono_parser::ArgMode::for_head`.
Two sources for one fact is how help and completion end up describing a language the parser does
not implement.

### 3. Requiredness is asked for, not declared

The registry marks no selector required, because whether one is depends on the command's other
arguments — `get process` needs none, `stop process` needs a target. The implementation asks
(`require_selector`), and the error names what was missing.

### 4. `type.mismatch` covers a surplus positional and a missing selector

`docs/spec/errors.yaml` has a code for neither, and spec §43 is closed and additive (ADR-0006).
Both are a value not fitting the shape the command declares, which is what `type.mismatch` says.

### 5. `explain` never fails

A stage whose arguments do not bind is reported *in the plan*, as a note on that stage, rather
than aborting it. `explain` exists to answer "what would happen", and refusing to answer because
something is wrong withholds the explanation exactly when it is most wanted (spec §15.3).

### 6. Capability risk is the provider API's vocabulary, not a copy

`CapabilitySpec` carries `ono_provider_api::Risk`. Two enumerations of the same four words would
drift, and one of them would be the one a security decision was made against.

## Consequences

Easy: `get process 4419` and `get service nginx` both work without the registry declaring
positions; a new command's help, completion and plan come from its contract with no code; a
contract that disagrees with the parser fails the gate rather than the user.

Hard: selector-by-type binding is ambiguous if a command ever declares two free selectors of the
same type. No command does, and `spec-check` should grow that check when one is proposed.

Encoded by: `crates/ono-command/tests/`, in particular the argument-binding suite and the
mode-conformance test over every command in the registry.
