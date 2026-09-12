# Ono-Sendai v0.6.1 - Stabilization and Polish

**Status:** Proposed  
**Release:** v0.6.1  
**Type:** Patch release / stabilization release  
**Target repository:** `godspeed-you/ono-sendai`  
**Scope:** Bug fixes, behavioral corrections, UX polish, and completion-system consolidation

---

## 1. Purpose

Ono-Sendai v0.6.1 is a stabilization and polish release following v0.6.0.

The purpose of this release is not to introduce another major product capability. Instead, it consolidates a set of defects, inconsistencies, incomplete interactions, and UX shortcomings discovered while using functionality already present in v0.6.0.

The release should leave the existing system more coherent, predictable, robust, and pleasant to use before development proceeds toward v0.7.

v0.6.1 intentionally groups multiple GitHub issues into a single patch release rather than creating separate patch versions for every individual correction.

The release has four primary goals:

1. fix known regressions and incorrect behavior;
2. make existing features behave consistently across different invocation paths;
3. complete the expression-aware completion experience;
4. improve robustness without expanding the conceptual scope of Ono-Sendai.

---

## 2. Release philosophy

v0.6.1 is a **stabilization release**, not a feature release.

Changes are appropriate for v0.6.1 when they:

- correct behavior that is clearly wrong;
- make two existing ways of performing the same operation behave consistently;
- finish an interaction that already exists but is incomplete;
- improve error tolerance;
- improve discoverability or usability of existing functionality;
- introduce small internal architectural improvements required to solve the included issues correctly;
- improve tests for existing behavior.

Changes are not appropriate merely because they are convenient to implement while working on affected code.

In particular, v0.6.1 MUST NOT become an opportunity to pull planned v0.7 functionality forward.

The guiding principle is:

> Make what already exists behave as though it was designed as one coherent system.

A successful v0.6.1 should not primarily feel like Ono-Sendai gained another set of features. It should feel like v0.6.0 became noticeably more finished.

---

## 3. GitHub milestone

A GitHub milestone MUST be created for this release.

### 3.1 Milestone name

The milestone MUST be named:

```text
v0.6.1
```

The milestone description SHOULD state approximately:

> Stabilization and polish release following v0.6.0. Focused on correctness, REPL behavior, completion, plugin/core integration, and small compatibility fixes. No new major product capabilities.

### 3.2 Milestone membership

The following issues MUST be assigned to the `v0.6.1` milestone:

- #128
- #130
- #131
- #132
- #133
- #134
- #135
- #136

If additional issues are discovered during implementation, they MAY be added to the milestone only if they satisfy the scope rules defined in this specification.

Adding an issue to the milestone MUST NOT be used to silently expand the release into a larger feature release.

### 3.3 Milestone as operational release scope

The GitHub milestone is the operational representation of the release scope.

This specification defines the architectural and behavioral boundaries of v0.6.1.

The individual GitHub issues remain the normative detailed problem statements and acceptance criteria for their respective changes.

The relationship is therefore:

- this specification defines **why the changes belong together and which cross-cutting rules apply**;
- the GitHub issues define **the concrete defects and expected local behavior**;
- the GitHub milestone defines **the actual release inventory**.

Issue descriptions SHOULD NOT be copied into parallel tracking artifacts unless additional context is necessary.

### 3.4 Milestone maintenance

During implementation:

- every intentionally included issue MUST be assigned to the milestone;
- an issue removed from the release MUST also be removed from the milestone;
- newly discovered defects included in the release MUST be tracked as GitHub issues and added to the milestone;
- issue closure MUST remain traceable to commits or pull requests.

The milestone and the actual release contents MUST remain synchronized.

### 3.5 Milestone completion

v0.6.1 MUST NOT be considered complete while any issue belonging to the milestone remains unresolved unless that issue has explicitly been:

- removed from the milestone;
- deferred with documented reasoning;
- superseded by another issue or change.

The milestone SHOULD be closed after the v0.6.1 release has been successfully published.

---

## 4. Release scope

### 4.1 Included issues

The release includes:

| Issue | Area | Release rationale |
|---|---|---|
| #128 | Permissions / scope | Correct existing state and representation behavior |
| #130 | Plugin target resolution | Restore semantic consistency across invocation paths |
| #131 | `ss` adapter robustness | Fail soft on unknown external socket types |
| #132 | REPL rendering | Preserve output without a trailing newline |
| #133 | Completion | Prevent invalid filesystem fallback |
| #134 | Completion | Make completion expression-aware |
| #135 | Completion | Add type-aware value completion |
| #136 | Completion UI | Surface candidate documentation without affecting insertion |

### 4.2 Explicitly excluded issues

The following issues are explicitly excluded:

- #124
- #125
- #126
- #127
- #129

Their exclusion is deliberate and does not imply that they are low priority.

---

## 5. Issue #128 - Permission and scope correctness

Issue #128 belongs to v0.6.1 because it corrects existing permission/scope behavior rather than introducing a new capability.

The implementation MUST ensure that permission state and its representation remain internally consistent.

The fix SHOULD address the root cause rather than only correcting rendered output.

Security-related state MUST remain deterministic and auditable.

Tests MUST verify effective state, not only formatted output.

Existing permission semantics MUST NOT be expanded as part of this issue unless required to restore already intended behavior.

---

## 6. Issue #130 - Plugin target resolution in functions

Issue #130 belongs to v0.6.1 because equivalent invocation paths currently behave differently when resolving plugin targets.

A target that can be resolved through an ordinary pipeline or alias SHOULD also resolve when used from a function where the language semantics permit the same operation.

The fix SHOULD eliminate accidental differences between:

- direct execution;
- aliases;
- functions.

The implementation SHOULD reuse the normal target-resolution mechanism rather than creating a separate function-specific resolver.

The objective is semantic consistency, not another resolution path.

Regression tests SHOULD demonstrate equivalent externally observable behavior across supported invocation forms.

---

## 7. Issue #131 - Robust handling of unknown socket types

Issue #131 is a robustness and compatibility fix.

External tools, operating systems, kernels, or future implementations may produce values unknown to Ono-Sendai.

Adapters MUST prefer preserving usable information over rejecting an entire result because one value is unknown.

For the affected `ss` integration, unknown socket types MUST therefore be handled gracefully according to the behavior defined by the issue.

The general principle is:

> Unknown external data should normally degrade representation, not destroy otherwise valid results.

This principle MAY guide nearby adapter code, but v0.6.1 MUST NOT turn this issue into a general adapter refactor.

Regression coverage MUST include at least one previously unknown or unsupported socket-type value and verify that otherwise valid records remain usable.

---

## 8. Issue #132 - REPL output without trailing newline

Issue #132 fixes REPL output corruption or loss when command output does not terminate with a newline.

The REPL MUST preserve command output independently of whether the producer emitted a trailing newline.

Prompt rendering MUST NOT overwrite or visually consume valid program output.

The implementation MUST handle at least:

- output ending in `\n`;
- output without a final `\n`;
- empty output;
- repeated commands;
- interactive prompt redraw.

A representative regression case is conceptually equivalent to:

```sh
printf foo
```

The visible `foo` MUST remain visible when the next prompt appears.

The fix MUST NOT introduce unconditional extra blank lines after correctly terminated output.

The REPL owns prompt presentation. Individual commands MUST NOT be required to work around this problem themselves.

---

## 9. Issues #133-#136 - Completion subsystem

Issues #133, #134, #135, and #136 form one coherent completion-system improvement.

They MUST NOT be implemented as four unrelated local patches.

Together they establish an explicit completion pipeline:

```text
user input
    |
    v
syntactic/context analysis
    |
    v
completion state
    |
    v
candidate providers
    |
    v
candidate normalization
    |
    v
completion UI
    |
    v
insertion
```

The implementation SHOULD preserve the existing architecture where practical, but MAY introduce shared internal abstractions where required to prevent duplicate or contradictory completion logic.

---

## 10. Completion model - Context before provider

Completion MUST first determine **what kind of thing can syntactically occur at the cursor**.

Only after that context has been determined SHOULD completion providers be queried.

The following anti-pattern MUST be avoided:

```text
No semantic candidates found
-> fall back to filesystem completion
```

Lack of semantic candidates does not imply that a filesystem path is syntactically valid.

The correct model is:

```text
Determine valid candidate categories
-> query only providers valid in this position
```

This rule is central to issue #133.

---

## 11. Expression-aware completion

The completion system MUST understand expression structure sufficiently to distinguish at least:

```text
field -> operator -> value -> logical connector -> field
```

It MUST work with both whitespace-separated and compact expressions where supported by Ono syntax.

Examples include forms conceptually equivalent to:

```text
size > 1GB
```

and:

```text
size>1GB
```

Completion MUST be based on syntax rather than merely splitting input on whitespace.

A full compiler-grade parse of an incomplete expression is not required if a smaller completion-specific parser or state machine produces correct behavior.

However, completion parsing SHOULD reuse canonical language information wherever practical to avoid grammar drift.

---

## 12. Field completion

Where an expression expects a field, completion SHOULD offer fields valid for the object type currently flowing through the pipeline.

Fields SHOULD originate from the same schema/type information used by the rest of the system where possible.

The completion system MUST NOT maintain an unrelated manually duplicated universe of field definitions if canonical metadata already exists.

Candidates MAY include documentation metadata.

---

## 13. Operator completion

After a field has been identified, completion MUST offer operators compatible with that field.

Operator selection MUST consider field type.

Relevant categories may include:

- equality and comparison;
- ordering;
- containment and membership;
- boolean-specific operations;
- pattern or string operations where supported.

The exact operator set remains governed by the language semantics and the relevant GitHub issue.

Completion MUST NOT advertise operators the evaluator cannot execute.

Likewise, valid evaluator operators SHOULD be discoverable through completion where appropriate.

If checker, evaluator, and completion semantics currently disagree, v0.6.1 SHOULD consolidate them around one canonical definition rather than introducing another independent rule table.

If this requires a durable architectural decision, an ADR SHOULD document the canonical relationship between:

- expression field types;
- valid operators;
- evaluator semantics;
- checker semantics;
- completion semantics.

---

## 14. Value completion

When the cursor is in a value position, completion SHOULD use type information from the selected field and operator.

At minimum, behavior defined in #135 MUST be covered for relevant types such as:

- enum values;
- boolean values;
- byte-size literals;
- duration literals;
- collection or membership syntax such as `in`.

The completion implementation MUST handle partially typed values sensibly.

Representative examples include:

```text
tr<Tab>
ESTA<Tab>
5G<Tab>
[<Tab>
```

Where a finite set of concrete values exists, candidates MAY complete the value directly.

Where the value space is open-ended, completion MAY provide syntactic guidance rather than inventing arbitrary values.

Dynamic value discovery from external systems is outside the scope of v0.6.1 unless already explicitly required by an included issue.

---

## 15. Logical connector completion

After a complete expression predicate, completion SHOULD offer valid continuation constructs such as logical connectors when supported by the expression language.

After such a connector, completion returns to the field state.

Conceptually:

```text
field
  -> operator
  -> value
  -> connector
  -> field
```

The state machine MUST tolerate incomplete input without crashing or producing unrelated filesystem suggestions.

---

## 16. Filesystem completion

Filesystem completion remains an important shell capability.

This release does not restrict filesystem completion globally.

Instead, filesystem completion MUST only be invoked where a path is valid in the current syntactic position.

Therefore:

```text
semantic provider produced zero candidates
```

MUST NOT by itself trigger filesystem completion.

Filesystem completion eligibility is a syntactic/contextual property.

---

## 17. Completion candidates

Completion candidates SHOULD use a common internal representation.

Where useful, this representation SHOULD distinguish at least:

- insertion text;
- display text;
- documentation;
- candidate category or type;
- source or provider.

Existing structures SHOULD be extended rather than replaced unnecessarily.

Insertion behavior MUST remain independent from presentation metadata.

For example, documentation shown alongside a candidate MUST NOT become part of the text inserted into the command line.

---

## 18. Completion documentation UI

Issue #136 requires completion documentation already available internally to survive through to the interactive UI.

Candidate documentation SHOULD be visible without changing the inserted text.

Rendering MUST:

- remain readable at normal terminal widths;
- respect terminal width;
- avoid corrupting candidate insertion;
- handle missing documentation;
- handle long documentation gracefully;
- behave correctly with Unicode display widths where the existing terminal UI supports them.

The implementation MAY truncate documentation when necessary.

The UI SHOULD favor a compact shell-native presentation rather than a large IDE-style documentation panel.

The intent is discoverability without turning completion into a full IntelliSense implementation.

---

## 19. Candidate prioritization and deduplication

Where multiple candidate providers are valid at the same position, results SHOULD be deterministic.

The completion system SHOULD:

1. gather candidates from valid providers;
2. normalize them;
3. deduplicate equivalent candidates;
4. preserve useful documentation;
5. present results in a stable order.

Provider-specific ranking SHOULD only be introduced if necessary.

v0.6.1 MUST NOT introduce a large generic ranking framework.

---

## 20. Incomplete and invalid input

Interactive shells routinely operate on syntactically incomplete input.

Therefore completion MUST treat incomplete input as normal.

Completion MUST NOT:

- panic;
- abort the REPL;
- produce arbitrary filesystem candidates merely because parsing failed;
- require the current command to be fully valid.

Where the current state cannot be determined reliably, the completion system SHOULD fail conservatively.

Conservative failure means:

- no completion; or
- only candidates known to be valid.

It does not mean guessing unrelated completion categories.

---

## 21. Completion architecture constraints

v0.6.1 MAY refactor code required to make the included issues coherent.

However, the implementation SHOULD favor consolidation over abstraction for abstraction's sake.

The following SHOULD be avoided:

- a new generic IntelliSense framework;
- a second expression grammar used only for completion;
- duplicate type systems;
- duplicate operator registries;
- large public API changes;
- speculative extension points without an immediate user;
- new plugin protocols solely for completion.

The target is a robust completion subsystem, not an IDE infrastructure project.

---

## 22. Issue #129 - Explicit exclusion

Issue #129 is explicitly excluded from v0.6.1.

Although it contains observable defects, it spans multiple aspects of plugin place/pin behavior and may require broader decisions around:

- persisted plugin types;
- metadata lookup;
- lazy installation;
- spatial resolution;
- `find place` behavior.

It SHOULD be handled separately rather than expanding the stabilization release.

Discovering that an included issue shares implementation code with #129 does not automatically bring #129 into scope.

A small prerequisite fix MAY be made if strictly necessary for an included issue, but #129 itself MUST remain open unless its complete acceptance criteria are intentionally brought into the milestone.

---

## 23. Issues #124-#127 - Explicit exclusion

Issues #124-#127 are explicitly excluded.

They belong to the binary-size, build, and packaging effort and involve changes such as:

- compiler and release optimization;
- binary-size measurement or gates;
- KUANG/11 build separation;
- feature-set-based builds for constrained environments.

Those changes form a separate conceptual body of work.

They MUST NOT be folded into v0.6.1 merely because build configuration or related code is touched during implementation.

---

## 24. Scope expansion policy

While implementing v0.6.1, additional defects may be discovered.

A newly discovered issue MAY be fixed as part of the release when all of the following are true:

1. it is clearly a defect in existing v0.6.0 functionality;
2. the fix is small and well understood;
3. it does not introduce a new product capability;
4. it does not substantially alter architecture;
5. it is required for the included changes to function correctly or is clearly appropriate for the same stabilization pass.

If the defect is intentionally included in v0.6.1:

- a GitHub issue MUST exist for it;
- the issue MUST be assigned to the `v0.6.1` milestone;
- regression coverage MUST be added.

If those conditions are not met, the issue SHOULD be scheduled separately.

---

## 25. Testing strategy

v0.6.1 requires regression coverage for every included issue.

Every issue MUST have at least one test that fails against the affected pre-fix behavior and passes after the fix.

Where appropriate, tests SHOULD be behavior-oriented rather than implementation-oriented.

The tests should answer:

> Can a user still observe the defect?

rather than only:

> Does the new helper function return the expected internal value?

---

## 26. Completion test matrix

Completion tests SHOULD cover combinations of:

- empty input;
- partial field;
- complete field;
- partial operator;
- complete operator;
- partial value;
- complete value;
- compact syntax;
- whitespace-separated syntax;
- logical continuation;
- invalid or incomplete expression;
- path-valid contexts;
- path-invalid contexts;
- candidates with documentation;
- candidates without documentation;
- narrow terminal rendering where testable.

Representative flows SHOULD test full completion sequences rather than individual helper functions only.

Example conceptual flow:

```text
field<Tab>
field operator<Tab>
field operator value<Tab>
field operator value connector<Tab>
```

Tests SHOULD verify both:

- candidates presented;
- text actually inserted.

---

## 27. REPL regression testing

Regression tests for #132 SHOULD verify terminal-visible behavior where practical.

Particular attention MUST be paid to output without a trailing newline.

The visible command output MUST remain intact when the prompt is rendered.

The implementation MUST also prove that correctly newline-terminated output does not gain spurious blank lines.

---

## 28. Plugin/core consistency testing

The fix for #130 SHOULD include behavior tests proving that target resolution is consistent between supported invocation mechanisms.

The tests SHOULD demonstrate equivalent externally observable behavior for the relevant:

- direct pipeline;
- alias;
- function.

They SHOULD NOT merely exercise a newly introduced resolver helper.

---

## 29. Adapter robustness testing

The fix for #131 SHOULD include previously unknown or unsupported external enum/type values.

The test MUST verify that otherwise valid records remain usable and that failure is localized to the unknown value representation.

---

## 30. Permission-state regression testing

Issue #128 MUST include regression coverage for the state transition that previously produced incorrect permission/scope behavior.

Tests SHOULD validate:

- stored/effective state;
- replacement semantics where relevant;
- user-visible representation where relevant.

A test that checks only presentation is insufficient if the underlying effective state could still be wrong.

---

## 31. Quality gates

Before v0.6.1 may be released:

- all existing tests MUST pass;
- all new regression tests MUST pass;
- formatting checks MUST pass;
- linting MUST pass;
- supported build configurations MUST compile;
- relevant integration tests MUST pass;
- there MUST be no known regression introduced by the release;
- all milestone issues MUST be resolved or explicitly removed/deferred;
- documentation affected by changed behavior MUST be updated.

Warnings introduced by the release SHOULD be treated as defects.

---

## 32. Compatibility

v0.6.1 SHOULD remain compatible with v0.6.0 configuration, scripts, plugins, and user expectations except where existing behavior is explicitly identified as incorrect.

No intentional breaking change is planned.

If fixing an issue reveals that a compatibility break is unavoidable, the implementation MUST document the break rather than silently introducing it.

Significant breaking changes SHOULD normally be deferred to a suitable feature release.

---

## 33. Documentation

Documentation updates SHOULD focus on user-observable changes.

Internal implementation details do not require user documentation unless they change an extension contract.

Completion documentation SHOULD describe behavior rather than internal completion architecture.

If existing documentation describes behavior contradicted by an included fix, it MUST be corrected.

---

## 34. ADR requirements

An ADR SHOULD be created or updated when implementation requires a durable architectural decision that is not already established.

Issue #134 is the most likely included issue to require one.

An ADR is appropriate if implementation establishes or changes the canonical relationship between:

- field schemas;
- value types;
- valid operators;
- expression evaluation;
- expression validation/checking;
- completion semantics.

An ADR MUST NOT be created merely to record routine implementation choices.

---

## 35. Recommended implementation order

The recommended implementation order is:

```text
Independent stabilization stream:
  #128
  #130
  #131
  #132

Completion stream:
  #133
    |
    v
  #134
    |
    v
  #135
    |
    v
  #136
```

The first four issues are largely independent and MAY be implemented in parallel where safe.

The completion issues SHOULD be treated as one workstream.

Within that workstream:

1. #133 establishes correct contextual fallback behavior;
2. #134 establishes expression-aware completion;
3. #135 builds value completion on top of that model;
4. #136 carries candidate documentation through to the UI.

Implementation may deviate from this exact commit order if a coherent architecture makes another sequence more practical.

Behavioral dependencies must nevertheless be respected.

---

## 36. Commit and issue traceability

Implementation commits SHOULD reference the GitHub issue they address.

Examples:

```text
fix(completion): make path fallback context-aware (#133)
```

```text
fix(repl): preserve output without trailing newline (#132)
```

A single commit MAY address multiple tightly coupled completion issues if separating the implementation would be artificial.

However, issue closure MUST remain traceable.

The implementation agent SHOULD avoid unrelated cleanup commits unless the cleanup is necessary for the included issue.

---

## 37. Release notes

The v0.6.1 release notes SHOULD present the release as stabilization rather than as a collection of internal tickets.

Suggested structure:

```markdown
## v0.6.1

v0.6.1 is a stabilization and polish release following v0.6.0.

Highlights:

- more reliable expression-aware completion;
- typed operator and value suggestions;
- documentation in completion candidates;
- corrected REPL handling of output without trailing newlines;
- more consistent plugin target resolution;
- improved permission-state correctness;
- more robust handling of external socket types.
```

Individual issue numbers MAY be included in a detailed changelog.

---

## 38. Versioning

The resulting release version MUST be:

```text
v0.6.1
```

This release MUST NOT be labeled `v0.7.0`.

The included work refines functionality already introduced before or in v0.6.0 and does not represent the next planned conceptual feature release.

All relevant crate, package, application, documentation, and release metadata version references MUST be updated consistently according to the project's established release process.

---

## 39. Definition of Done

v0.6.1 is complete when all of the following are true:

### Release organization

- [ ] the `v0.6.1` GitHub milestone exists;
- [ ] #128 is assigned to the milestone;
- [ ] #130 is assigned to the milestone;
- [ ] #131 is assigned to the milestone;
- [ ] #132 is assigned to the milestone;
- [ ] #133 is assigned to the milestone;
- [ ] #134 is assigned to the milestone;
- [ ] #135 is assigned to the milestone;
- [ ] #136 is assigned to the milestone;
- [ ] any additional issue intentionally included in the release is also part of the milestone.

### Issue completion

- [ ] #128 is resolved;
- [ ] #130 is resolved;
- [ ] #131 is resolved;
- [ ] #132 is resolved;
- [ ] #133 is resolved;
- [ ] #134 is resolved;
- [ ] #135 is resolved;
- [ ] #136 is resolved.

### Behavior

- [ ] completion behaves contextually rather than relying on blind filesystem fallback;
- [ ] expression completion understands field/operator/value/connector position;
- [ ] type-aware operator completion works;
- [ ] type-aware value completion works;
- [ ] candidate documentation reaches the completion UI;
- [ ] REPL output without a trailing newline remains visible;
- [ ] plugin target resolution behaves consistently across supported invocation forms;
- [ ] the included permission/scope issue is corrected;
- [ ] unknown socket types no longer unnecessarily destroy otherwise usable results.

### Quality

- [ ] regression tests exist for all fixed issues;
- [ ] the full test suite passes;
- [ ] formatting checks pass;
- [ ] linting passes;
- [ ] supported builds compile;
- [ ] relevant integration tests pass;
- [ ] documentation has been updated where necessary;
- [ ] no known v0.6.1 regression remains.

### Scope control

- [ ] #129 has not been accidentally absorbed into the release;
- [ ] #124-#127 have not been accidentally absorbed into the release;
- [ ] no planned v0.7 feature has been pulled forward unintentionally.

### Release

- [ ] release notes have been prepared;
- [ ] the project version is v0.6.1;
- [ ] the v0.6.1 build has been validated;
- [ ] the release has been published;
- [ ] the GitHub milestone has been closed after publication.

---

## 40. Non-goals

The following are explicitly not goals of v0.6.1:

- redesigning the entire completion architecture;
- implementing a full language server;
- IDE-style IntelliSense;
- dynamic remote value completion;
- general fuzzy-ranking infrastructure;
- redesigning the expression language;
- redesigning the plugin protocol;
- solving #129;
- binary-size optimization from #124-#127;
- implementing v0.7 features;
- speculative extensibility without a current use case.

---

## 41. Final release intent

v0.6.1 should be a deliberately boring release in the best possible sense.

It should not increase Ono-Sendai's conceptual surface area significantly.

Instead, it should make the system feel more deliberate:

- existing language behavior is easier to discover;
- completion understands what the user is doing;
- the REPL preserves output correctly;
- plugin targets behave consistently;
- permission state is correct;
- adapters tolerate unfamiliar external data better;
- the release inventory is visible and auditable through a GitHub milestone.

The outcome should be a stronger baseline for v0.7 rather than another layer of unfinished functionality.
