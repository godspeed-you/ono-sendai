# ADR-0574: Rust has no UndefinedBehaviorSanitizer, so that job checks what the toolchain offers

- Status: accepted
- Date: 2026-09-04
- Spec refs: v0.4.1 §42.1, §42.2, §42.3, §42.4, §44.3, §66.5
- Issues: none (found by the first scheduled run of `verification.yml`)
- Decided by: agent (autonomous)

## Context

§42.3 asks Linux scheduled CI to run "AddressSanitizer and UndefinedBehaviorSanitizer on selected
integration tests **where Rust/toolchain support permits**". ADR-0522 built that as a matrix over
`-Zsanitizer=${{ matrix.sanitizer }}` with the values `address` and `undefined`, and the workflow
had never run: it is scheduled on the default branch, and the branch only carried it from
2026-09-04. Its first run answered in sixteen seconds:

```
error: incorrect value `undefined` for unstable option `sanitizer` - comma separated list of
sanitizers: `address`, `cfi`, `dataflow`, `hwaddress`, `kcfi`, `kernel-address`,
`kernel-hwaddress`, `leak`, `memory`, `memtag`, `safestack`, `shadow-call-stack`, `thread`, or
'realtime' was expected
```

UndefinedBehaviorSanitizer is a Clang instrumentation for C and C++ semantics — signed overflow,
misaligned access, invalid enum values — and `rustc` has never accepted it. What is undefined in
Rust is defined by a different document, and the tools that check it are different tools. So the
conditional clause in §42.3 is not a loophole here; it is the sentence that applies.

## Decision

**AddressSanitizer stays, and the undefined-behaviour job runs the checks Rust does have:
`-Zub-checks` over a standard library rebuilt with them.**

`-Zub-checks=yes` compiles in the library's own preconditions — `get_unchecked` bounds,
`unreachable_unchecked`, `NonNull::new_unchecked`, alignment and validity assertions on
`ptr::read` and `transmute` — which are exactly the operations an `unsafe` block promises are
sound. Paired with `-Zbuild-std` the checks reach across the standard library too, so a wrapper
that hands `std` an invalid value is caught inside `std` rather than at the next corruption. Both
jobs run the same four crates that hold every `unsafe` block, and the crate list is still read
from the tree by the gate.

This is the same relationship to §42.3 as before: two mechanisms, one for memory errors and one
for undefined behaviour, on a daily schedule, blocking a release when they find something (§42.4).

## Consequences

- The verification workflow has three jobs: `miri`, `address`, and `undefined behaviour`. The
  gate's contract test checks the sanitizer flag and the UB flag rather than a matrix value that
  a compiler rejects, and still requires every unsafe-bearing crate to appear in both.
- A finding from `-Zub-checks` is a panic with a message naming the precondition, which
  reproduces on any nightly with the same flags. §42.4's "reproducible" is satisfied without an
  artifact.
- Miri remains the semantic check — aliasing, provenance, uninitialised reads — and remains the
  one that cannot execute the process layer.

## Alternatives considered

- **MemorySanitizer (`-Zsanitizer=memory`) as the second job.** It finds uninitialised reads,
  which is undefined behaviour, and the priority target of §42.3 is the syscall and FFI layer.
  Rejected: MSan reports every read of memory an uninstrumented library wrote, and the FFI target
  here is `getaddrinfo` inside the system C library. The job would report the C library on every
  run, which is §42.4's release blocker pointed at a false positive.
- **ThreadSanitizer.** A useful check the supervisor's threads would earn, and a different
  question from the one §42.3 asks. Not ruled out later; not a stand-in for this.
- **Leaving the matrix and marking the job as allowed to fail.** Refused by ADR-0522 in its own
  words, and rightly: a job that may fail and still report green is a job nobody reads.
